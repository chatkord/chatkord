//! An encoded stream in, pictures out. Hardware is tried first and software is the fallback on purpose; the path
//! taken is reported, so a stream that began on hardware and was refused says so and carries on.

pub(crate) mod mjpeg;

use crate::av::{AvError, CodecContext, CodecRef, Conversion, Frame, HwDevice, PixelFormat, View, ViewMut};
use crate::coded::Coded;
use crate::colours::{Colours, Range};
#[cfg(target_os = "linux")]
use crate::device::surface::Surface;
use crate::encode::{FrameError, Nv12};
use crate::picture::Picture;
use crate::planes::Planes;
use crate::probe::{Backend, Codec, On, Path, Sources};
use crate::size::{MAX_PIXELS, fits};
use std::ffi::c_int;
use std::fmt;

impl Path {
    /// Whether a decode on this path leaves its pictures where the window paints them: VA-API's surfaces on Linux,
    /// the window's own D3D12 device on Windows. Every other path's pictures are in memory, downloaded from a device.
    pub fn leaves_on_gpu(self) -> bool {
        match self {
            Self::Hardware(Backend::VaApi) => cfg!(target_os = "linux"),
            #[cfg(windows)]
            Self::Hardware(Backend::D3d12) => true,
            #[cfg(windows)]
            Self::Hardware(Backend::D3d11) => false,
            Self::Hardware(Backend::Vulkan) | Self::Software => false,
        }
    }
}

/// One decoded picture and the timestamp its unit carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The picture, copied to memory.
    pub picture: Nv12,
    /// The timestamp the unit was decoded with.
    pub pts: i64,
}

/// A picture as a window that imports takes it: where the decoder left it, or in memory.
pub enum Shown {
    /// On the GPU where the window paints it.
    #[cfg(target_os = "linux")]
    OnGpu(Surface),
    /// On the window's own D3D12 device.
    #[cfg(windows)]
    OnGpu(crate::D3d12Surface),
    /// In memory, from a path that cannot leave it on the GPU: the decoder's own planes or its download, by
    /// reference, converted first only where its layout is neither NV12 nor 8-bit 4:2:0.
    InMemory(Picture),
}

impl Shown {
    #[cfg(any(target_os = "linux", windows))]
    fn make(path: Path, copier: &mut Copier, frame: &Frame) -> Result<Self, DecodeError> {
        if !path.leaves_on_gpu() {
            return copier.in_memory(frame).map(Self::InMemory);
        }
        #[cfg(target_os = "linux")]
        let on_gpu = Surface::export(frame);
        #[cfg(windows)]
        let on_gpu = crate::D3d12Surface::of(frame, &mut copier.lender);
        on_gpu.map(Self::OnGpu)
    }
}

