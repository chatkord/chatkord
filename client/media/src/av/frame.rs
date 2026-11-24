//! A picture or a hardware surface in FFmpeg's frame.

use super::error::{AvError, check};
use super::format::{ColourRange, ColourSpace, PixelFormat};
use ffmpeg_sys_next as ff;
use std::ffi::c_int;

/// How many frames and buffers `av` has allocated, so a test can show a route makes none per picture.
#[cfg(test)]
pub(crate) mod counted {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FRAMES: AtomicUsize = AtomicUsize::new(0);
    static BUFFERS: AtomicUsize = AtomicUsize::new(0);

    pub(crate) fn frame() {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn buffer() {
        BUFFERS.fetch_add(1, Ordering::Relaxed);
    }

    /// The frames and the picture buffers made so far, in this process.
    pub(crate) fn made() -> (usize, usize) {
        (FRAMES.load(Ordering::Relaxed), BUFFERS.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
fn count_frame() {
    counted::frame();
}
#[cfg(not(test))]
fn count_frame() {}

#[cfg(test)]
fn count_buffer() {
    counted::buffer();
}
#[cfg(not(test))]
fn count_buffer() {}

/// An FFmpeg frame, freed when dropped.
pub struct Frame(pub(super) *mut ff::AVFrame);

/// One plane of a frame in memory, as its rows lie.
#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
    /// The plane's bytes from its first row to the last byte of its last row.
    pub bytes: &'a [u8],
    /// The bytes from the start of one row to the start of the next, at least a row.
    pub stride: usize,
}

/// One plane of a frame in memory, written.
#[derive(Debug)]
pub struct PlaneMut<'a> {
    /// The plane's bytes from its first row to the last byte of its last row.
    pub bytes: &'a mut [u8],
    /// The bytes from the start of one row to the start of the next, at least a row.
    pub stride: usize,
}

fn side(value: u32) -> Result<c_int, AvError> {
    c_int::try_from(value).map_err(|_| AvError::Invalid)
}

impl Frame {
    /// An empty frame.
    pub fn new() -> Option<Self> {
        // SAFETY: allocation has no preconditions; null is handled.
        let raw = unsafe { ff::av_frame_alloc() };
        count_frame();
        (!raw.is_null()).then_some(Self(raw))
    }

    /// A frame in memory of `format` and `size`, with buffers of its own.
    pub fn software(format: PixelFormat, size: (u32, u32)) -> Result<Self, AvError> {
        let mut frame = Self::new().ok_or(AvError::NoMemory)?;
        frame.allocate(format, size)?;
        Ok(frame)
    }

    /// Gives an empty frame `format`, `size` and buffers of its own.
    fn allocate(&mut self, format: PixelFormat, (width, height): (u32, u32)) -> Result<(), AvError> {
        let (width, height) = (side(width)?, side(height)?);
        // SAFETY: the frame is owned, and nothing else reaches it while this borrow lives.
        let fresh = unsafe { &mut *self.0 };
        fresh.format = format.to_c() as c_int;
        (fresh.width, fresh.height) = (width, height);
        count_buffer();
        // SAFETY: the frame's format and size are set; its buffers are allocated for them and owned by it.
        check(unsafe { ff::av_frame_get_buffer(self.0, 0) }).map(drop)
    }

    /// Another frame that shares this one's buffers.
    pub fn reference(&self) -> Option<Self> {
        let other = Self::new()?;
        // SAFETY: both frames are owned; `other` is empty, and the call gives it its own reference to the buffers.
        (unsafe { ff::av_frame_ref(other.0, self.0) } >= 0).then_some(other)
    }

    /// Makes this frame another reference to `from`'s buffers, letting go of what it held.
    pub fn refer_to(&mut self, from: &Self) -> Result<(), AvError> {
        self.unref();
        // SAFETY: both frames are owned and this one is empty; the call gives it its own reference to the buffers.
        check(unsafe { ff::av_frame_ref(self.0, from.0) }).map(drop)
    }

    /// Makes this frame an NV12 picture of `size` that reads `owner`'s bytes where they are, packed luma then the
    /// interleaved chroma, letting go of what it held; FFmpeg may not write them, and `owner` lives until the last
    /// reference to them lets go.
    pub fn read_nv12<T: AsRef<[u8]> + Send + Sync + 'static>(
        &mut self,
        owner: std::sync::Arc<T>,
        (width, height): (u32, u32),
    ) -> Result<(), AvError> {
        let pixels = usize::try_from(width).ok().zip(usize::try_from(height).ok()).and_then(|(w, h)| w.checked_mul(h));
        let bytes = (*owner).as_ref();
        let (Some(luma), Ok(stride), Ok(rows)) = (pixels, side(width), side(height)) else {
            return Err(AvError::Invalid);
        };
        if bytes.len() < luma + luma / 2 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(AvError::Invalid);
        }
        let (start, len) = (bytes.as_ptr().cast_mut(), bytes.len());
        self.unref();
        let held = std::sync::Arc::into_raw(owner).cast_mut().cast::<std::ffi::c_void>();
        let read_only = ff::AV_BUFFER_FLAG_READONLY;
        // SAFETY: `start` and `len` are `held`'s bytes, which `release_owner` keeps alive until FFmpeg calls it once,
        // with `held`, when the last reference to the buffer goes; read-only, FFmpeg never writes them.
        let buffer = unsafe { ff::av_buffer_create(start, len, Some(release_owner::<T>), held, read_only) };
        if buffer.is_null() {
            // SAFETY: the buffer that would have taken `held` back was not made, so it is taken back here, once.
            drop(unsafe { std::sync::Arc::from_raw(held.cast_const().cast::<T>()) });
            return Err(AvError::NoMemory);
        }
        let fields = self.fields_mut();
        fields.buf[0] = buffer;
        fields.data[0] = start;
        // The chroma starts after the luma's bytes, inside the same buffer.
        fields.data[1] = start.wrapping_add(luma);
        (fields.linesize[0], fields.linesize[1]) = (stride, stride);
        fields.format = PixelFormat::Nv12.to_c() as c_int;
        (fields.width, fields.height) = (stride, rows);
        Ok(())
    }

