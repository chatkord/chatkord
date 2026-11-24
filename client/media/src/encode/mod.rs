//! Frames in, an encoded stream out, on an encoder the probe proved opens. Pictures come from memory or from the
//! screen's GPU buffer, which is read and converted on the GPU; a software encoder's are then brought to memory.

pub(crate) mod camera;
mod filler;
mod in_flight;
#[cfg(target_os = "linux")]
mod lift;
#[cfg(target_os = "linux")]
mod linux;
mod retired;
#[cfg(any(target_os = "linux", windows))]
mod screen;
mod to_nv12;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use lift::{Inputs, Lift, Lifted};
#[cfg(target_os = "linux")]
pub(crate) use screen::vulkan_on;

use crate::av::{
    AvError, CodecContext, Conversion, Frame, HwDevice, HwFrames, Packet, PixelFormat, Scaler, View, ViewMut,
};
use crate::coded::{Coded, CodedError};
use crate::colours::Colours;
use crate::planes::Planes;
use crate::probe::{Backend, Device, Encoder, Runs, nv12_pool};
use crate::tuning::{self, Name};
use capture::Layout;
use filler::Filler;
use in_flight::InFlight;
use std::ffi::c_int;
use std::fmt;
use std::mem::ManuallyDrop;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A frame in NV12, tightly packed: `width * height` luma bytes, then `width * height / 2` of interleaved chroma.
/// Both sides are even and nonzero. Its bytes are shared, never written: a clone, or a software encoder given them, reads
/// the same ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nv12 {
    width: usize,
    height: usize,
    data: Arc<Vec<u8>>,
    colours: Colours,
}

impl Nv12 {
    /// The frame `data` holds, in [`Colours::HD`], refused unless both sides are even and nonzero and `data` is
    /// exactly their size.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self, FrameError> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FrameError::Size);
        }
        match width.checked_mul(height).and_then(|pixels| pixels.checked_add(pixels / 2)) {
            Some(expected) if expected == data.len() => {
                Ok(Self { width, height, data: Arc::new(data), colours: Colours::HD })
            }
            _ => Err(FrameError::Length),
        }
    }

    /// The same frame, its samples in `colours`.
    #[must_use]
    pub fn in_colours(self, colours: Colours) -> Self {
        Self { colours, ..self }
    }

    /// The matrix and range its samples are in.
    pub fn colours(&self) -> Colours {
        self.colours
    }

    /// Even and nonzero.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Even and nonzero.
    pub fn height(&self) -> usize {
        self.height
    }

    /// `width * height` bytes, one row after another.
    pub fn luma(&self) -> &[u8] {
        &self.data[..self.width * self.height]
    }

    /// Half the luma's bytes: the two colour differences interleaved, at half the size each way.
    pub fn chroma(&self) -> &[u8] {
        &self.data[self.width * self.height..]
    }
}

/// Why bytes are not a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// A side is zero or odd.
    Size,
    /// The bytes are not `width * height * 3 / 2`.
    Length,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Size => f.write_str("a frame needs even, nonzero sides"),
            Self::Length => f.write_str("the bytes are not the size of an NV12 frame"),
        }
    }
}

impl std::error::Error for FrameError {}

/// What one encode session is asked to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The stream's pixels across, nonzero.
    pub width: usize,
    /// The stream's pixels down, nonzero.
    pub height: usize,
    /// Nonzero; a frame's timestamp counts in its reciprocal.
    pub frames_per_second: u32,
    /// The rate the encoder aims at, in bits per second. A budget, not a cap.
    pub bits_per_second: u32,
    /// A key frame at least this often, in frames; none: only at the stream's start and when one is asked for.
    pub key_frame_every: Option<NonZeroU32>,
}

/// What a reopen made of the stream: what the old encoder still held, and what changed.
#[derive(Debug)]
pub struct Reopening {
    /// The units the encoder being replaced still held, to be sent before the new stream's.
    pub held: Vec<Encoded>,
    /// What the new stream has that the old did not; nothing when no change was asked.
    pub changes: Changes,
    /// The change was a rate the running encoder took in place: no new stream, nothing held, no key frame.
    pub in_place: bool,
}

/// `before` and `after`, when they differ.
fn changed<T: PartialEq>(before: T, after: T) -> Option<(T, T)> {
    (before != after).then_some((before, after))
}

/// What a reopen changed, each as what it was and what it became.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// The rate the encoder aims at, in bits per second.
    pub bits_per_second: Option<(u32, u32)>,
    /// Pictures a second.
    pub frames_per_second: Option<(u32, u32)>,
    /// The stream's width and height.
    pub size: Option<((usize, usize), (usize, usize))>,
    /// The encoder.
    pub encoder: Option<(Name, Name)>,
}

impl Changes {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// An encoded unit and when it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    /// In the codec's own byte stream (Annex B for H.264, OBUs for AV1), in the encoder's own buffer.
    pub data: Coded,
    /// The count of pictures the session was sent before this one, repeats included: an order, not a time.
    pub pts: i64,
    /// Whether a receiver can start decoding at this unit.
    pub key_frame: bool,
    /// When its picture was captured: the screen's own time for a screen's picture, the send for one from memory or a
    /// still screen's repeat, which is a new picture to the stream.
    pub captured: Instant,
}

/// Why a session could not be made or a frame not encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum EncodeError {
    /// The settings are zero, or too large for the encoder's types.
    Settings,
    /// The device did not open.
    Device,
    /// The device would not make frames of this size.
    Frames,
    /// The driver refused the encoder at these settings.
    Open,
    /// A frame is not the size the session was opened for.
    FrameSize,
    /// The frame could not be uploaded to the device.
    Upload,
    /// A picture could not be brought to memory in the software encoder's format.
    Planar,
    /// This device cannot be fed the screen's pictures on the GPU: it lacks what the convert needs.
    Screen,
    /// A captured picture could not be read or converted on the video device.
    Convert,
    /// The session takes pictures the other way: from memory, or from the screen, or in the colours it is tagged with.
    Input,
    /// An encoder of another codec: that is a new stream with new parameters, not a change to this one.
    OtherCodec,
    /// The device was lost under the encoder: the driver reset or removed it. A new device of the same kind may work
    /// where this one no longer does.
    DeviceLost,
    /// The encoder took more pictures than any encoder holds and handed out no unit for them.
    Stalled,
    /// The encoder failed, as FFmpeg says.
    Encoder(#[public] AvError),
}

impl EncodeError {
    /// Whether the encoder or its device failed, which another encoder of the codec may not: a picture or settings
    /// that are wrong are wrong for every encoder.
    pub fn is_the_encoders(self) -> bool {
        match self {
            Self::Device
            | Self::Frames
            | Self::Open
            | Self::Upload
            | Self::Planar
            | Self::Screen
            | Self::Convert
            | Self::DeviceLost
            | Self::Stalled
            | Self::Encoder(_) => true,
            Self::Settings | Self::FrameSize | Self::Input | Self::OtherCodec => false,
        }
    }