/// Why a unit could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// No path, software included, would take the stream.
    Refused,
    /// The decoder produced a picture larger than `MAX_PICTURE`, with an odd or zero side, or one
    /// this module cannot convert.
    Picture,
    /// The decoder failed, as FFmpeg says.
    Decoder(AvError),
    /// The decoder took the stream's units and made no picture of them.
    NoPicture,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => f.write_str("no video decoder would take this stream"),
            Self::Picture => f.write_str("the decoder made a picture this client cannot show"),
            Self::Decoder(error) => write!(f, "the video decoder failed ({error})"),
            Self::NoPicture => f.write_str("the video decoder made no picture of the stream"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<FrameError> for DecodeError {
    fn from(_: FrameError) -> Self {
        Self::Picture
    }
}

/// Makes one picture of a frame the path decoded, with what copying keeps between pictures.
type Make<T> = fn(Path, &mut Copier, &Frame) -> Result<T, DecodeError>;

/// One way of decoding, open and ready.
struct Attempt {
    context: CodecContext,
    _device: Option<HwDevice>,
    path: Path,
    copier: Copier,
}

/// What copying a decoded picture to the CPU keeps from one picture to the next.
#[derive(Default)]
struct Copier {
    planes: Planes,
    /// The pool the last picture left on the window's device was lent from, until the decoder lends from another.
    #[cfg(windows)]
    lender: Option<std::sync::Arc<crate::Lender>>,
}

/// How many pictures libdav1d works on at once, a thread each, and so how many units late a picture may come out. One
/// at a time, a 4K60 share of one tile took 19.7 ms a picture on two threads and 27.9 on one against the 16.7 it has
/// (Machine B, copy out included); more threads than pictures in flight cost CPU and gained nothing.
const DAV1D_PICTURES_AT_ONCE: c_int = 2;

/// Lets libdav1d decode [`DAV1D_PICTURES_AT_ONCE`] pictures at once, a thread each; FFmpeg's default is one thread.
/// False when refused. `context` is allocated for libdav1d, not yet opened.
fn decode_pictures_at_once(context: &mut CodecContext) -> bool {
    context.set_threads(DAV1D_PICTURES_AT_ONCE);
    context.set_option_int(c"max_frame_delay", i64::from(DAV1D_PICTURES_AT_ONCE))
}

impl Attempt {
    /// Opens `path` for `codec`, to refuse a stream whose pictures have more than `max_pixels`.
    fn open(codec: Codec, path: Path, on: &On, max_pixels: i64) -> Option<Self> {
        let name = match (codec, path) {
            // FFmpeg's own, which takes High, on one thread: a 4K60 High picture took 6.0 to 6.3 ms on Machine B,
            // and each thread more is a picture late.
            (Codec::H264, _) => c"h264",
            (Codec::Av1, Path::Software) => c"libdav1d",
            (Codec::Av1, Path::Hardware(_)) => c"av1",
        };
        let decoder = CodecRef::decoder(name)?;
        let mut context = CodecContext::new(Some(decoder))?;
        context.set_max_pixels(max_pixels);
        let device = match path {
            Path::Software => None,
            Path::Hardware(backend) => {
                let device = on.open(backend)?;
                if !crate::decoders::decodes(&device, codec) || !context.set_hw_decode(&device, backend.pixel_format())
                {
                    return None;
                }
                Some(device)
            }
        };
        if (codec, path) == (Codec::Av1, Path::Software) && !decode_pictures_at_once(&mut context) {
            return None;
        }
        context.open().ok()?;
        Some(Self { context, _device: device, path, copier: Copier::default() })
    }
}

/// What a decoder keeps to replay on a fallback before its first picture, chosen to hold several 4K key frames: a
/// stream with no picture past it is not one a fallback can save.
const MAX_REPLAY_BYTES: usize = 32 * 1024 * 1024;

/// How many units in a row a path may take before its first picture: past it, the path will never show the stream.
/// The most any path held back was 2 (libdav1d; none on D3D11VA, VA-API or FFmpeg's H.264, Machines A and B).
const UNITS_WITHOUT_A_PICTURE: usize = 16;

/// A running decode.
pub struct Decoder {
    codec: Codec,
    /// The most pixels a picture of the stream may have, given to every path opened.
    max_pixels: i64,
    /// Ways still to try if this one is refused, nearest first.
    later: Vec<(Path, On)>,
    current: Attempt,
    /// Paths that were opened and then refused the stream.
    refused: Vec<Path>,
    /// Every unit sent to the path in use, kept until a picture comes out so a fallback can replay them; none once
    /// they passed `MAX_REPLAY_BYTES`, when the next path starts at a key frame instead.
    replay: Option<Vec<(Coded, i64)>>,
    /// The bytes `replay` holds, bounded by `MAX_REPLAY_BYTES`.
    replay_bytes: usize,
    /// Units the path in use took, since it opened, without a picture or a failure.
    unanswered: usize,
    produced: bool,
}

// SAFETY: a decoder owns its FFmpeg contexts and the device they were made on; nothing else points into them, and
// every call takes `&mut self`, so it may move to the thread that decodes.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Opens the first path that takes `codec`: each hardware device in `sources` (VA-API's render nodes, Vulkan; on
    /// Windows the window's D3D12 device, then D3D11), then software.
    pub fn open(codec: Codec, sources: &Sources) -> Result<Self, DecodeError> {
        Self::open_within(codec, sources, MAX_PIXELS)
    }

    /// Opens `codec` on `path` alone: for a graphics path the first of `sources`' devices of its backend, with the
    /// paths after it in [`Decoder::open`]'s order to fall back to. A path another decoder refused is tried again this
    /// way, as a decoder of its own. [`DecodeError::Refused`] when `path` does not open.
    pub fn open_on(codec: Codec, path: Path, sources: &Sources) -> Result<Self, DecodeError> {
        let mut plan = plan(sources).into_iter().skip_while(|(planned, _)| *planned != path);
        let (path, on) = plan.next().ok_or(DecodeError::Refused)?;
        let current = Attempt::open(codec, path, &on, MAX_PIXELS).ok_or(DecodeError::Refused)?;
        Ok(Self::opened(codec, MAX_PIXELS, current, plan.collect(), Vec::new()))
    }

    fn open_within(codec: Codec, sources: &Sources, max_pixels: i64) -> Result<Self, DecodeError> {
        let mut refused = Vec::new();
        let mut plan = plan(sources).into_iter();
        while let Some((path, on)) = plan.next() {
            if let Some(current) = Attempt::open(codec, path, &on, max_pixels) {
                return Ok(Self::opened(codec, max_pixels, current, plan.collect(), refused));
            }
            refused.push(path);
        }
        Err(DecodeError::Refused)
    }

    /// A decode on `current`, which has shown nothing yet, with `later` to fall back to.
    fn opened(codec: Codec, max_pixels: i64, current: Attempt, later: Vec<(Path, On)>, refused: Vec<Path>) -> Self {
        Self {
            codec,
            max_pixels,
            later,
            current,
            refused,
            replay: Some(Vec::new()),
            replay_bytes: 0,
            unanswered: 0,
            produced: false,
        }
    }

    /// The path decoding is on now.
    pub fn path(&self) -> Path {
        self.current.path
    }

    /// Paths that were tried and did not take the stream, in the order they were tried.
    pub fn refused(&self) -> &[Path] {
        &self.refused
    }

    /// The path in use is broken: it refused a key frame after it showed pictures (a device lost, a size it will not
    /// take, a pool run dry), which no later key frame mends. Opens the next path that opens, software last, for the
    /// next unit, which must be a key frame, and returns it; none when no path is left, the decoder staying on the one
    /// it has.
    pub fn hand_over(&mut self) -> Option<Path> {
        while !self.later.is_empty() {
            let (path, on) = self.later.remove(0);
            let Some(next) = Attempt::open(self.codec, path, &on, self.max_pixels) else {
                self.refused.push(path);
                continue;
            };
            self.refused.push(self.current.path);
            self.current = next;
            // The new path has shown nothing yet, so a refusal before its first picture falls back as at the start.
            self.produced = false;
            self.replay = Some(Vec::new());
            self.replay_bytes = 0;
            self.unanswered = 0;
            return Some(path);
        }
        None
    }

    /// Decodes one unit and returns the pictures it finished in memory, each the decoder's own or its download, by
    /// reference: for a window that cannot import them. A picture held is one its buffers are not written again under.
    pub fn decode_in_memory(&mut self, unit: &Coded, pts: i64) -> Result<Vec<Picture>, DecodeError> {
        self.run(unit, pts, |_, copier, frame| copier.in_memory(frame))
    }

    /// Decodes one unit and returns the pictures it finished, copied to the CPU.
    pub fn decode(&mut self, unit: &Coded, pts: i64) -> Result<Vec<Decoded>, DecodeError> {
        self.run(unit, pts, |_, copier, frame| copier.picture(frame))
    }

    /// Decodes one unit and returns the pictures it finished as DMA-BUF surfaces, with no copy; on a path other than
    /// VA-API this is [`DecodeError::Picture`] and the caller uses [`Decoder::decode`]. A surface held is one the
    /// decoder cannot reuse, and its pool grows to make another, so hold a few at a time.
    #[cfg(target_os = "linux")]
    pub fn decode_surfaces(&mut self, unit: &Coded, pts: i64) -> Result<Vec<Surface>, DecodeError> {
        if !self.current.path.leaves_on_gpu() {
            return Err(DecodeError::Picture);
        }
        self.run(unit, pts, |_, _, frame| Surface::export(frame))
    }

    /// Decodes one unit and returns the pictures it finished on the window's D3D12 device, with no copy; on another
    /// path this is [`DecodeError::Picture`] and the caller uses [`Decoder::decode`]. A picture held is one the
    /// decoder cannot write again until it is dropped.
    #[cfg(windows)]
    pub fn decode_d3d12(&mut self, unit: &Coded, pts: i64) -> Result<Vec<crate::D3d12Surface>, DecodeError> {
        if !self.current.path.leaves_on_gpu() {
            return Err(DecodeError::Picture);
        }
        self.run(unit, pts, |_, copier, frame| crate::D3d12Surface::of(frame, &mut copier.lender))
    }

    /// Decodes one unit for a window that takes pictures where they were decoded: each picture on the GPU while the
    /// path leaves it there, in memory otherwise, so a fallback's replayed pictures arrive in memory.
    #[cfg(any(target_os = "linux", windows))]
    pub fn decode_shown(&mut self, unit: &Coded, pts: i64) -> Result<Vec<Shown>, DecodeError> {
        self.run(unit, pts, Shown::make)
    }

    fn run<T>(&mut self, unit: &Coded, pts: i64, make: Make<T>) -> Result<Vec<T>, DecodeError> {
        if !self.produced && !self.later.is_empty() {
            if self.replay_bytes + unit.len() > MAX_REPLAY_BYTES {
                self.replay = None;
                self.replay_bytes = 0;
            } else if let Some(kept) = &mut self.replay {
                kept.push((unit.clone(), pts));
                self.replay_bytes += unit.len();
            }
        }
        match send_and_drain(&mut self.current, unit, pts, make) {
            Ok(pictures) if self.produced => Ok(pictures),
            Ok(pictures) if pictures.is_empty() => {
                self.unanswered += 1;
                if self.unanswered > UNITS_WITHOUT_A_PICTURE && !self.later.is_empty() {
                    return self.fall_back(DecodeError::NoPicture, make);
                }
                Ok(pictures)
            }
            Ok(pictures) => {
                self.shown();
                Ok(pictures)
            }
            Err(error) if !self.produced => self.fall_back(error, make),
            Err(error) => Err(error),
        }
    }

    /// The path in use made its first picture: nothing is replayed from here.
    fn shown(&mut self) {
        self.produced = true;
        self.replay = None;
        self.replay_bytes = 0;
    }

    /// Ends the stream and returns the surfaces the decoder still held; see [`Decoder::decode_surfaces`].
    #[cfg(target_os = "linux")]
    pub fn finish_surfaces(&mut self) -> Result<Vec<Surface>, DecodeError> {
        self.end(|_, _, frame| Surface::export(frame))
    }

    /// Ends the stream and returns what the decoder still held.
    pub fn finish(&mut self) -> Result<Vec<Decoded>, DecodeError> {
        self.end(|_, copier, frame| copier.picture(frame))
    }

    fn end<T>(&mut self, make: Make<T>) -> Result<Vec<T>, DecodeError> {
        self.current.context.send_packet(None).map_err(DecodeError::Decoder)?;
        drain(&mut self.current, make)
    }

    /// The path in use refused the stream, or made nothing of it, before showing a picture: take the next and replay.
    /// With the units no longer kept, the next path is only opened, and the failure asks its caller for a key frame.
    fn fall_back<T>(&mut self, error: DecodeError, make: Make<T>) -> Result<Vec<T>, DecodeError> {
        let Some(replay) = self.replay.take() else {
            self.hand_over();
            return Err(error);
        };
        let mut error = error;
        while !self.later.is_empty() {
            let (path, on) = self.later.remove(0);
            let Some(next) = Attempt::open(self.codec, path, &on, self.max_pixels) else {
                self.refused.push(path);
                continue;
            };
            self.refused.push(self.current.path);
            self.current = next;
            self.unanswered = 0;
            let mut pictures = Vec::new();
            let mut failed = None;
            for (unit, pts) in &replay {
                match send_and_drain(&mut self.current, unit, *pts, make) {
                    Ok(mut more) => pictures.append(&mut more),
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            match failed {
                None => {
                    if pictures.is_empty() {
                        self.replay = Some(replay);
                    } else {
                        self.shown();
                    }
                    return Ok(pictures);
                }
                Some(e) => error = e,
            }
        }
        self.replay = Some(replay);
        Err(error)
    }
}

/// Every path `sources` offer, in the order a decoder tries them: each device, then software, which needs none and is
/// opened `On::Default`.
fn plan(sources: &Sources) -> Vec<(Path, On)> {
    let mut plan: Vec<(Path, On)> =
        sources.devices().into_iter().map(|(backend, on)| (Path::Hardware(backend), on)).collect();
    plan.push((Path::Software, On::Default));
    plan
}

/// Sends one unit and returns the pictures it finished. A full decoder (`EAGAIN`) is drained and given the same unit
/// again, so no unit is lost; full a second time is a failure.
fn send_and_drain<T>(attempt: &mut Attempt, unit: &Coded, pts: i64, make: Make<T>) -> Result<Vec<T>, DecodeError> {
    let packet = unit.stamped(pts).map_err(|_| DecodeError::Decoder(AvError::NoMemory))?;
    let mut out = Vec::new();
    let mut sent = attempt.context.send_packet(Some(&packet));
    if sent == Err(AvError::Again) {
        out = drain(attempt, make)?;
        sent = attempt.context.send_packet(Some(&packet));
    }
    sent.map_err(DecodeError::Decoder)?;
    out.append(&mut drain(attempt, make)?);
    Ok(out)
}

fn drain<T>(attempt: &mut Attempt, make: Make<T>) -> Result<Vec<T>, DecodeError> {
    let mut frame = Frame::new().ok_or(DecodeError::Decoder(AvError::NoMemory))?;
    let mut out = Vec::new();
    loop {
        match attempt.context.receive_frame(&mut frame) {
            Ok(()) => {}
            Err(AvError::Again | AvError::End) => return Ok(out),
            Err(error) => return Err(DecodeError::Decoder(error)),
        }
        let (width, height) = frame.size();
        if !fits(width, height) {
            return Err(DecodeError::Picture);
        }
        out.push(make(attempt.path, &mut attempt.copier, &frame)?);
        // The decoder's buffers are let go for the next receive.
        frame.unref();
    }
}

/// What `source`'s samples are in once the converter has made them NV12: it maps a full-range format (YUVJ) to
/// limited range and leaves any other's range as it is.
fn converted_colours(source: &Frame) -> Colours {
    match Colours::of(source) {
        said if source.format().is_some_and(PixelFormat::is_full_range_spelling) => {
            Colours { range: Range::Limited, ..said }
        }
        said => said,
    }
}

impl Copier {
    /// A decoded frame in memory, by reference: the decoder's own, or its download from a device into a frame kept for
    /// it. A layout the window does not take (4:2:2, a full-range spelling) is converted to NV12 into a kept frame; a
    /// format this client does not name (10-bit) is refused, as a copy refuses it.
    fn in_memory(&mut self, frame: &Frame) -> Result<Picture, DecodeError> {
        let Planes { scaler, download, converted } = &mut self.planes;
        let source = download.in_memory(frame).ok_or(DecodeError::Picture)?;
        if let Some(picture) = Picture::of(source) {
            return Ok(picture);
        }
        let colours = converted_colours(source);
        let into = converted.frame(PixelFormat::Nv12, source.size()).ok_or(DecodeError::Picture)?;
        let from = View::of_frame(source).ok_or(DecodeError::Picture)?;
        let mut to = ViewMut::of_frame(into).ok_or(DecodeError::Picture)?;
        scaler.convert(&from, &mut to, Conversion::Colour).map_err(|_| DecodeError::Picture)?;
        into.set_pts(source.pts());
        Picture::in_colours(into, colours).ok_or(DecodeError::Picture)
    }

    /// Brings a decoded frame, in system memory or on a device, to a packed NV12 picture of its own.
    fn picture(&mut self, frame: &Frame) -> Result<Decoded, DecodeError> {
        let source = self.planes.download.in_memory(frame).ok_or(DecodeError::Picture)?;
        let (wide, high) = source.size();
        let width = usize::try_from(wide).map_err(|_| DecodeError::Picture)?;
        let height = usize::try_from(high).map_err(|_| DecodeError::Picture)?;
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(DecodeError::Picture);
        }
        let pts = source.pts();
        let colours = converted_colours(source);
        let from = View::of_frame(source).ok_or(DecodeError::Picture)?;
        let mut data = vec![0u8; width * height * 3 / 2];
        let (luma, chroma) = data.split_at_mut(width * height);
        let mut to = ViewMut::nv12((wide, high), luma, chroma).ok_or(DecodeError::Picture)?;
        self.planes.scaler.convert(&from, &mut to, Conversion::Colour).map_err(|_| DecodeError::Picture)?;
        Ok(Decoded { picture: Nv12::new(width, height, data)?.in_colours(colours), pts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tier::skipped;

    const UNIT_BYTES: usize = 1024 * 1024;

    /// A quarter more units than the replay's bound holds.
    const PAST_THE_BOUND: usize = MAX_REPLAY_BYTES / UNIT_BYTES * 5 / 4;

    #[cfg(target_os = "linux")]
    #[test]
    fn only_va_api_leaves_its_pictures_where_the_window_paints_them_on_linux() {
        assert!(Path::Hardware(Backend::VaApi).leaves_on_gpu());
        assert!(!Path::Hardware(Backend::Vulkan).leaves_on_gpu());
        assert!(!Path::Software.leaves_on_gpu());
    }

    /// A unit that looks like the start of a key frame and is nothing else.
    fn garbage() -> Vec<u8> {
        let mut unit = vec![0_u8; UNIT_BYTES];
        unit[..4].copy_from_slice(&[0x00, 0x00, 0x01, 0x65]);
        unit
    }

    impl Decoder {
        fn replayed_bytes(&self) -> usize {
            self.replay_bytes
        }
    }

    /// A short H.264 stream from the first hardware encoder that opens, one picture a unit.
    fn stream(width: usize, height: usize) -> Option<Vec<crate::Encoded>> {
        let encoder = crate::survey().encoders.into_iter().find(|encoder| encoder.codec() == Codec::H264)?;
        let settings = crate::Settings {
            width,
            height,
            frames_per_second: 30,
            bits_per_second: 1_000_000,
            key_frame_every: std::num::NonZeroU32::new(30),
        };
        let mut session = crate::Session::open(&encoder, settings).ok()?;
        let mut units = Vec::new();
        for n in 0..30 {
            let mut data = vec![128_u8; width * height * 3 / 2];
            data[..width * height]
                .iter_mut()
                .enumerate()
                .for_each(|(at, luma)| *luma = u8::try_from((at + n * 7) % 220 + 16).expect("below 236"));
            units.extend(session.encode(&Nv12::new(width, height, data).ok()?).ok()?);
        }
        units.extend(session.finish().ok()?);
        Some(units)
    }

    fn software() -> Sources {
        Sources::default()
    }

    /// An AV1 unit that holds a temporal delimiter and no picture: a decoder takes it and shows nothing.
    const NO_PICTURE: [u8; 2] = [0x12, 0x00];

    #[test]
    fn a_path_that_takes_units_and_makes_no_picture_hands_over_to_the_next() {
        let mut decoder = Decoder::open(Codec::Av1, &software()).expect("libdav1d");
        decoder.later.push((Path::Software, On::Default));
        let unit = Coded::copied(&NO_PICTURE).expect("a unit");
        for sent in 0..UNITS_WITHOUT_A_PICTURE {
            assert_eq!(decoder.decode(&unit, 0).map(|pictures| pictures.len()), Ok(0), "unit {sent} is taken");
            assert!(decoder.refused().is_empty(), "within the bound the path is kept: unit {sent}");
        }
        assert_eq!(decoder.decode(&unit, 0).map(|pictures| pictures.len()), Ok(0), "replayed on the next path");
        assert_eq!(decoder.refused(), [Path::Software], "the path that showed nothing is refused");
        assert_eq!(decoder.unanswered, 0, "the next path's count starts again");
    }

    #[test]
    fn a_path_that_makes_no_picture_stays_when_no_other_is_left() {
        let mut decoder = Decoder::open(Codec::Av1, &software()).expect("libdav1d");
        let unit = Coded::copied(&NO_PICTURE).expect("a unit");
        for _ in 0..=UNITS_WITHOUT_A_PICTURE {
            assert_eq!(decoder.decode(&unit, 0).map(|pictures| pictures.len()), Ok(0));
        }
        assert!(decoder.refused().is_empty());
    }

    #[test]
    fn a_path_whose_units_are_no_longer_kept_hands_over_and_asks_for_a_key_frame() {
        let mut decoder = Decoder::open(Codec::Av1, &software()).expect("libdav1d");
        decoder.later.push((Path::Software, On::Default));
        decoder.replay = None;
        let unit = Coded::copied(&NO_PICTURE).expect("a unit");
        for _ in 0..UNITS_WITHOUT_A_PICTURE {
            assert_eq!(decoder.decode(&unit, 0).map(|pictures| pictures.len()), Ok(0));
        }
        assert_eq!(decoder.decode(&unit, 0).map(|pictures| pictures.len()), Err(DecodeError::NoPicture));
        assert_eq!(decoder.refused(), [Path::Software]);
        assert_eq!(decoder.replay.as_ref().map(Vec::len), Some(0), "the next path keeps its units from its start");
    }

    #[test]
    fn a_decoder_opened_on_a_path_takes_that_one_and_falls_back_only_to_those_after_it() {
        let opened = Decoder::open_on(Codec::Av1, Path::Software, &software()).expect("libdav1d");
        assert_eq!(opened.path(), Path::Software);
        assert!(opened.later.is_empty() && opened.refused().is_empty(), "nothing after software");
        let unoffered = Decoder::open_on(Codec::Av1, Path::Hardware(Backend::Vulkan), &software());
        assert_eq!(unoffered.err(), Some(DecodeError::Refused), "a path the sources do not offer is not opened");
    }

    /// Each plane's first byte, as the window would upload it.
    fn starts(samples: crate::Samples<'_>) -> Vec<*const u8> {
        match samples {
            crate::Samples::Nv12(planes) => planes.iter().map(|(bytes, _)| bytes.as_ptr()).collect(),
            crate::Samples::I420(planes) => planes.iter().map(|(bytes, _)| bytes.as_ptr()).collect(),
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_picture_in_memory_reaches_the_window_as_the_decoder_s_own_planes_in_either_layout() {
        for format in [PixelFormat::Yuv420p, PixelFormat::Nv12] {
            let decoded = Frame::software(format, (64, 32)).expect("a frame");
            let own: Vec<*const u8> =
                (0..3).map_while(|plane| decoded.plane(plane)).map(|plane| plane.bytes.as_ptr()).collect();
            let mut copier = Copier::default();
            let Ok(Shown::InMemory(shown)) = Shown::make(Path::Software, &mut copier, &decoded) else {
                panic!("{format:?}: a software picture is shown in memory");
            };
            assert_eq!(starts(shown.samples()), own, "{format:?}: the window is handed the decoder's own planes");
            let refused = copier.in_memory(&decoded).expect("in memory");
            assert_eq!(starts(refused.samples()), own, "{format:?}: and so is a window that refused an import");
        }
    }

    #[test]
    fn a_layout_the_window_does_not_take_is_converted_into_a_frame_no_picture_holds() {
        let decoded = Frame::software(PixelFormat::Yuv422p, (64, 32)).expect("a frame");
        let mut copier = Copier::default();
        let first = copier.in_memory(&decoded).expect("converted");
        assert_eq!(first.layout(), crate::PictureLayout::Nv12);
        let second = copier.in_memory(&decoded).expect("converted");
        assert_ne!(starts(first.samples()), starts(second.samples()), "a picture held is not written over");
        drop((first, second));
        let before = crate::av::counted::made();
        drop(copier.in_memory(&decoded).expect("converted"));
        assert_eq!(crate::av::counted::made().1, before.1, "a frame let go of is written again, no buffer made");
    }

    mod lab {
        use super::*;

        #[test]
        fn garbage_never_grows_the_replay_past_its_bound() {
            let Ok(mut decoder) =
                Decoder::open(Codec::H264, &Sources { render_nodes: vec!["/dev/null".into()], ..Sources::default() })
            else {
                return skipped("no H.264 decoder");
            };
            let unit = Coded::copied(&garbage()).expect("a unit");
            for _ in 0..PAST_THE_BOUND {
                let _ = decoder.decode(&unit, 0);
                assert!(decoder.replayed_bytes() <= MAX_REPLAY_BYTES);
            }
            assert_eq!(decoder.replayed_bytes(), 0, "software only: nothing can be replayed, so nothing is kept");
        }

        #[test]
        fn a_pending_fallback_stops_being_kept_past_the_bound() {
            let Ok(mut decoder) = Decoder::open(Codec::H264, &Sources::default()) else {
                return skipped("no H.264 decoder");
            };
            let unit = Coded::copied(&garbage()).expect("a unit");
            let mut kept_most = 0;
            for _ in 0..PAST_THE_BOUND {
                // A way still to try, so the units are kept as they would be before a fallback.
                if decoder.later.is_empty() && decoder.replay_bytes + unit.len() <= MAX_REPLAY_BYTES {
                    decoder.later.push((Path::Software, On::Default));
                }
                let _ = decoder.decode(&unit, 0);
                kept_most = kept_most.max(decoder.replayed_bytes());
                assert!(decoder.replayed_bytes() <= MAX_REPLAY_BYTES);
            }
            assert!(kept_most > 0, "units before the bound are kept for a fallback");
        }

        #[test]
        fn a_broken_path_hands_over_to_the_next_and_stays_on_the_last_when_none_is_left() {
            let Ok(mut decoder) = Decoder::open(Codec::H264, &Sources::default()) else {
                return skipped("no H.264 decoder");
            };
            decoder.produced = true;
            decoder.later.push((Path::Software, On::Default));
            assert_eq!(decoder.hand_over(), Some(Path::Software));
            assert_eq!(decoder.refused(), [Path::Software], "the broken one is refused");
            assert!(!decoder.produced, "the new path has shown nothing, so it may still fall back");
            assert_eq!(decoder.hand_over(), None, "none left");
            assert_eq!((decoder.path(), decoder.refused().len()), (Path::Software, 1), "it stays on the one it has");
        }

        #[test]
        fn a_stream_of_more_pixels_than_its_bound_is_refused_on_every_path_before_any_picture() {
            let Some(stream) = stream(1280, 720) else { return skipped("no H.264 encoder to make a stream") };
            for (label, sources) in [("software", software()), ("this system's devices", Sources::system())] {
                let mut within = Decoder::open_within(Codec::H264, &sources, 1280 * 720).expect("a decoder");
                let shown: usize =
                    stream.iter().filter_map(|unit| within.decode(&unit.data, unit.pts).ok()).flatten().count();
                assert!(shown > 0, "{label}: a stream at its bound is shown");
                let mut beyond = Decoder::open_within(Codec::H264, &sources, 1280 * 720 - 1).expect("a decoder");
                for unit in &stream {
                    let decoded = beyond.decode(&unit.data, unit.pts);
                    assert!(!decoded.is_ok_and(|pictures| !pictures.is_empty()), "{label}: a picture past the bound");
                }
                assert_eq!(beyond.path(), Path::Software, "{label}: every path before the last refused it");
            }
        }

        #[test]
        fn a_full_decoder_is_drained_and_given_the_same_unit_again() {
            let Some(stream) = stream(640, 360) else { return skipped("no hardware H.264 encoder to make a stream") };
            let mut decoder = Decoder::open(Codec::H264, &software()).expect("a software decoder");
            // Units sent with no picture taken fill the decoder until it refuses one.
            let mut sent = 0;
            loop {
                let packet = stream[sent].data.stamped(stream[sent].pts).expect("a packet");
                match decoder.current.context.send_packet(Some(&packet)) {
                    Err(AvError::Again) => break,
                    Err(error) => panic!("the decoder failed: {error}"),
                    Ok(()) => {}
                }
                sent += 1;
                assert!(sent < stream.len(), "the decoder never filled");
            }
            let mut pictures = 0;
            for unit in &stream[sent..] {
                pictures += decoder.decode(&unit.data, unit.pts).expect("decoded").len();
            }
            pictures += decoder.finish().expect("finished").len();
            assert_eq!(pictures, stream.len(), "every unit made its picture, the refused one too");
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn a_fallback_before_the_first_picture_hands_its_pictures_to_the_window_as_copies() {
            let Some(stream) = stream(640, 360) else { return skipped("no hardware H.264 encoder to make a stream") };
            let on_va_api = Sources { render_nodes: Sources::system().render_nodes, ..Sources::default() };
            let mut decoder = match Decoder::open(Codec::Av1, &on_va_api) {
                Ok(decoder) if decoder.path() == Path::Hardware(Backend::VaApi) => decoder,
                _ => return skipped("no VA-API AV1 decoder"),
            };
            // An AV1 decoder on the GPU takes H.264 without a picture, then refuses an OBU with its forbidden bit set,
            // as a driver that opens a codec it cannot decode does; H.264 in software skips that byte to the unit after.
            decoder.codec = Codec::H264;
            let (before, after) = stream.split_at(stream.len() / 2);
            for unit in before {
                assert!(decoder.decode_shown(&unit.data, unit.pts).expect("taken").is_empty());
            }
            let ((first, after), mut refusing) = (after.split_first().expect("a unit after"), vec![0x80]);
            refusing.extend_from_slice(&first.data);
            let refused = decoder
                .decode_shown(&Coded::copied(&refusing).expect("a unit"), first.pts)
                .expect("the replayed pictures, not a failure");
            assert_eq!(decoder.path(), Path::Software);
            assert!(!refused.is_empty(), "the replay made pictures");
            let mut copies = refused.iter().filter(|picture| matches!(picture, Shown::InMemory(_))).count();
            for unit in after {
                let shown = decoder.decode_shown(&unit.data, unit.pts).expect("decoded");
                copies += shown.iter().filter(|picture| matches!(picture, Shown::InMemory(_))).count();
            }
            assert!(copies + 2 >= stream.len(), "{copies} copies of {} units", stream.len());
        }

        #[test]
        fn one_converter_serves_a_stream_until_its_size_changes() {
            let (Some(small), Some(large)) = (stream(640, 360), stream(1280, 720)) else {
                return skipped("no hardware H.264 encoder to make a stream");
            };
            let mut decoder = Decoder::open(Codec::H264, &software()).expect("a software decoder");
            let mut pictures = 0;
            for unit in &small {
                pictures += decoder.decode(&unit.data, unit.pts).expect("decoded").len();
            }
            assert!(pictures > 1);
            assert_eq!(
                decoder.current.copier.planes.scaler.made, 1,
                "one converter for {pictures} pictures of one size"
            );
            for unit in &large {
                decoder.decode(&unit.data, unit.pts).expect("decoded");
            }
            decoder.finish().expect("finished");
            assert_eq!(decoder.current.copier.planes.scaler.made, 2, "another for the new size");
        }
    }
}