    /// Lets go of the picture it holds, so it can take the next.
    pub fn unref(&mut self) {
        // SAFETY: the frame is owned; `av_frame_unref` leaves it empty and reusable.
        unsafe { ff::av_frame_unref(self.0) };
    }

    fn fields(&self) -> &ff::AVFrame {
        // SAFETY: the frame is owned and live; only plain fields are read.
        unsafe { &*self.0 }
    }

    fn fields_mut(&mut self) -> &mut ff::AVFrame {
        // SAFETY: the frame is owned and live, and `&mut self` is the one way to it.
        unsafe { &mut *self.0 }
    }

    /// Pixels across and down; zero where the frame holds none.
    pub fn size(&self) -> (u32, u32) {
        let fields = self.fields();
        (u32::try_from(fields.width).unwrap_or(0), u32::try_from(fields.height).unwrap_or(0))
    }

    /// Its format, none for one this client does not name.
    pub fn format(&self) -> Option<PixelFormat> {
        PixelFormat::from_raw(self.fields().format)
    }

    /// Says the samples are `format`, as a view of the same picture under another name.
    #[cfg(target_os = "linux")]
    pub fn set_format(&mut self, format: PixelFormat) {
        self.fields_mut().format = format.to_c() as c_int;
    }

    /// Whether the picture is on a hardware device rather than in memory.
    pub fn is_hardware(&self) -> bool {
        !self.fields().hw_frames_ctx.is_null()
    }

    /// How a hardware frame's samples are laid out in memory, none for a frame in memory.
    pub fn hardware_layout(&self) -> Option<PixelFormat> {
        let context = self.fields().hw_frames_ctx;
        // SAFETY: a frame's frames context is live while the frame is; null is handled.
        let frames = unsafe { context.as_ref() }?;
        // SAFETY: a frames context's buffer data is its `AVHWFramesContext`.
        PixelFormat::from_c(unsafe { (*frames.data.cast::<ff::AVHWFramesContext>()).sw_format })
    }

    /// The timestamp it carries.
    pub fn pts(&self) -> i64 {
        self.fields().pts
    }

    /// Marks the picture's place in the stream and whether an encoder is to make it a key frame.
    pub fn stamp(&mut self, pts: i64, key_frame: bool) {
        let fields = self.fields_mut();
        fields.pts = pts;
        fields.pict_type =
            if key_frame { ff::AVPictureType::AV_PICTURE_TYPE_I } else { ff::AVPictureType::AV_PICTURE_TYPE_NONE };
    }

    /// Sets the timestamp.
    pub fn set_pts(&mut self, pts: i64) {
        self.fields_mut().pts = pts;
    }

    /// The matrix its colour differences are tagged with.
    pub fn colour_space(&self) -> ColourSpace {
        ColourSpace::read(self.fields().colorspace)
    }