    /// Whether the device under the encoder was lost, so a new one of the same kind is worth opening before another
    /// kind of encoder is.
    pub fn is_device_lost(self) -> bool {
        self == Self::DeviceLost
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings => f.write_str("the encode settings are not usable"),
            Self::Device => f.write_str("the video device did not open"),
            Self::Frames => f.write_str("the video device would not make frames of this size"),
            Self::Open => f.write_str("the video encoder refused these settings"),
            Self::FrameSize => f.write_str("the frame is not the size this encode was opened for"),
            Self::Upload => f.write_str("the frame could not be sent to the video device"),
            Self::Planar => f.write_str("the picture could not be prepared for the software encoder"),
            Self::Screen => f.write_str("this video device cannot take the screen"),
            Self::Convert => f.write_str("the captured picture could not be converted on the video device"),
            Self::Input => f.write_str("this encode takes its pictures the other way"),
            Self::OtherCodec => f.write_str("a running video stream cannot change its codec"),
            Self::DeviceLost => f.write_str("the video device was lost"),
            Self::Stalled => f.write_str("the video encoder stopped handing out pictures"),
            Self::Encoder(error) => write!(f, "the video encoder failed ({error})"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Opens the device `device` names.
fn open_device(device: &Device) -> Result<HwDevice, EncodeError> {
    crate::probe::device_on(device.backend, device.node.as_deref(), device.validation).ok_or(EncodeError::Device)
}

#[cfg(any(target_os = "linux", windows))]
type Screen = screen::Path;
#[cfg(any(target_os = "linux", windows))]
pub use screen::Convert;
/// A system with no capture has no screen to take pictures from.
#[cfg(not(any(target_os = "linux", windows)))]
type Screen = std::convert::Infallible;

/// What of a picture laid out as `source` is read into a stream of `settings`, from its top left: the stream itself,
/// one pixel to one, where it is the largest of the encoder's multiple inside the picture; else the picture's even
/// size, scaled to the stream.
#[cfg(any(target_os = "linux", windows))]
fn read_of(encoder: &Encoder, settings: Settings, source: Layout) -> (u32, u32) {
    let stream = (u32::try_from(settings.width).ok(), u32::try_from(settings.height).ok());
    match stream {
        (Some(width), Some(height)) if (width, height) == encoder.multiple.inside(source.width, source.height) => {
            (width, height)
        }
        _ => (source.width & !1, source.height & !1),
    }
}

/// The screen path `encoder` reads, on `on`'s devices, and the frames it fills: the first convert that leaves what it
/// reads and opens here; `Screen` where none does.
#[cfg(any(target_os = "linux", windows))]
fn screen_frames(
    encoder: &Encoder,
    on: &screen::On<'_>,
    settings: Settings,
    source: Layout,
) -> Result<(Screen, HwFrames), EncodeError> {
    let read = read_of(encoder, settings, source);
    let scaled = (read.0 as usize, read.1 as usize) != (settings.width, settings.height);
    for convert in screen::Convert::for_screen(encoder, scaled) {
        match screen::Path::open(convert, on, settings, source, read) {
            Ok(path) => {
                let pool = path.frames()?;
                return Ok((path, pool));
            }
            // This machine lacks what that convert needs: the next one is tried.
            Err(EncodeError::Screen) => {}
            Err(failed) => return Err(failed),
        }
    }
    Err(EncodeError::Screen)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn screen_frames(_: &Encoder, _: &HwDevice, _: Settings, _: Layout) -> Result<(Screen, HwFrames), EncodeError> {
    Err(EncodeError::Device)
}

/// What a reopen on the same encoder device keeps open of a feed: the encoder's device, and the Vulkan device a
/// screen's area pass runs on beside a VA-API one.
struct Kept {
    device: HwDevice,
    #[cfg(target_os = "linux")]
    pass: Option<HwDevice>,
}

/// The most frames of a software encoder's pictures kept from one to the next: as many as the most any encoder here
/// holds in flight (AMF's default depth, 16), so one of them has always been let go. The kept grow only as far as the
/// encoder holds, since a frame is taken only when nothing else has it.
const SPARES: usize = 16;

/// Frames of a software encoder's pictures kept from one picture to the next, so one the encoder has let go of is
/// written again and not allocated anew.
#[derive(Default)]
struct Spares(Vec<Frame>);

impl Spares {
    /// A kept frame of `format` and `size` that nothing else holds.
    fn take(&mut self, format: PixelFormat, size: (u32, u32)) -> Option<Frame> {
        let at = self
            .0
            .iter()
            .position(|frame| frame.is_writable() && frame.format() == Some(format) && frame.size() == size)?;
        Some(self.0.swap_remove(at))
    }

    /// Keeps `frame` for a later picture, unless enough are kept.
    fn keep(&mut self, frame: Frame) {
        if self.0.len() < SPARES {
            self.0.push(frame);
        }
    }
}

/// What an upload keeps from one picture to the next: an empty frame that points at the caller's planes for the
/// transfer alone, and one empty frame the device's picture is drawn into.
struct Upload {
    picture: Option<Frame>,
    staging: Frame,
}

impl Upload {
    /// `from` uploaded to a frame of `pool`.
    fn picture(&mut self, pool: &HwFrames, from: &View<'_>) -> Result<Frame, EncodeError> {
        let mut hardware = match self.picture.take() {
            Some(kept) => kept,
            None => Frame::new().ok_or(EncodeError::Upload)?,
        };
        pool.upload_nv12(&mut self.staging, from, &mut hardware).map_err(|_| EncodeError::Upload)?;
        Ok(hardware)
    }

    /// Takes back the frame an encoder was given, letting go of its surface at once, for the next picture.
    fn keep(&mut self, mut picture: Frame) {
        picture.unref();
        self.picture = Some(picture);
    }
}

/// An empty frame that is made to read a picture already in the encoder's format and size where it is, for one send,
/// so a software encoder is given it with no copy; emptied once sent and kept for the next.
#[derive(Default)]
struct Reader(Option<Frame>);

impl Reader {
    fn empty(&mut self) -> Result<Frame, EncodeError> {
        self.0.take().map_or_else(|| Frame::new().ok_or(EncodeError::Planar), Ok)
    }

    /// A frame that reads `nv12`'s own bytes.
    fn read(&mut self, nv12: &Nv12) -> Result<Frame, EncodeError> {
        let mut frame = self.empty()?;
        let size = (u32::try_from(nv12.width), u32::try_from(nv12.height));
        let (Ok(width), Ok(height)) = size else { return Err(EncodeError::FrameSize) };
        frame.read_nv12(Arc::clone(&nv12.data), (width, height)).map_err(|_| EncodeError::Planar)?;
        Ok(frame)
    }

    /// Another reference to `picture`'s buffers.
    #[cfg(any(target_os = "linux", windows))]
    fn refer(&mut self, picture: &Frame) -> Result<Frame, EncodeError> {
        let mut frame = self.empty()?;
        frame.refer_to(picture).map_err(|_| EncodeError::Planar)?;
        Ok(frame)
    }

    /// Takes back a frame it made, letting go of what it read.
    fn keep(&mut self, mut frame: Frame) {
        frame.unref();
        self.0 = Some(frame);
    }
}

/// A picture as its encoder is given it.
enum Given {
    /// Made in a frame of the feed's own, kept for a later picture once sent.
    Made(Frame),
    /// The picture as it already was, read where it is by a frame of the feed's [`Reader`], which lets it go once
    /// sent.
    Read(Frame),
}

impl Given {
    fn frame(&mut self) -> &mut Frame {
        match self {
            Self::Made(frame) | Self::Read(frame) => frame,
        }
    }
}

/// Whether a software encoder of `format` at `size` takes `picture`, a picture in memory, as it is; one of another
/// format or size is converted.
#[cfg(any(target_os = "linux", windows))]
fn takes_as_it_is(picture: &Frame, format: PixelFormat, size: (u32, u32)) -> bool {
    picture.format() == Some(format) && picture.size() == size
}

/// Whether a software encoder of `format` at `size` takes `nv12` as it is: its own size, in its own format.
fn takes_as_it_is_nv12(nv12: &Nv12, format: PixelFormat, size: (u32, u32)) -> bool {
    let own = (u32::try_from(nv12.width), u32::try_from(nv12.height));
    format == PixelFormat::Nv12 && matches!(own, (Ok(width), Ok(height)) if (width, height) == size)
}

/// What a session's pictures pass through before its encoder, by where they come from and where they are coded.
/// Each variant's fields are declared in the order they are freed: what draws from a pool, the pool, its device.
enum Feed {
    /// Memory's pictures, uploaded to the encoder's own device.
    Upload { upload: Upload, pool: HwFrames, device: HwDevice },
    /// The screen's pictures, converted on the encoder's own device.
    Screen { path: Screen, pool: HwFrames, device: HwDevice },
    /// Memory's pictures, in the software encoder's format: read as they are where they are in it already.
    Planar { spares: Spares, reader: Reader, planes: Planes, format: PixelFormat },
    /// The screen's pictures, converted on a device, then brought to memory for a software encoder, whose download is
    /// read as it is where the encoder takes its format.
    ScreenPlanar { spares: Spares, reader: Reader, planes: Planes, format: PixelFormat, path: Screen, device: HwDevice },
    /// A camera's pictures, lifted onto the encoder's own device and converted there before they come.
    #[cfg(target_os = "linux")]
    Lifted { pool: HwFrames, device: HwDevice },
}

impl Feed {
    /// The feed `encoder` takes pictures through, from memory or, with `source`, from the screen; on the devices of
    /// `kept` when it is the device the encoder names, already open.
    fn open(
        encoder: &Encoder,
        settings: Settings,
        source: Option<Layout>,
        kept: Option<Kept>,
    ) -> Result<Self, EncodeError> {
        #[cfg(target_os = "linux")]
        let (kept, pass) = kept.map_or((None, None), |Kept { device, pass }| (Some(device), pass));
        #[cfg(not(target_os = "linux"))]
        let kept = kept.map(|Kept { device }| device);
        let device = |named: &Device| kept.map_or_else(|| open_device(named), Ok);
        let software = || encoder.software_format().ok_or(EncodeError::Input);
        match (&encoder.runs, source) {
            (Runs::Gpu(named), None) => {
                let device = device(named)?;
                let upload = Upload { picture: None, staging: Frame::new().ok_or(EncodeError::Upload)? };
                Ok(Self::Upload { upload, pool: pool_for(&device, named.backend, settings)?, device })
            }
            (Runs::Gpu(named) | Runs::Cpu { convert: Some(named) }, Some(source)) => {
                let device = device(named)?;
                #[cfg(any(target_os = "linux", windows))]
                let on = &screen::On {
                    device: &device,
                    #[cfg(target_os = "linux")]
                    named,
                    #[cfg(target_os = "linux")]
                    pass: pass.as_ref(),
                };
                #[cfg(not(any(target_os = "linux", windows)))]
                let on = &device;
                let (path, pool) = screen_frames(encoder, on, settings, source)?;
                Ok(match encoder.runs {
                    Runs::Gpu(_) => Self::Screen { path, pool, device },
                    // The path keeps the frames it converts into; the encoder draws from none.
                    Runs::Cpu { .. } => Self::ScreenPlanar {
                        spares: Spares::default(),
                        reader: Reader::default(),
                        planes: Planes::default(),
                        format: software()?,
                        path,
                        device,
                    },
                })
            }
            (Runs::Cpu { .. }, None) => {
                let (spares, reader, planes) = (Spares::default(), Reader::default(), Planes::default());
                Ok(Self::Planar { spares, reader, planes, format: software()? })
            }
            (Runs::Cpu { convert: None }, Some(_)) => Err(EncodeError::Screen),
        }
    }

    /// What a reopen on the same encoder device keeps open: its device and, where a screen's area pass runs beside
    /// it, the pass's.
    fn kept(&self) -> Option<Kept> {
        Some(Kept {
            device: self.device()?.again()?,
            #[cfg(target_os = "linux")]
            pass: self.pass_device().and_then(HwDevice::again),
        })
    }

    /// The device it opened; none for memory's pictures to a software encoder.
    fn device(&self) -> Option<&HwDevice> {
        match self {
            Self::Upload { device, .. } | Self::Screen { device, .. } | Self::ScreenPlanar { device, .. } => {
                Some(device)
            }
            #[cfg(target_os = "linux")]
            Self::Lifted { device, .. } => Some(device),
            Self::Planar { .. } => None,
        }
    }

    /// The frames a hardware encoder draws from; none for a software encoder, which takes pictures in memory.
    fn encoder_frames(&self) -> Option<&HwFrames> {
        match self {
            Self::Upload { pool, .. } | Self::Screen { pool, .. } => Some(pool),
            #[cfg(target_os = "linux")]
            Self::Lifted { pool, .. } => Some(pool),
            Self::Planar { .. } | Self::ScreenPlanar { .. } => None,
        }
    }

    /// Takes back `given`, which the encoder was given: a frame made is kept so the next is made in it where the
    /// encoder has let it go, and a frame that read a picture lets it go.
    fn keep(&mut self, given: Given) {
        match (self, given) {
            (Self::Planar { reader, .. } | Self::ScreenPlanar { reader, .. }, Given::Read(frame)) => reader.keep(frame),
            (Self::Upload { upload, .. }, Given::Made(picture)) => upload.keep(picture),
            (Self::Planar { spares, .. } | Self::ScreenPlanar { spares, .. }, Given::Made(picture)) => {
                spares.keep(picture);
            }
            (Self::Upload { .. } | Self::Screen { .. }, Given::Made(_) | Given::Read(_)) => {}
            #[cfg(target_os = "linux")]
            (Self::Lifted { .. }, Given::Made(_) | Given::Read(_)) => {}
        }
    }
}

/// `from`, a picture in memory of `size`, as a frame in the software encoder's `format`: a kept frame the encoder has
/// let go of, else a new one. A format already the encoder's is copied as it is.
fn planar(
    scaler: &mut Scaler,
    spares: &mut Spares,
    format: PixelFormat,
    from: &View<'_>,
    size: (u32, u32),
) -> Result<Frame, EncodeError> {
    let mut picture = match spares.take(format, size) {
        Some(kept) => kept,
        None => Frame::software(format, size).map_err(|_| EncodeError::Planar)?,
    };
    let mut to = ViewMut::of_frame(&mut picture).ok_or(EncodeError::Planar)?;
    scaler.convert(from, &mut to, Conversion::Colour).map_err(|_| EncodeError::Planar)?;
    Ok(picture)
}

/// A pool of device frames of `settings`' size, for pictures uploaded from memory.
fn pool_for(device: &HwDevice, backend: Backend, settings: Settings) -> Result<HwFrames, EncodeError> {
    let side = |value: usize| u32::try_from(value).ok().filter(|v| *v > 0);
    let (Some(width), Some(height)) = (side(settings.width), side(settings.height)) else {
        return Err(EncodeError::Settings);
    };
    nv12_pool(device, backend, (width, height)).ok_or(EncodeError::Frames)
}

/// Opens a codec for `settings` that draws from `feed`. FFmpeg reads every rate setting here and nowhere after,
/// which is why a new rate is a new codec.
fn build(encoder: &Encoder, settings: Settings, colours: Colours, feed: &Feed) -> Result<CodecContext, EncodeError> {
    let input = match (&encoder.runs, feed.encoder_frames()) {
        (Runs::Gpu(device), Some(pool)) => Some((pool, device.backend.pixel_format())),
        (Runs::Cpu { .. }, None) => None,
        _ => return Err(EncodeError::Input),
    };
    let side = |value: usize| c_int::try_from(value).ok().filter(|v| *v > 0);
    let (Some(width), Some(height)) = (side(settings.width), side(settings.height)) else {
        return Err(EncodeError::Settings);
    };
    let bit_rate = i64::from(settings.bits_per_second);
    let rate = c_int::try_from(settings.frames_per_second).ok().filter(|v| *v > 0);
    let Some(rate) = rate.filter(|_| bit_rate > 0) else {
        return Err(EncodeError::Settings);
    };
    let codec = encoder.name.found().ok_or(EncodeError::Open)?;
    let software = encoder.software_format();
    tuning::open(codec, encoder.name, |target| {
        match (input, software) {
            (Some((pool, format)), _) => {
                if !target.set_hw_frames(pool) {
                    return false;
                }
                target.set_pixel_format(format);
            }
            (None, Some(format)) => target.set_pixel_format(format),
            (None, None) => return false,
        }
        target.set_size(width, height);
        target.set_frame_rate(rate);
        // The session asks for each key frame itself: AMF at ultra-low latency keeps no period of its own.
        target.set_gop(c_int::from(i16::MAX));
        // Frames leave in the order they arrive: a call is live, so nothing waits for a later frame.
        target.set_max_b_frames(0);
        // What the pictures are, said in the stream so a receiver does not guess.
        target.set_tags(colours.matrix.tag(), colours.range.tag());
        tuning::hold_rate(target, encoder.name.family(), bit_rate, rate, opening(encoder))
    })
    .ok_or(EncodeError::Open)
}

/// The buffer a new encoder opens with: its first picture is a key frame.
fn opening(encoder: &Encoder) -> tuning::Buffer {
    tuning::Buffer::for_picture(encoder.name.family(), true)
}

/// The most times a picture is sent again to bring a key frame out of the encoder.
#[cfg(any(target_os = "linux", windows))]
const REPEATS_UNTIL_OUT: usize = 4;

/// One running encode.
pub struct Session {
    /// What runs on the encoder's device; freed by [`Drop`] only while that device lives, else retired.
    parts: ManuallyDrop<Parts>,
    /// Where each unit the encoder finishes is received, kept from one unit to the next.
    packet: Packet,
    encoder: Encoder,
    /// What the encoder runs at: the size asked, rounded down to the encoder's multiple.
    settings: Settings,
    /// The stream's size as it was asked: a picture from memory is this size, and its top left is sent.
    asked: (usize, usize),
    /// What the stream is tagged as carrying, which a reopen keeps.
    colours: Colours,
    /// The size and channel order of the screen's pictures when this session takes them, none when it takes memory's.
    /// A side may be one more than the settings': the picture is read without its odd last column or row.
    source: Option<Layout>,
    next: i64,
    key_frame_wanted: bool,
    /// The rate buffer the encoder holds the next picture to.
    buffer: tuning::Buffer,
    /// The frame by which the next key frame is due, `key_frame_every` after the last one sent; none without a period.
    next_key: Option<i64>,
    /// What was asked to change, applied before the next frame.
    pending: Pending,
    /// Each picture sent whose unit has not come out.
    in_flight: InFlight,
    /// The last new picture an encoder that keeps pictures in flight has not yet handed out, by its pts.
    change: Option<i64>,
    /// What the last picture's encode took.
    timed: Timed,
}

/// What one picture's encode took, apart: what made it the encoder's, and the encoder's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timed {
    /// Making the picture the encoder's: a screen's convert submitted on its GPU and, for a software encoder, brought
    /// to memory; a picture from memory uploaded or converted.
    pub convert: Duration,
    /// The encoder's own: the picture sent and what it finished taken.
    pub encode: Duration,
}

/// What an encoder holds on its device, which goes as one: freed in this order, or kept whole once the device is lost.
struct Parts {
    // Declared first so the codec is freed before the frame pool and the device it uses.
    context: CodecContext,
    /// What takes the filler out of what `context` writes, for an encoder that pads to a constant rate.
    filler: Option<Filler>,
    // The last screen picture as the encoder was given it, drawn from the feed's pool, so it goes first.
    last: Option<Frame>,
    feed: Feed,
}

impl Drop for Session {
    fn drop(&mut self) {
        let lost = self.device_lost();
        // SAFETY: `parts` is taken once, here, and the session is not used after its drop.
        let parts = unsafe { ManuallyDrop::take(&mut self.parts) };
        if lost {
            retired::keep(parts);
        } else {
            drop(parts);
        }
    }
}

/// The changes a session has been asked for and has not yet made.
#[derive(Clone, Default)]
struct Pending {
    rate: Option<Rate>,
    size: Option<(usize, usize)>,
    /// A screen's new picture, of which `size` is the stream.
    source: Option<Layout>,
    encoder: Option<Encoder>,
    /// The encoder asked is opened on a new device, not the one the running encoder has.
    renew: bool,
}

/// `settings` at the largest size of `encoder`'s multiple inside the size they ask; refused where none is.
fn coded(encoder: &Encoder, settings: Settings) -> Result<Settings, EncodeError> {
    let side = |value: usize| u32::try_from(value).map_err(|_| EncodeError::Settings);
    let (width, height) = encoder.multiple.inside(side(settings.width)?, side(settings.height)?);
    if width == 0 || height == 0 {
        return Err(EncodeError::Settings);
    }
    Ok(Settings { width: width as usize, height: height as usize, ..settings })
}

/// Whether a stream of `width` × `height` is an even size inside the largest even size of `source`.
fn fits_inside(width: usize, height: usize, source: Layout) -> bool {
    let inside = |coded: usize, whole: u32| usize::try_from(whole).is_ok_and(|whole| coded > 0 && coded <= whole & !1);
    width.is_multiple_of(2) && height.is_multiple_of(2) && inside(width, source.width) && inside(height, source.height)
}

/// A rate target: what the engine's congestion controller asks of the encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    /// A budget the encoder aims at, not a cap.
    pub bits_per_second: u32,
    /// Pictures a second, a whole number.
    pub frames_per_second: u32,
}

impl Rate {
    /// A target of `bits_per_second` at `frames_per_second`; zero bits and anything past the encoder's types are
    /// refused, so a `Rate` made here is one an encoder can be opened at.
    pub fn new(bits_per_second: u64, frames_per_second: NonZeroU32) -> Result<Self, EncodeError> {
        let bits_per_second = u32::try_from(bits_per_second).ok().filter(|bits| *bits > 0);
        let frames = frames_per_second.get();
        match bits_per_second {
            Some(bits_per_second) if i32::try_from(frames).is_ok() => {
                Ok(Self { bits_per_second, frames_per_second: frames })
            }
            _ => Err(EncodeError::Settings),
        }
    }
}

/// A key frame interval of `every` pictures at `before` a second, as pictures at `after`, so it lasts as long.
fn same_seconds(every: NonZeroU32, before: u32, after: u32) -> NonZeroU32 {
    if before == 0 {
        return every;
    }
    let pictures = u64::from(every.get()) * u64::from(after) / u64::from(before);
    NonZeroU32::new(u32::try_from(pictures).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN)
}

// SAFETY: a session owns every FFmpeg object it holds and shares none, and FFmpeg lets a codec context move between
// threads while one thread uses it at a time, which `&mut self` ensures.
unsafe impl Send for Session {}

impl Session {
    /// Opens `encoder` for `settings`, its stream tagged [`Colours::HD`].
    pub fn open(encoder: &Encoder, settings: Settings) -> Result<Self, EncodeError> {
        Self::open_in(encoder, settings, Colours::HD)
    }

    /// Opens `encoder` for `settings` to take pictures in `colours` from memory, its stream tagged as they are: the
    /// stream is the largest of the encoder's multiple inside the pictures, which [`Session::running`] says, their
    /// right and bottom edge left out.
    pub fn open_in(encoder: &Encoder, settings: Settings, colours: Colours) -> Result<Self, EncodeError> {
        Self::start(encoder, settings, colours, None)
    }

    /// Opens `encoder` for `settings` to take the screen's pictures, of the size and channel order of `source`,
    /// through [`Session::encode_frame`]. The GPU the encoder runs on reads, scales and converts them in one pass,
    /// into [`Colours::HD`]: `settings` is any even size inside the largest even size of `source`, whose odd last
    /// column or row is left out, and the stream the largest of the encoder's multiple inside it, which
    /// [`Session::running`] says; at the picture's own size its right and bottom edge are left out, not scaled.
    pub fn open_screen(encoder: &Encoder, settings: Settings, source: Layout) -> Result<Self, EncodeError> {
        if !fits_inside(settings.width, settings.height, source) {
            return Err(EncodeError::Settings);
        }
        Self::start(encoder, settings, Colours::HD, Some(source))
    }

    fn start(
        encoder: &Encoder,
        settings: Settings,
        colours: Colours,
        source: Option<Layout>,
    ) -> Result<Self, EncodeError> {
        let asked = (settings.width, settings.height);
        let settings = coded(encoder, settings)?;
        let feed = Feed::open(encoder, settings, source, None)?;
        Self::begin(encoder, settings, asked, colours, source, feed)
    }

    fn begin(
        encoder: &Encoder,
        settings: Settings,
        asked: (usize, usize),
        colours: Colours,
        source: Option<Layout>,
        feed: Feed,
    ) -> Result<Self, EncodeError> {
        let context = build(encoder, settings, colours, &feed)?;
        let filler = encoder.name.family().pads().then(|| Filler::new(encoder.codec(), &context)).transpose()?;
        Ok(Self {
            parts: ManuallyDrop::new(Parts { context, filler, last: None, feed }),
            packet: Packet::new().ok_or(EncodeError::Encoder(AvError::NoMemory))?,
            encoder: encoder.clone(),
            settings,
            asked,
            colours,
            source,
            next: 0,
            key_frame_wanted: false,
            buffer: opening(encoder),
            next_key: settings.key_frame_every.map(|every| i64::from(every.get())),
            pending: Pending::default(),
            in_flight: InFlight::new(),
            change: None,
            timed: Timed::default(),
        })
    }

    /// Asks for a new rate, made by the next encode: an encoder that [`Session::takes_rate_in_place`] takes it in its
    /// running stream; for every other FFmpeg reads a rate only when it opens, so the old stream is finished and a new
    /// encoder opens on the same device, starting on a key frame. The running rate changes nothing.
    pub fn set_rate(&mut self, rate: Rate) -> Result<(), EncodeError> {
        if rate.bits_per_second == 0 || rate.frames_per_second == 0 {
            return Err(EncodeError::Settings);
        }
        self.pending.rate = Some(rate);
        Ok(())
    }

    /// Asks for pictures of another size, as a source that changed size needs: a screen's stream becomes the new
    /// picture's even size, unscaled. Made like a rate change, and the next frame must be this size.
    pub fn set_size(&mut self, width: usize, height: usize) -> Result<(), EncodeError> {
        if width == 0 || height == 0 {
            return Err(EncodeError::Settings);
        }
        self.pending.size = Some((width, height));
        Ok(())
    }

    /// Asks a screen's session for pictures of `source`, a new size or channel order, sent as a stream of `stream`, an
    /// even size inside it as [`Session::open_screen`] takes. Made like a rate change, on a key frame, and the next
    /// frame must be `source`'s.
    pub fn set_source(&mut self, source: Layout, stream: (usize, usize)) -> Result<(), EncodeError> {
        if self.source.is_none() {
            return Err(EncodeError::Input);
        }
        if !fits_inside(stream.0, stream.1, source) {
            return Err(EncodeError::Settings);
        }
        self.pending.source = Some(source);
        self.pending.size = Some(stream);
        Ok(())
    }

    /// Asks for another encoder of the same codec (VA-API to Vulkan, or another device), made like a rate change; the
    /// running one asked again is opened anew. A different codec is a different stream and is refused.
    pub fn set_encoder(&mut self, encoder: &Encoder) -> Result<(), EncodeError> {
        if encoder.codec() != self.encoder.codec() {
            return Err(EncodeError::OtherCodec);
        }
        self.pending.encoder = Some(encoder.clone());
        self.pending.renew = false;
        Ok(())
    }

    /// Asks for the running encoder opened anew on a new device of its kind, made like a rate change: for a device
    /// that was lost, which the same device cannot be reopened on.
    pub fn rebuild_device(&mut self) {
        self.pending.encoder = Some(self.encoder.clone());
        self.pending.renew = true;
    }

    /// Drops the encoder asked, and no other change: the stream goes on with the one it has.
    pub fn withdraw_encoder(&mut self) {
        self.pending.encoder = None;
        self.pending.renew = false;
    }

    /// Whether the next reopen makes a new stream: a new size, source or encoder was asked, or a rate that the running
    /// encoder cannot take in place.
    pub fn will_reopen(&self) -> bool {
        let pending = &self.pending;
        pending.size.is_some()
            || pending.source.is_some()
            || pending.encoder.is_some()
            || (pending.rate.is_some() && !self.takes_rate_in_place())
    }

    /// Drops every change asked and not yet made: the running stream goes on as it is.
    pub fn withdraw(&mut self) {
        self.pending = Pending::default();
    }

    /// The size and encoder the running stream was opened with: the size asked, rounded down to the encoder's
    /// multiple.
    pub fn running(&self) -> (usize, usize, &Encoder) {
        (self.settings.width, self.settings.height, &self.encoder)
    }

    /// What the stream is tagged as carrying, which every encoder it is reopened on keeps.
    pub fn colours(&self) -> Colours {
        self.colours
    }

    /// The convert the screen's pictures pass through; none for pictures from memory.
    #[cfg(any(target_os = "linux", windows))]
    pub fn convert(&self) -> Option<Convert> {
        match &self.parts.feed {
            Feed::Screen { path, .. } | Feed::ScreenPlanar { path, .. } => Some(path.kind()),
            Feed::Upload { .. } | Feed::Planar { .. } => None,
            #[cfg(target_os = "linux")]
            Feed::Lifted { .. } => None,
        }
    }

    /// Whether a new rate alone is taken by the running encoder, with no new stream and no key frame.
    pub fn takes_rate_in_place(&self) -> bool {
        self.encoder.name.family().takes_rate_in_place()
    }

    /// The rate the running encoder aims at: the one it opened with, or the last it took in place.
    pub fn rate(&self) -> Rate {
        Rate { bits_per_second: self.settings.bits_per_second, frames_per_second: self.settings.frames_per_second }
    }

    /// The next frame is encoded as a key frame, for a receiver that lost the picture.
    pub fn request_key_frame(&mut self) {
        self.key_frame_wanted = true;
    }

    /// Whether the frame about to be sent is a key frame: one was asked for, or `key_frame_every` frames have passed
    /// since the last. Kept here for every encoder rather than left to a driver's own period.
    fn key_frame_now(&mut self) -> bool {
        let due = self.next_key.is_some_and(|at| self.next >= at);
        let key = std::mem::take(&mut self.key_frame_wanted) || due;
        if key {
            self.next_key = self.settings.key_frame_every.map(|every| self.next + i64::from(every.get()));
        }
        key
    }

    /// Gives a key frame about to be sent its budget, which it keeps until it is out: VA-API and Vulkan read the
    /// buffer when they start a picture, after later sends.
    fn budget_key_frame(&mut self, key_frame: bool) -> Result<(), EncodeError> {
        if key_frame { self.hold_buffer(tuning::Buffer::for_picture(self.encoder.name.family(), true)) } else { Ok(()) }
    }

    /// Holds the running encoder to `buffer` from the next picture it starts, if it holds another now.
    fn hold_buffer(&mut self, buffer: tuning::Buffer) -> Result<(), EncodeError> {
        let family = self.encoder.name.family();
        if buffer == self.buffer {
            return Ok(());
        }
        let fps = c_int::try_from(self.settings.frames_per_second).map_err(|_| EncodeError::Settings)?;
        let bits = self.settings.bits_per_second.into();
        if !tuning::set_rate_in_place(&mut self.parts.context, family, bits, fps, buffer) {
            return Err(EncodeError::Settings);
        }
        self.buffer = buffer;
        Ok(())
    }

    /// Stamps `picture` with its place in the stream, gives a key frame its budget and sends it to the encoder, which
    /// takes a reference to it.
    fn send(&mut self, picture: &mut Frame, key_frame: bool) -> Result<(), EncodeError> {
        picture.stamp(self.next, key_frame);
        self.budget_key_frame(key_frame)?;
        self.parts.context.send_frame(Some(&*picture)).map_err(|error| self.failed(error))
    }

    /// Encodes one frame and returns whatever the encoder has finished. A software encoder that reads NV12 at the
    /// frame's own size is given the frame's own bytes, which it may hold past this call.
    pub fn encode(&mut self, frame: &Nv12) -> Result<Vec<Encoded>, EncodeError> {
        if self.source.is_some() {
            return Err(EncodeError::Input);
        }
        if frame.colours != self.colours {
            return Err(EncodeError::Input);
        }
        if (frame.width, frame.height) != self.pending.size.unwrap_or(self.asked) {
            return Err(EncodeError::FrameSize);
        }
        let mut out = self.reopen()?.held;
        self.timed = Timed::default();
        let started = Instant::now();
        let key_frame = self.key_frame_now();
        let to_c = |value: usize| u32::try_from(value).map_err(|_| EncodeError::FrameSize);
        let size = (to_c(self.settings.width)?, to_c(self.settings.height)?);
        let whole = View::nv12((to_c(frame.width)?, to_c(frame.height)?), frame.luma(), frame.chroma());
        let from = whole.and_then(|view| view.top_left(size)).ok_or(EncodeError::FrameSize)?;
        let mut given = match &mut self.parts.feed {
            Feed::Upload { upload, pool, .. } => Given::Made(upload.picture(pool, &from)?),
            Feed::Planar { reader, format, .. } if takes_as_it_is_nv12(frame, *format, size) => {
                Given::Read(reader.read(frame)?)
            }
            Feed::Planar { spares, planes, format, .. } => {
                Given::Made(planar(&mut planes.scaler, spares, *format, &from, size)?)
            }
            Feed::Screen { .. } | Feed::ScreenPlanar { .. } => return Err(EncodeError::Input),
            #[cfg(target_os = "linux")]
            Feed::Lifted { .. } => return Err(EncodeError::Input),
        };
        let converted = Instant::now();
        self.send(given.frame(), key_frame)?;
        self.change = Some(self.next);
        self.sent(Instant::now())?;
        self.parts.feed.keep(given);
        out.extend(self.drain()?);
        self.timed = Timed { convert: converted - started, encode: converted.elapsed() };
        Ok(out)
    }

    /// Encodes one picture of the screen and returns whatever the encoder has finished; the buffer goes back to the
    /// compositor once the GPU has read it, which a later picture or [`Session::settle`] waits for, as the encoder
    /// waits for the convert on the GPU. Only a session opened with [`Session::open_screen`] takes these.
    #[cfg(any(target_os = "linux", windows))]
    pub fn encode_frame(&mut self, frame: capture::Frame) -> Result<Vec<Encoded>, EncodeError> {
        let source = self.source.ok_or(EncodeError::Input)?;
        let layout = frame.layout();
        let expected = match self.pending.source {
            Some(next) => (next.width as usize, next.height as usize),
            None => self.pending.size.unwrap_or((source.width as usize, source.height as usize)),
        };
        if (layout.width as usize, layout.height as usize) != expected {
            return Err(EncodeError::FrameSize);
        }
        let mut out = self.reopen()?.held;
        self.timed = Timed::default();
        let started = Instant::now();
        let key_frame = self.key_frame_now();
        let captured = frame.captured();
        let to_c = |value: usize| u32::try_from(value).map_err(|_| EncodeError::Settings);
        let size = (to_c(self.settings.width)?, to_c(self.settings.height)?);
        let mut given = match &mut self.parts.feed {
            Feed::Screen { path, .. } => Given::Made(path.convert(frame, self.next)?),
            Feed::ScreenPlanar { spares, reader, planes, format, path, .. } => {
                let converted = path.convert(frame, self.next)?;
                let in_memory = planes.download.in_memory(&converted).ok_or(EncodeError::Planar)?;
                if takes_as_it_is(in_memory, *format, size) {
                    Given::Read(reader.refer(in_memory)?)
                } else {
                    let from = View::of_frame(in_memory).ok_or(EncodeError::Planar)?;
                    // `from` points into the download `planes` keeps, which converting leaves alone.
                    Given::Made(planar(&mut planes.scaler, spares, *format, &from, in_memory.size())?)
                }
            }
            Feed::Upload { .. } | Feed::Planar { .. } => return Err(EncodeError::Input),
            #[cfg(target_os = "linux")]
            Feed::Lifted { .. } => return Err(EncodeError::Input),
        };
        let converted = Instant::now();
        self.send(given.frame(), key_frame)?;
        self.change = Some(self.next);
        self.sent(captured)?;
        self.parts.last = given.frame().reference();
        self.parts.feed.keep(given);
        out.extend(self.drain()?);
        self.timed = Timed { convert: converted - started, encode: converted.elapsed() };
        Ok(out)
    }

    /// What the last picture's encode took: the convert apart from the encoder's own time, so an encoder is judged by
    /// what it took alone. Zero for a picture whose encode failed.
    pub fn timed(&self) -> Timed {
        self.timed
    }

    /// Waits for every screen picture still converting on the GPU, so each captured buffer is its owner's again: a
    /// share that rests calls it, as a caller that draws into a buffer it lent does before it draws again.
    #[cfg(any(target_os = "linux", windows))]
    pub fn settle(&mut self) -> Result<(), EncodeError> {
        match &mut self.parts.feed {
            Feed::Screen { path, .. } | Feed::ScreenPlanar { path, .. } => path.settle(),
            Feed::Upload { .. } | Feed::Planar { .. } => Ok(()),
            #[cfg(target_os = "linux")]
            Feed::Lifted { .. } => Ok(()),
        }
    }

    /// Pictures handed to the encoder since the session opened, repeats included.
    pub fn sends(&self) -> u64 {
        u64::try_from(self.next).unwrap_or(0)
    }

    /// The units the encoder has finished since the last call, sending it nothing: a stream nobody takes collects
    /// them, since an encoder lets go of a picture it was given only once its unit is taken.
    pub fn finished(&mut self) -> Result<Vec<Encoded>, EncodeError> {
        self.drain()
    }

    /// Lets go of the last picture, kept to be sent again: a newer one the caller holds stands for the screen now, so
    /// a capture of few buffers is not left with none to fill.
    pub fn superseded(&mut self) {
        self.parts.last = None;
    }

    /// Whether the encoder still holds a new picture it has not handed out: VA-API and Vulkan keep one in flight, so
    /// a screen that goes still shows its last change only once something is sent after it.
    pub fn holds_a_change(&self) -> bool {
        self.change.is_some()
    }

    /// Encodes the screen's last picture again, as a key frame when one was asked for, for a receiver that lost the
    /// picture of a screen that sends nothing while still. A key frame comes out only after a later send, so the
    /// picture is sent again until it does; nothing is encoded before the first picture.
    #[cfg(any(target_os = "linux", windows))]
    pub fn repeat_last(&mut self) -> Result<Vec<Encoded>, EncodeError> {
        let mut out = self.reopen()?.held;
        let Some(mut again) = self.parts.last.as_ref().and_then(Frame::reference) else {
            return Ok(out);
        };
        let key_frame = self.key_frame_now();
        // The encoder holds at most a picture or two: more sends than that and the key frame is out.
        for sent in 0..REPEATS_UNTIL_OUT {
            let key = key_frame && sent == 0;
            self.send(&mut again, key)?;
            self.sent(Instant::now())?;
            out.extend(self.drain()?);
            if !key_frame || out.iter().any(|unit| unit.key_frame) {
                break;
            }
        }
        // What came before the key frame belongs to a picture the receiver is waiting to replace: it is not sent.
        if let Some(first) = out.iter().position(|unit| unit.key_frame) {
            out.drain(..first);
        }
        Ok(out)
    }

    /// Makes what was asked (a rate, a size, a source, an encoder) now rather than at the next encode: finishes the
    /// running stream and opens its successor, returning what the old encoder still held and what changed, so a caller
    /// can time the encode apart from the reopen. Nothing when nothing was asked; the next frame is a key frame
    /// otherwise. A refusal leaves the running stream as it was and the change still asked, for another encoder to take
    /// ([`Session::set_encoder`]) or to be withdrawn ([`Session::withdraw`]).
    pub fn reopen(&mut self) -> Result<Reopening, EncodeError> {
        let family = self.encoder.name.family();
        let due = self.key_frame_wanted || self.next_key.is_some_and(|at| self.next >= at);
        if due && self.next > 0 && family.takes_rate_in_place() && !family.budgets_in_place() {
            self.pending.encoder.get_or_insert_with(|| self.encoder.clone());
        }
        let pending = self.pending.clone();
        let rate = pending.rate.unwrap_or_else(|| self.rate());
        let (mut width, mut height) = pending.size.unwrap_or(self.asked);
        let mut source = self.source;
        match (pending.source, source.as_mut(), pending.size) {
            // A screen's new picture, with the stream asked for it.
            (Some(next), _, _) => source = Some(next),
            // A screen's new size alone is the picture's; the stream is the even size inside it.
            (None, Some(layout), Some((whole_width, whole_height))) => {
                layout.width = u32::try_from(whole_width).map_err(|_| EncodeError::Settings)?;
                layout.height = u32::try_from(whole_height).map_err(|_| EncodeError::Settings)?;
                (width, height) = (width & !1, height & !1);
            }
            (None, _, _) => {}
        }
        let fresh = pending.encoder.is_some();
        let renew = pending.renew;
        let encoder = pending.encoder.unwrap_or_else(|| self.encoder.clone());
        let asked = (width, height);
        let settings = coded(
            &encoder,
            Settings {
                width,
                height,
                bits_per_second: rate.bits_per_second,
                frames_per_second: rate.frames_per_second,
                key_frame_every: self
                    .settings
                    .key_frame_every
                    .map(|every| same_seconds(every, self.settings.frames_per_second, rate.frames_per_second)),
            },
        )?;
        let anew = encoder == self.encoder && settings == self.settings && source == self.source;
        if settings == self.settings && !fresh && source == self.source {
            self.pending = Pending::default();
            return Ok(Reopening { held: Vec::new(), changes: Changes::default(), in_place: false });
        }
        let rate_alone = Settings { bits_per_second: self.settings.bits_per_second, ..settings } == self.settings
            && !fresh
            && source == self.source;
        if rate_alone && encoder.name.family().takes_rate_in_place() {
            let fps = c_int::try_from(settings.frames_per_second).map_err(|_| EncodeError::Settings)?;
            let family = encoder.name.family();
            let (bits, buffer) = (settings.bits_per_second.into(), self.buffer);
            if !tuning::set_rate_in_place(&mut self.parts.context, family, bits, fps, buffer) {
                return Err(EncodeError::Settings);
            }
            let changes = Changes {
                bits_per_second: changed(self.settings.bits_per_second, settings.bits_per_second),
                ..Changes::default()
            };
            self.pending = Pending::default();
            self.settings = settings;
            return Ok(Reopening { held: Vec::new(), changes, in_place: true });
        }
        // Build first: if the driver refuses, the running encoder is untouched. Another encoder on the same device
        // keeps it, so a capture drawing into it carries on.
        let same_device = !renew && encoder.device() == self.encoder.device();
        let feed = match self.parts.feed.lifted_again().filter(|_| same_device) {
            Some(lifted) => lifted,
            None => {
                let kept = same_device.then(|| self.parts.feed.kept()).flatten();
                Feed::open(&encoder, settings, source, kept)?
            }
        };
        let context = build(&encoder, settings, self.colours, &feed)?;
        let filler = encoder.name.family().pads().then(|| Filler::new(encoder.codec(), &context)).transpose()?;
        // On a lost device the old encoder is neither drained nor freed (`retired`): what it held is lost.
        let lost = self.device_lost();
        // An encoder that failed may not drain: its successor takes over all the same, and what it held is lost.
        let held = self.drain_to_end().unwrap_or_default();
        let changes = Changes {
            bits_per_second: changed(self.settings.bits_per_second, settings.bits_per_second),
            frames_per_second: changed(self.settings.frames_per_second, settings.frames_per_second),
            size: changed((self.settings.width, self.settings.height), (settings.width, settings.height)),
            encoder: changed(self.encoder.name, encoder.name),
        };
        self.pending = Pending::default();
        if lost {
            let parts = Parts { context, filler, last: None, feed };
            retired::keep(std::mem::replace(&mut *self.parts, parts));
        } else {
            // Assigned in the order the fields are declared, so the old codec is freed before its feed.
            self.parts.context = context;
            self.parts.filler = filler;
            // The same encoder opened anew keeps the last picture, which it holds a reference to, so a still screen
            // answers the key frame asked with it.
            if !anew || renew {
                self.parts.last = None;
            }
            self.parts.feed = feed;
        }
        self.encoder = encoder;
        self.settings = settings;
        self.asked = asked;
        self.source = source;
        self.buffer = opening(&self.encoder);
        self.key_frame_wanted = true;
        Ok(Reopening { held, changes, in_place: false })
    }

    fn drain_to_end(&mut self) -> Result<Vec<Encoded>, EncodeError> {
        // An encoder never given a picture holds none, and FFmpeg's Vulkan encoders crash when one is ended so; one
        // on a lost device never returns from the drain (`retired`).
        if self.next == 0 || self.device_lost() {
            return Ok(Vec::new());
        }
        self.parts.context.send_frame(None).map_err(|error| self.failed(error))?;
        self.drain()
    }

    /// Whether the device the encoder runs on was lost (a driver's reset, a GPU hang): nothing on it works again, and the
    /// share rebuilds it. An encoder that is given no picture (a still screen nobody watches) never fails to say so.
    pub fn device_lost(&self) -> bool {
        self.parts.feed.device().is_some_and(HwDevice::is_lost)
    }

    /// Ends the stream and returns what the encoder still held.
    pub fn finish(mut self) -> Result<Vec<Encoded>, EncodeError> {
        self.drain_to_end()
    }

    fn drain(&mut self) -> Result<Vec<Encoded>, EncodeError> {
        let mut out = Vec::new();
        loop {
            match self.parts.context.receive_packet(&mut self.packet) {
                Ok(()) => {}
                Err(AvError::Again | AvError::End) => return Ok(out),
                Err(error) => return Err(self.failed(error)),
            }
            if let Some(filler) = &mut self.parts.filler
                && !filler.take(&mut self.packet)?
            {
                continue;
            }
            let (pts, key_frame) = (self.packet.pts(), self.packet.is_key());
            // The encoder's buffer moves into the unit, leaving the packet blank for the next receive.
            let data = match Coded::taken(&mut self.packet) {
                Ok(data) => data,
                Err(CodedError::Size) => continue,
                Err(CodedError::Memory) => return Err(EncodeError::Encoder(AvError::NoMemory)),
            };
            let encoded = Encoded { data, pts, key_frame, captured: self.in_flight.captured_of(pts, Instant::now()) };
            if self.change.is_some_and(|change| encoded.pts >= change) {
                self.change = None;
            }
            if encoded.key_frame {
                self.hold_buffer(tuning::Buffer::Frame)?;
            }
            out.push(encoded);
        }
    }

    /// What the encoder's failure `error` is: its device's loss when the device says it is lost, which FFmpeg's
    /// status cannot tell, else the encoder's own.
    fn failed(&self, error: AvError) -> EncodeError {
        if self.device_lost() { EncodeError::DeviceLost } else { EncodeError::Encoder(error) }
    }

    /// A picture was sent, captured at `captured`: the next one is counted after it. An encoder already holding more
    /// pictures than any encoder holds, with no unit out, has stalled.
    fn sent(&mut self, captured: Instant) -> Result<(), EncodeError> {
        self.in_flight.sent(self.next, captured).map_err(|in_flight::Stalled| EncodeError::Stalled)?;
        self.next += 1;
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
impl Feed {
    /// Only Linux lifts a camera's pictures onto the encoder's device.
    fn lifted_again(&self) -> Option<Self> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Codec;

    fn software(name: Name) -> Encoder {
        Encoder { name, runs: Runs::Cpu { convert: None }, multiple: crate::Multiple::EVEN }
    }

    fn small() -> Settings {
        Settings { width: 320, height: 180, frames_per_second: 30, bits_per_second: 600_000, key_frame_every: None }
    }

    #[test]
    fn a_key_frame_interval_lasts_as_long_at_another_frame_rate() {
        let every = |pictures| NonZeroU32::new(pictures).unwrap();
        assert_eq!(same_seconds(every(600), 60, 30), every(300), "ten seconds at 60 is ten seconds at 30");
        assert_eq!(same_seconds(every(600), 60, 24), every(240));
        assert_eq!(same_seconds(every(600), 60, 60), every(600));
        assert_eq!(
            (same_seconds(every(1), 60, 1), same_seconds(every(600), 0, 30)),
            (every(1), every(600)),
            "never none, and an unknown rate is left"
        );
    }

    #[test]
    fn a_key_frame_is_held_to_its_budget_until_it_is_out_and_every_picture_after_to_one_frame() {
        let report = crate::survey();
        let x264 = report.memory_order(crate::Codec::H264).find(|encoder| encoder.name == Name::X264).expect("x264");
        let mut session = Session::open(x264, small()).expect("x264 opens");
        let buffer = |session: &Session| session.parts.context.buffer_size();
        assert_eq!(buffer(&session), 180_000, "the first key frame is held to 300 ms of the rate");
        let still = Nv12::new(320, 180, vec![128; 320 * 180 * 3 / 2]).expect("a picture");
        let out = session.encode(&still).expect("encoded");
        assert!(out.iter().any(|unit| unit.key_frame), "x264 hands its key frame out of the send");
        assert_eq!(buffer(&session), 20_000, "once it is out, every picture is held to one frame");
    }

    #[test]
    fn a_software_encoder_is_given_the_format_it_reads() {
        assert_eq!(software(Name::X264).software_format(), Some(PixelFormat::Nv12), "x264 takes NV12 as it comes");
        assert_eq!(software(Name::SvtAv1).software_format(), Some(PixelFormat::Yuv420p));
        let device = Device { backend: Backend::Vulkan, node: None, vendor: None, validation: crate::Validation::Off };
        let hardware = Encoder { name: Name::H264Vulkan, runs: Runs::Gpu(device), multiple: crate::Multiple::EVEN };
        assert_eq!(hardware.software_format(), None);
    }

    #[test]
    fn x264_is_fed_nv12_and_its_stream_decodes_to_the_picture_sent() {
        let mut session = Session::open(&software(Name::X264), small()).expect("x264 opens");
        let (width, height) = (320, 180);
        let mut data = vec![128; width * height * 3 / 2];
        data[..width * height].fill(100);
        let picture = Nv12::new(width, height, data).expect("a picture");
        let mut units = Vec::new();
        for _ in 0..10 {
            units.extend(session.encode(&picture).expect("encoded"));
        }
        units.extend(session.finish().expect("finished"));
        assert!(units.first().is_some_and(|unit| unit.key_frame), "the stream starts on a key frame");
        let mut decoder = crate::Decoder::open(Codec::H264, &crate::Sources::default()).expect("a software decoder");
        let mut shown = Vec::new();
        for unit in &units {
            shown.extend(decoder.decode(&unit.data, unit.pts).expect("decoded"));
        }
        shown.extend(decoder.finish().expect("finished"));
        assert_eq!(shown.len(), 10, "every picture comes back");
        let luma = shown[9].picture.luma();
        assert!(luma.iter().all(|sample| sample.abs_diff(100) <= 3), "the luma sent is the luma shown");
    }

    #[test]
    fn a_memory_route_to_a_software_encoder_makes_no_frame_or_buffer_once_it_is_warm() {
        for name in [Name::X264, Name::SvtAv1] {
            let mut session = Session::open(&software(name), small()).expect("the software encoder opens");
            let still = Nv12::new(320, 180, vec![128; 320 * 180 * 3 / 2]).expect("a picture");
            for _ in 0..20 {
                session.encode(&still).expect("encoded");
            }
            let before = crate::av::counted::made();
            for _ in 0..100 {
                session.encode(&still).expect("encoded");
            }
            assert_eq!(crate::av::counted::made(), before, "{name}: pictures allocated frames or buffers");
        }
    }

    #[test]
    fn x264_reads_a_picture_from_memory_where_it_is_and_lets_it_go_once_sent() {
        let mut session = Session::open(&software(Name::X264), small()).expect("x264 opens");
        let still = Nv12::new(320, 180, vec![128; 320 * 180 * 3 / 2]).expect("a picture");
        let Feed::Planar { reader, .. } = &mut session.parts.feed else { panic!("a memory route") };
        let read = reader.read(&still).expect("read where it is");
        let planes = [read.plane(0).expect("luma"), read.plane(1).expect("chroma")];
        assert_eq!(planes[0].bytes.as_ptr(), still.luma().as_ptr(), "the encoder is given the picture's own luma");
        assert_eq!(planes[1].bytes.as_ptr(), still.chroma().as_ptr(), "and its own chroma");
        reader.keep(read);
        assert_eq!(Arc::strong_count(&still.data), 1, "once sent, only the picture holds its bytes");
        for _ in 0..3 {
            session.encode(&still).expect("encoded");
        }
        assert_eq!(Arc::strong_count(&still.data), 1, "x264 copied what it keeps: it holds none of the picture");
        let scaled = Nv12::new(322, 180, vec![128; 322 * 180 * 3 / 2]).expect("a larger picture");
        assert!(!takes_as_it_is_nv12(&scaled, PixelFormat::Nv12, (320, 180)), "another size is converted");
        assert!(!takes_as_it_is_nv12(&still, PixelFormat::Yuv420p, (320, 180)), "another format is converted");
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_download_already_in_the_encoder_s_format_is_given_to_it_by_reference() {
        let download = Frame::software(PixelFormat::Nv12, (320, 180)).expect("a frame");
        assert!(takes_as_it_is(&download, PixelFormat::Nv12, (320, 180)));
        assert!(!takes_as_it_is(&download, PixelFormat::Yuv420p, (320, 180)), "SVT-AV1's planes are converted");
        assert!(!takes_as_it_is(&download, PixelFormat::Nv12, (320, 176)), "another size is converted");
        let given = Reader::default().refer(&download).expect("a reference");
        let luma = |frame: &Frame| frame.plane(0).expect("luma").bytes.as_ptr();
        assert_eq!(luma(&given), luma(&download), "the encoder reads the download's own buffer");
    }

    #[test]
    fn an_encode_is_timed_apart_from_what_made_its_picture_the_encoder_s() {
        let mut session = Session::open(&software(Name::SvtAv1), small()).expect("SVT-AV1 opens");
        assert_eq!(session.timed(), Timed::default(), "nothing encoded, nothing timed");
        let still = Nv12::new(320, 180, vec![128; 320 * 180 * 3 / 2]).expect("a picture");
        session.encode(&still).expect("encoded");
        let timed = session.timed();
        assert!(timed.convert > Duration::ZERO, "SVT-AV1's planes were made from NV12");
        assert!(timed.encode > Duration::ZERO, "and the encoder took its own time");
    }

    mod lab {
        use super::*;

        #[test]
        fn a_memory_route_to_a_hardware_encoder_makes_no_frame_or_buffer_once_it_is_warm() {
            tier::lab();
            let report = crate::survey();
            let Some(encoder) = report.memory_order(Codec::H264).find(|each| each.path() != crate::Path::Software)
            else {
                return tier::skipped("no hardware H.264 encoder");
            };
            let mut session = Session::open(encoder, small()).expect("the hardware encoder opens");
            let still = Nv12::new(320, 180, vec![128; 320 * 180 * 3 / 2]).expect("a picture");
            for _ in 0..20 {
                session.encode(&still).expect("encoded");
            }
            let before = crate::av::counted::made();
            for _ in 0..100 {
                session.encode(&still).expect("encoded");
            }
            assert_eq!(crate::av::counted::made(), before, "{encoder}: pictures allocated frames or buffers");
        }
    }
}
