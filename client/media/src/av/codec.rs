//! A codec context: an encoder or decoder, set up with typed settings, opened, and fed.

use super::device::HwDevice;
use super::error::{AvError, check};
use super::format::{ColourRange, ColourSpace, PixelFormat};
use super::frame::Frame;
use super::frames::HwFrames;
use super::packet::Packet;
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, c_int};
use std::ptr;

/// The padding FFmpeg's readers may touch past the end of a packet's bytes.
pub const INPUT_PADDING: usize = ff::AV_INPUT_BUFFER_PADDING_SIZE as usize;

/// A codec of this FFmpeg build: a row of its static table.
#[derive(Clone, Copy, Debug)]
pub struct CodecRef(*const ff::AVCodec);

// SAFETY: a codec is a static, read-only row of FFmpeg's table.
unsafe impl Send for CodecRef {}
// SAFETY: as above.
unsafe impl Sync for CodecRef {}

impl CodecRef {
    /// The encoder FFmpeg calls `name`; none when it was built without it.
    pub fn encoder(name: &CStr) -> Option<Self> {
        // SAFETY: the name is NUL terminated; null is handled.
        let codec = unsafe { ff::avcodec_find_encoder_by_name(name.as_ptr()) };
        (!codec.is_null()).then_some(Self(codec))
    }

    /// Whether the encoder takes BGRA pictures from a hardware device, as AMF's do.
    #[cfg(windows)]
    pub fn takes_bgra(self) -> bool {
        let (mut listed, mut count): (*const std::ffi::c_void, c_int) = (ptr::null(), 0);
        // SAFETY: a codec of FFmpeg's table, asked of no context; the list is FFmpeg's own, read only.
        let asked = unsafe {
            ff::avcodec_get_supported_config(
                ptr::null(),
                self.0,
                ff::AVCodecConfig::AV_CODEC_CONFIG_PIX_FORMAT,
                0,
                &raw mut listed,
                &raw mut count,
            )
        };
        if asked < 0 || listed.is_null() {
            return false;
        }
        // SAFETY: FFmpeg's list of `count` pixel formats for this codec, alive for the program's run.
        let formats = unsafe {
            std::slice::from_raw_parts(listed.cast::<ff::AVPixelFormat>(), usize::try_from(count).unwrap_or(0))
        };
        formats.contains(&PixelFormat::Bgra.to_c())
    }

    /// The decoder FFmpeg calls `name`; none when it was built without it.
    pub fn decoder(name: &CStr) -> Option<Self> {
        // SAFETY: the name is NUL terminated; null is handled.
        let codec = unsafe { ff::avcodec_find_decoder_by_name(name.as_ptr()) };
        (!codec.is_null()).then_some(Self(codec))
    }
}

/// The names of the HEVC and VP9 codecs this build carries, which nothing here sends.
#[cfg(test)]
pub fn unwanted_codec_names() -> Vec<String> {
    let mut opaque = ptr::null_mut();
    let mut carried = Vec::new();
    loop {
        // SAFETY: FFmpeg's own iterator over its static codec table, which `opaque` walks from null to the end.
        let next = unsafe { ff::av_codec_iterate(&raw mut opaque) };
        // SAFETY: a codec the iterator returned is a static entry of that table; null is its end.
        let Some(codec) = (unsafe { next.as_ref() }) else { break };
        if matches!(codec.id, ff::AVCodecID::AV_CODEC_ID_HEVC | ff::AVCodecID::AV_CODEC_ID_VP9) {
            // SAFETY: every codec in the table names itself with a static NUL-terminated string.
            carried.push(unsafe { CStr::from_ptr(codec.name) }.to_string_lossy().into_owned());
        }
    }
    carried
}

/// The codecs a stream is of.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub enum StreamCodec {
    /// H.264.
    H264,
    /// AV1.
    Av1,
}

/// The rate fields of a context, in FFmpeg's units.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rates {
    /// The average, bits a second.
    pub average: i64,
    /// The peak, bits a second.
    pub peak: i64,
    /// The rate buffer, bits.
    pub buffer: c_int,
    /// What the buffer holds at the start, bits.
    pub initial: c_int,
}

/// An FFmpeg codec context, freed when dropped.
pub struct CodecContext {
    raw: *mut ff::AVCodecContext,
    codec: Option<CodecRef>,
}

impl CodecContext {
    /// A context for `codec`, or for no codec in particular, which is never opened.
    pub fn new(codec: Option<CodecRef>) -> Option<Self> {
        // SAFETY: the codec is a table row or null; the context is owned from here.
        let raw = unsafe { ff::avcodec_alloc_context3(codec.map_or(ptr::null(), |codec| codec.0)) };
        (!raw.is_null()).then_some(Self { raw, codec })
    }