    /// The range its samples are tagged with.
    pub fn colour_range(&self) -> ColourRange {
        ColourRange::read(self.fields().color_range)
    }

    /// Tags the matrix.
    pub fn set_colour_space(&mut self, space: ColourSpace) {
        self.fields_mut().colorspace = space.to_c();
    }

    /// Tags the range.
    pub fn set_colour_range(&mut self, range: ColourRange) {
        self.fields_mut().color_range = range.to_c();
    }

    /// Tags the picture as BT.709 in limited range, whole: matrix, primaries, transfer and range.
    #[cfg(any(windows, test))]
    pub fn tag_bt709_limited(&mut self) {
        self.set_colour_space(ColourSpace::Bt709);
        self.set_colour_range(ColourRange::Limited);
        let fields = self.fields_mut();
        fields.color_primaries = super::format::BT709_PRIMARIES;
        fields.color_trc = super::format::BT709_TRANSFER;
    }

    /// Whether the frame's buffers are its own, to be written.
    pub fn is_writable(&self) -> bool {
        // SAFETY: the frame is owned and live.
        !self.fields().buf[0].is_null() && unsafe { ff::av_frame_is_writable(self.0) } > 0
    }

    /// Plane `index` of a frame in memory; none for a plane its format lacks or a frame on a device.
    pub fn plane(&self, index: usize) -> Option<Plane<'_>> {
        let (data, stride, len) = self.plane_extent(index)?;
        // SAFETY: the frame's plane holds its rows at `stride` from `data`, `len` bytes to the end of the last, alive
        // while the frame is and read only through this borrow.
        Some(Plane { bytes: unsafe { std::slice::from_raw_parts(data, len) }, stride })
    }

    /// Plane `index` of a frame in memory, to be written; none for a plane its format lacks or a frame on a device.
    pub fn plane_mut(&mut self, index: usize) -> Option<PlaneMut<'_>> {
        let (data, stride, len) = self.plane_extent(index)?;
        // SAFETY: as in `plane`, and `&mut self` is the one way to the frame meanwhile.
        Some(PlaneMut { bytes: unsafe { std::slice::from_raw_parts_mut(data, len) }, stride })
    }

    /// Where plane `index` starts, its stride, and the bytes from its first row to the end of its last.
    fn plane_extent(&self, index: usize) -> Option<(*mut u8, usize, usize)> {
        let format = self.format()?;
        let fields = self.fields();
        let (width, height) = (fields.width, fields.height);
        let data = *fields.data.get(index)?;
        let row = format.row_bytes(width, index)?;
        let rows = format.plane_rows(index, height)?;
        let stride = usize::try_from(*fields.linesize.get(index)?).ok().filter(|stride| *stride >= row)?;
        let len = rows.checked_sub(1)?.checked_mul(stride)?.checked_add(row)?;
        (!data.is_null() && fields.hw_frames_ctx.is_null()).then_some((data, stride, len))
    }

    /// The hardware frame `from` brought to memory into this frame: in place if this frame is writable and already
    /// holds that layout and size, else in buffers allocated anew.
    pub fn fetch(&mut self, from: &Self) -> Result<(), AvError> {
        let reusable = self.is_writable() && self.format() == from.hardware_layout() && self.size() == from.size();
        if !reusable {
            self.unref();
            count_buffer();
        }
        // SAFETY: `from` is on a hardware frames context and this frame is empty or holds buffers of its layout.
        check(unsafe { ff::av_hwframe_transfer_data(self.0, from.0, 0) }).map(drop)
    }

    /// This frame in memory uploaded into `hardware`, a frame of a device's pool, which it must match in layout and
    /// size.
    #[cfg(any(test, feature = "testing"))]
    pub fn upload_into(&self, hardware: &mut Self) -> Result<(), AvError> {
        // SAFETY: `hardware` is a frame of a frames context and this frame a picture in memory of its layout and size.
        check(unsafe { ff::av_hwframe_transfer_data(hardware.0, self.0, 0) }).map(drop)
    }

    /// The VA-API surface a VA-API frame names, none for any other frame.
    #[cfg(target_os = "linux")]
    pub fn vaapi_surface(&self) -> Option<u32> {
        if self.fields().format != PixelFormat::Vaapi.to_c() as c_int {
            return None;
        }
        // A VA-API picture names its surface in its fourth plane pointer.
        u32::try_from(self.fields().data[3].addr()).ok()
    }

    /// The VA-API display of a VA-API frame, none for any other.
    #[cfg(target_os = "linux")]
    pub fn vaapi_display(&self) -> Option<super::device::VaDisplay> {
        // SAFETY: a frame's frames context is live while the frame is; null is handled.
        let frames = unsafe { self.fields().hw_frames_ctx.as_ref() }?;
        // SAFETY: a frames context's buffer data is its `AVHWFramesContext`, whose device context is live with it.
        let device = unsafe { (*frames.data.cast::<ff::AVHWFramesContext>()).device_ctx };
        // SAFETY: as above.
        let device = unsafe { device.as_ref() }?;
        if device.type_ != ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI {
            return None;
        }
        // SAFETY: a VA-API device context's hwctx is its `AVVAAPIDeviceContext`.
        Some(super::device::VaDisplay(unsafe { (*device.hwctx.cast::<ff::AVVAAPIDeviceContext>()).display }))
    }

    /// A VA-API frame of `frames` that names `surface`, which `owner` keeps alive until the last reference to the
    /// frame lets go, of `size` cropped to its top-left `read`.
    #[cfg(target_os = "linux")]
    pub fn vaapi<T: Send + Sync + 'static>(
        frames: &super::frames::HwFrames,
        surface: u32,
        owner: std::sync::Arc<T>,
        (width, height): (u32, u32),
        read: (u32, u32),
    ) -> Result<Self, AvError> {
        let (Some(crop_right), Some(crop_bottom)) = (width.checked_sub(read.0), height.checked_sub(read.1)) else {
            return Err(AvError::Invalid);
        };
        let frame = Self::new().ok_or(AvError::NoMemory)?;
        let held = std::sync::Arc::into_raw(owner);
        // SAFETY: `held` is a live `Arc`'s pointer, which `release` takes back once, when FFmpeg lets the buffer go.
        let buffer = unsafe {
            ff::av_buffer_create(held.cast_mut().cast(), size_of::<T>(), Some(release::<T>), std::ptr::null_mut(), 0)
        };
        if buffer.is_null() {
            // SAFETY: the buffer that would have taken `held` back was not made, so it is taken back here, once.
            drop(unsafe { std::sync::Arc::from_raw(held) });
            return Err(AvError::NoMemory);
        }
        // SAFETY: the frames context is live; the new reference is the frame's, which frees it.
        let context = unsafe { ff::av_buffer_ref(frames.0) };
        // SAFETY: `frame` is owned and empty, and nothing else reaches it while this borrow lives.
        let fields = unsafe { &mut *frame.0 };
        fields.buf[0] = buffer;
        fields.hw_frames_ctx = context;
        fields.format = PixelFormat::Vaapi.to_c() as c_int;
        fields.width = side(width)?;
        fields.height = side(height)?;
        fields.data[3] = std::ptr::without_provenance_mut(surface as usize);
        fields.crop_right = crop_right as usize;
        fields.crop_bottom = crop_bottom as usize;
        if context.is_null() {
            return Err(AvError::NoMemory);
        }
        Ok(frame)
    }
}

/// Takes back the `Arc` a frame made by [`Frame::vaapi`] holds.
#[cfg(target_os = "linux")]
unsafe extern "C" fn release<T>(_: *mut std::ffi::c_void, data: *mut u8) {
    // SAFETY: `data` is the pointer `Arc::into_raw` gave in `vaapi`; FFmpeg calls this once, for the last reference.
    drop(unsafe { std::sync::Arc::from_raw(data.cast_const().cast::<T>()) });
}

/// Takes back the `Arc` a frame made by [`Frame::read_nv12`] holds.
unsafe extern "C" fn release_owner<T>(owner: *mut std::ffi::c_void, _: *mut u8) {
    // SAFETY: `owner` is the pointer `Arc::into_raw` gave in `read_nv12`; FFmpeg calls this once, for the last
    // reference.
    drop(unsafe { std::sync::Arc::from_raw(owner.cast_const().cast::<T>()) });
}

impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `av_frame_alloc` and is freed once.
        unsafe { ff::av_frame_free(&raw mut self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_tagged_bt709_limited_reads_back_whole() {
        let mut frame = Frame::new().expect("a frame");
        frame.tag_bt709_limited();
        assert_eq!(frame.colour_space(), ColourSpace::Bt709);
        assert_eq!(frame.fields().color_primaries, super::super::format::BT709_PRIMARIES);
        assert_eq!(frame.fields().color_trc, super::super::format::BT709_TRANSFER);
        assert_eq!(frame.colour_range(), ColourRange::Limited);
    }
}