    fn fields(&self) -> &ff::AVCodecContext {
        // SAFETY: the context is owned and live; plain fields are read.
        unsafe { &*self.raw }
    }

    fn fields_mut(&mut self) -> &mut ff::AVCodecContext {
        // SAFETY: the context is owned and live, and `&mut self` is the one way to it.
        unsafe { &mut *self.raw }
    }

    /// The context, for the calls in this module that read it.
    pub(super) fn raw(&self) -> *const ff::AVCodecContext {
        self.raw
    }

    /// The units its timestamps count in.
    pub(super) fn time_base(&self) -> ff::AVRational {
        self.fields().time_base
    }

    /// Whether it codes H.264.
    pub fn is_h264(&self) -> bool {
        self.fields().codec_id == ff::AVCodecID::AV_CODEC_ID_H264
    }

    /// Says the stream is `codec`'s video at 30 a second, for a context made for no codec.
    #[cfg(test)]
    pub fn describe(&mut self, codec: StreamCodec) {
        let fields = self.fields_mut();
        fields.codec_type = ff::AVMediaType::AVMEDIA_TYPE_VIDEO;
        fields.codec_id = match codec {
            StreamCodec::H264 => ff::AVCodecID::AV_CODEC_ID_H264,
            StreamCodec::Av1 => ff::AVCodecID::AV_CODEC_ID_AV1,
        };
        fields.time_base = ff::AVRational { num: 1, den: 30 };
    }

    /// The format of the pictures an encoder is given.
    pub fn set_pixel_format(&mut self, format: PixelFormat) {
        self.fields_mut().pix_fmt = format.to_c();
    }

    /// The picture's width and height.
    pub fn set_size(&mut self, width: c_int, height: c_int) {
        let fields = self.fields_mut();
        fields.width = width;
        fields.height = height;
    }

    /// The picture's width and height, zero where unset.
    pub fn size(&self) -> (u32, u32) {
        let fields = self.fields();
        (u32::try_from(fields.width).unwrap_or(0), u32::try_from(fields.height).unwrap_or(0))
    }

    /// The width and height an open encoder codes its pictures at: its size, or more where it pads them, which it says
    /// as its coded size (Vulkan's) or as the crop it asks of the stream (AMF's AV1).
    pub fn padded_size(&self) -> (u32, u32) {
        let fields = self.fields();
        let side = |value: c_int| u32::try_from(value).unwrap_or(0);
        let (width, height) = (side(fields.width), side(fields.height));
        let crop = usize::try_from(fields.nb_coded_side_data).ok().and_then(|count| {
            // SAFETY: an open encoder's coded side data is its own array of `count` entries, or null with none.
            let found = unsafe {
                ff::av_packet_side_data_get(
                    fields.coded_side_data,
                    c_int::try_from(count).ok()?,
                    ff::AVPacketSideDataType::AV_PKT_DATA_FRAME_CROPPING,
                )
            };
            // SAFETY: a found entry is a live element of that array.
            let entry = unsafe { found.as_ref() }?;
            // SAFETY: the entry's data is `size` bytes the encoder wrote.
            let bytes = unsafe { std::slice::from_raw_parts(entry.data, entry.size) };
            // Four little-endian 32-bit counts: top, bottom, left, right.
            let count = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
            Some((count(12)?.saturating_add(count(8)?), count(0)?.saturating_add(count(4)?)))
        });
        let (across, down) = crop.unwrap_or((0, 0));
        (
            side(fields.coded_width).max(width.saturating_add(across)),
            side(fields.coded_height).max(height.saturating_add(down)),
        )
    }

    /// Pictures a second: the time base is their reciprocal.
    pub fn set_frame_rate(&mut self, per_second: c_int) {
        let fields = self.fields_mut();
        fields.time_base = ff::AVRational { num: 1, den: per_second };
        fields.framerate = ff::AVRational { num: per_second, den: 1 };
    }

    /// The whole pictures a second it was set to, zero where unset.
    pub fn frame_rate(&self) -> u32 {
        let rate = self.fields().framerate;
        u32::try_from(rate.num / rate.den.max(1)).unwrap_or(0)
    }

    /// A key frame at least this often, in frames.
    pub fn set_gop(&mut self, frames: c_int) {
        self.fields_mut().gop_size = frames;
    }

    /// The most pictures an encoder may reorder.
    pub fn set_max_b_frames(&mut self, frames: c_int) {
        self.fields_mut().max_b_frames = frames;
    }

    /// What the stream says its pictures are. Primaries and transfer are BT.709's for every matrix: a camera's BT.601
    /// matrix sits on sRGB's primaries and curve.
    pub fn set_tags(&mut self, space: ColourSpace, range: ColourRange) {
        let fields = self.fields_mut();
        fields.colorspace = space.to_c();
        fields.color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;
        fields.color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
        fields.color_range = range.to_c();
    }

    /// Asks the codec for the lowest delay it has.
    pub fn set_low_delay(&mut self) {
        self.fields_mut().flags |= ff::AV_CODEC_FLAG_LOW_DELAY.cast_signed();
    }

    /// Codes H.264 as High.
    pub fn set_h264_high(&mut self) {
        self.fields_mut().profile = ff::AV_PROFILE_H264_HIGH as c_int;
    }

    /// Sets the level of an H.264 stream.
    pub fn set_level(&mut self, level: c_int) {
        self.fields_mut().level = level;
    }

    /// Draws the encoder's pictures from `frames`; false when FFmpeg cannot reference them.
    pub fn set_hw_frames(&mut self, frames: &HwFrames) -> bool {
        // SAFETY: `frames.0` is a live frames context; the new reference is the codec context's, which frees it.
        let reference = unsafe { ff::av_buffer_ref(frames.0) };
        let fields = self.fields_mut();
        // SAFETY: the old reference, if any, is the context's own, given back before it is replaced.
        unsafe { ff::av_buffer_unref(&raw mut fields.hw_frames_ctx) };
        fields.hw_frames_ctx = reference;
        !reference.is_null()
    }

    /// Decodes on `device`, accepting only pictures in `wanted`, the device's own format; false when FFmpeg cannot
    /// reference the device.
    pub fn set_hw_decode(&mut self, device: &HwDevice, wanted: PixelFormat) -> bool {
        // SAFETY: the device is a live device context; the new reference is the codec context's.
        let reference = unsafe { ff::av_buffer_ref(device.0) };
        let fields = self.fields_mut();
        // SAFETY: the old reference, if any, is the context's own, given back before it is replaced.
        unsafe { ff::av_buffer_unref(&raw mut fields.hw_device_ctx) };
        fields.hw_device_ctx = reference;
        fields.get_format = Some(pick_hardware);
        fields.opaque = wanted.to_c() as usize as *mut _;
        !reference.is_null()
    }

    /// The most pixels a picture of the stream may have; a larger one is refused.
    pub fn set_max_pixels(&mut self, pixels: i64) {
        self.fields_mut().max_pixels = pixels;
    }

    /// How many threads decode at once.
    pub fn set_threads(&mut self, threads: c_int) {
        self.fields_mut().thread_count = threads;
    }

    /// Sets the codec's private option `name` to `value`; false when it refuses.
    pub fn set_option(&mut self, name: &CStr, value: &CStr) -> bool {
        // SAFETY: a codec's private options live in its `priv_data`, allocated with the context.
        unsafe { ff::av_opt_set(self.fields().priv_data, name.as_ptr(), value.as_ptr(), 0) >= 0 }
    }

    /// Sets the codec's private option `name` to the number `value`; false when it refuses.
    pub fn set_option_int(&mut self, name: &CStr, value: i64) -> bool {
        // SAFETY: as in `set_option`.
        unsafe { ff::av_opt_set_int(self.fields().priv_data, name.as_ptr(), value, 0) >= 0 }
    }

    /// The codec's private option `name`, a number.
    #[cfg(all(test, windows))]
    pub fn option_int(&self, name: &CStr) -> Option<i64> {
        let mut value = 0;
        // SAFETY: as in `set_option`; `value` is a valid out-pointer.
        let read = unsafe { ff::av_opt_get_int(self.fields().priv_data, name.as_ptr(), 0, &raw mut value) };
        (read >= 0).then_some(value)
    }

    /// The rate fields: the average, the peak, and a buffer that starts full.
    pub fn set_rates(&mut self, average: i64, peak: i64, buffer: c_int) {
        let fields = self.fields_mut();
        fields.bit_rate = average;
        fields.rc_max_rate = peak;
        fields.rc_buffer_size = buffer;
        fields.rc_initial_buffer_occupancy = buffer;
    }

    /// The rate buffer, in bits.
    pub fn buffer_size(&self) -> c_int {
        self.fields().rc_buffer_size
    }

    /// The rate fields as set.
    #[cfg(test)]
    pub fn rates(&self) -> Rates {
        let fields = self.fields();
        Rates {
            average: fields.bit_rate,
            peak: fields.rc_max_rate,
            buffer: fields.rc_buffer_size,
            initial: fields.rc_initial_buffer_occupancy,
        }
    }

    /// Opens the codec it was made for. An open on a hardware device or frame pool is where the driver gets ready, so
    /// it enters the loader as a device creation does and takes its lock; a software codec's open never does.
    pub fn open(&mut self) -> Result<(), AvError> {
        let codec = self.codec.ok_or(AvError::Invalid)?;
        crate::log::forward();
        #[cfg(target_os = "linux")]
        let _alone = {
            let fields = self.fields();
            (!fields.hw_device_ctx.is_null() || !fields.hw_frames_ctx.is_null()).then(crate::loader::alone)
        };
        // SAFETY: the context was allocated for `codec` and is not opened yet; a null out-pointer is how FFmpeg is
        // told there is no extra data.
        check(unsafe { ff::avcodec_open2(self.raw, codec.0, ptr::null_mut()) }).map(drop)
    }

    /// Sends a picture to the encoder, which takes a reference to it; `None` ends the input.
    pub fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), AvError> {
        // SAFETY: the context is open; a null frame is how FFmpeg is told the input has ended.
        check(unsafe { ff::avcodec_send_frame(self.raw, frame.map_or(ptr::null(), |frame| frame.0)) }).map(drop)
    }

    /// Takes a finished unit from the encoder into `packet`, which is empty.
    pub fn receive_packet(&mut self, packet: &mut Packet) -> Result<(), AvError> {
        // SAFETY: the context is open and the packet is owned and empty.
        check(unsafe { ff::avcodec_receive_packet(self.raw, packet.0) }).map(drop)
    }

    /// Sends a unit to the decoder, which takes a reference to what it keeps; a refused packet is left as it was.
    /// `None` ends the input.
    pub fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), AvError> {
        // SAFETY: the context is open; a null packet tells FFmpeg the input has ended.
        check(unsafe { ff::avcodec_send_packet(self.raw, packet.map_or(ptr::null(), |packet| packet.0)) }).map(drop)
    }

    /// Sends the first `length` bytes of `padded` to the decoder through `packet`, which holds no buffer of its own:
    /// the decoder copies a packet with none, so nothing points into `padded` after this returns. `padded` ends in
    /// the [`INPUT_PADDING`] zeros FFmpeg's readers may touch.
    pub fn send_padded(&mut self, packet: &mut Packet, padded: &[u8], length: usize) -> Result<(), AvError> {
        let size = c_int::try_from(length).map_err(|_| AvError::Invalid)?;
        if padded.len() < length.saturating_add(INPUT_PADDING) {
            return Err(AvError::Invalid);
        }
        // SAFETY: the packet is owned and holds no buffer; it points at `padded`'s bytes for the send alone.
        let held = unsafe { &mut *packet.0 };
        (held.data, held.size) = (padded.as_ptr().cast_mut(), size);
        // SAFETY: the context is open and the packet is complete, with the padding behind its bytes.
        let sent = check(unsafe { ff::avcodec_send_packet(self.raw, packet.0) });
        // SAFETY: the packet lets go of the pointer before `padded` can go.
        let held = unsafe { &mut *packet.0 };
        (held.data, held.size) = (ptr::null_mut(), 0);
        sent.map(drop)
    }

    /// Takes a finished picture from the decoder into `frame`, which is empty.
    pub fn receive_frame(&mut self, frame: &mut Frame) -> Result<(), AvError> {
        // SAFETY: the context is open and the frame is owned and empty.
        check(unsafe { ff::avcodec_receive_frame(self.raw, frame.0) }).map(drop)
    }
}

impl Drop for CodecContext {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `avcodec_alloc_context3` and is freed once.
        unsafe { ff::avcodec_free_context(&raw mut self.raw) };
    }
}

/// FFmpeg asks which of the formats a decoder offers to use; only the device's own will do.
unsafe extern "C" fn pick_hardware(
    context: *mut ff::AVCodecContext,
    offered: *const ff::AVPixelFormat,
) -> ff::AVPixelFormat {
    // SAFETY: FFmpeg passes the context it is opening, whose `opaque` `set_hw_decode` set to the wanted format.
    let Ok(wanted) = c_int::try_from(unsafe { (*context).opaque }.addr()) else {
        return ff::AVPixelFormat::AV_PIX_FMT_NONE;
    };
    let mut at = offered;
    loop {
        // SAFETY: FFmpeg passes a list ended by AV_PIX_FMT_NONE, and `at` has not passed that end.
        let format = unsafe { *at };
        if format == ff::AVPixelFormat::AV_PIX_FMT_NONE || format as c_int == wanted {
            return format;
        }
        // SAFETY: `at` is not the end of the list, so the next format is in it too.
        at = unsafe { at.add(1) };
    }
}
