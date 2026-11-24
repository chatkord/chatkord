//! A camera's decoded picture lifted onto the encoder's own VA-API device and made NV12 there: uploaded once in a
//! layout the device takes (the picture's own, else repacked into one of its sampling with its full range kept), then
//! converted by the device's video processor to the BT.709 limited-range NV12 the encoder tags its stream with. The
//! lifted surface is what the encoder takes and what one's own card paints, so the processor's only work past the
//! decode is the upload, and the repack where one is needed.

use super::{EncodeError, open_device};
use crate::av::{ColourRange, Conversion, Frame, HwDevice, HwFrames, PixelFormat, View, ViewMut};
use crate::colours::{Colours, Matrix};
use crate::decode::mjpeg::Decoded;
use crate::device::surface::Surface;
use crate::device::vaapi::{Scale, Source};
use crate::planes::Planes;
use crate::probe::{Backend, Encoder, Runs, pool};
use std::ffi::{CString, c_int};

/// What the lifted pictures are, as `TO_NV12` makes them and the encoder tags its stream.
const MADE: Colours = Colours::HD;
/// The matrix a camera's pictures are in, which the device converts from.
const CAMERA: Matrix = Matrix::Bt601;
/// The device's conversion, from whatever range and matrix the source says to `MADE`.
const TO_NV12: &str = "format=nv12:out_range=tv:out_color_matrix=bt709";
/// A packed picture's first step, to planes of its own matrix: Mesa's video processor (26.2.6) maps a packed
/// picture's range but not its matrix, which it converts only from planes.
const TO_PLANES: &str = "format=nv12:out_range=tv:out_color_matrix=bt470bg";

/// A layout the device takes in place of a picture's own, and what repacks pictures into it.
struct Repack {
    format: PixelFormat,
    planes: Planes,
    /// The frame the repack is written into, kept from one picture to the next: the upload copies it at once.
    picture: Option<Frame>,
}

impl Repack {
    /// `decoded` repacked into the kept frame of this layout at `size`, its range kept.
    fn repacked(&mut self, decoded: &Decoded, size: (u32, u32)) -> Result<&mut Frame, EncodeError> {
        let kept = match self.picture.take() {
            Some(kept) if kept.format() == Some(self.format) && kept.size() == size && kept.is_writable() => kept,
            _ => Frame::software(self.format, size).map_err(|_| EncodeError::Upload)?,
        };
        let picture = self.picture.insert(kept);
        let from = View::of_frame(decoded.frame()).ok_or(EncodeError::Upload)?;
        let mut to = ViewMut::of_frame(picture).ok_or(EncodeError::Upload)?;
        self.planes.scaler.convert(&from, &mut to, Conversion::Repack).map_err(|_| EncodeError::Upload)?;
        Ok(picture)
    }
}

/// A camera's pictures of one size and layout lifted onto one VA-API device.
pub struct Lift {
    // Declared first: the graph is freed before the frames it was built from.
    scale: Scale,
    upload: HwFrames,
    device: HwDevice,
    /// The pictures' own layout, and the range they are in.
    layout: PixelFormat,
    range: ColourRange,
    /// The layout the device takes them in, where it is not their own.
    repack: Option<Repack>,
    size: (u32, u32),
    /// The size the device makes them.
    out: (u32, u32),
}

// SAFETY: a lift owns every FFmpeg object it holds and shares none; FFmpeg lets them move between threads while one
// thread uses them at a time, which `&mut self` ensures.
unsafe impl Send for Lift {}

/// A camera's picture on the device, NV12, the GPU done with it. Dropping it gives the surface back.
pub struct Lifted(Frame);

// SAFETY: the frame holds references to buffers FFmpeg counts atomically; it is read by one thread at a time.
unsafe impl Send for Lifted {}

impl Lifted {
    /// The surface as DMA-BUFs, for a card to paint where it lies: in the colours the device made it, whatever its
    /// frame says.
    pub fn surface(&self) -> Option<Surface> {
        Surface::export(&self.0).ok().map(|surface| surface.in_colours(MADE))
    }

    /// Another reference to the same surface, for the encoder.
    pub(super) fn reference(&self) -> Option<Frame> {
        self.0.reference()
    }
}

/// The layout a VA-API upload takes for a decoded picture, and the range it is in: a JPEG's planes are FFmpeg's
/// full-range formats, which a device pool names by their plain layout and a frame's range.
fn layout_of(decoded: &Decoded) -> Option<(PixelFormat, ColourRange)> {
    let frame = decoded.frame();
    Some(match frame.format()? {
        PixelFormat::Yuvj420p => (PixelFormat::Yuv420p, ColourRange::Full),
        PixelFormat::Yuvj422p => (PixelFormat::Yuv422p, ColourRange::Full),
        PixelFormat::Yuyv422 => (PixelFormat::Yuyv422, frame.colour_range()),
        PixelFormat::Nv12 => (PixelFormat::Nv12, frame.colour_range()),
        _ => return None,
    })
}

/// What a picture of `layout` is uploaded as on a device that takes `uploads`: itself, else a layout of the same
/// sampling it is repacked into (packed 4:2:2 for planar 4:2:2, NV12 for planar 4:2:0).
fn upload_as(layout: PixelFormat, uploads: &[PixelFormat]) -> Option<PixelFormat> {
    if uploads.contains(&layout) {
        return Some(layout);
    }
    // A packed repack is a shuffle; a 4:2:0 one would resample the chroma on the processor, as dear as memory's route.
    let alike: &[PixelFormat] = match layout {
        PixelFormat::Yuv422p => &[PixelFormat::Yuyv422, PixelFormat::Uyvy422],
        PixelFormat::Yuv420p => &[PixelFormat::Nv12],
        _ => &[],
    };
    alike.iter().copied().find(|each| uploads.contains(each))
}

/// The device's steps from an upload of `taken` to the encoder's NV12.
fn steps(taken: PixelFormat, scaled_to: Option<(u32, u32)>) -> Vec<String> {
    let last = match scaled_to {
        Some((width, height)) => format!("{TO_NV12}:w={width}:h={height}"),
        None => TO_NV12.to_string(),
    };
    if taken.is_planar() { vec![last] } else { vec![TO_PLANES.to_string(), last] }
}

/// The layout, range and size of the pictures a lift takes, so a lift of the same kind is made again without one.
#[derive(Clone, Copy)]
pub struct Inputs {
    layout: PixelFormat,
    range: ColourRange,
    size: (u32, u32),
}

impl Inputs {
    /// What `decoded` is, where a device can take it at all.
    pub fn of(decoded: &Decoded) -> Option<Self> {
        let (layout, range) = layout_of(decoded)?;
        Some(Self { layout, range, size: (decoded.width(), decoded.height()) })
    }

    /// Pixels across and down.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

impl Lift {
    /// A lift onto `encoder`'s own VA-API device for pictures like `first`, at `frames_per_second`; refused where
    /// the encoder is not on VA-API, the device will not upload the picture's layout, or its video processor will
    /// not convert it.
    pub fn open(encoder: &Encoder, first: &Decoded, frames_per_second: u32) -> Result<Self, EncodeError> {
        let inputs = Inputs::of(first).ok_or(EncodeError::Settings)?;
        Self::open_as(encoder, inputs, inputs.size, frames_per_second)
    }

    /// As [`Lift::open`], for pictures of the `inputs` kind, made `out` in size by the device's video processor as it
    /// converts them: a camera stepping down is scaled where it is converted, and its mode never reopens.
    pub fn open_as(
        encoder: &Encoder,
        inputs: Inputs,
        out: (u32, u32),
        frames_per_second: u32,
    ) -> Result<Self, EncodeError> {
        Self::build(encoder, (inputs.layout, inputs.range), inputs.size, out, frames_per_second)
    }

    /// The kind of picture this lift takes.
    pub fn inputs(&self) -> Inputs {
        Inputs { layout: self.layout, range: self.range, size: self.size }
    }

    /// The size the lifted pictures are made.
    pub fn out(&self) -> (u32, u32) {
        self.out
    }

    fn build(
        encoder: &Encoder,
        (layout, range): (PixelFormat, ColourRange),
        size: (u32, u32),
        out: (u32, u32),
        frames_per_second: u32,
    ) -> Result<Self, EncodeError> {
        let Runs::Gpu(named) = &encoder.runs else { return Err(EncodeError::Device) };
        if named.backend != Backend::VaApi {
            return Err(EncodeError::Device);
        }
        let to_c = |value: u32| c_int::try_from(value).ok().filter(|v| *v > 0).ok_or(EncodeError::Settings);
        if out != size && (out.0 == 0 || out.1 == 0 || !out.0.is_multiple_of(2) || !out.1.is_multiple_of(2)) {
            return Err(EncodeError::Settings);
        }
        let sides = (to_c(size.0)?, to_c(size.1)?);
        let device = open_device(named)?;
        let taken = upload_as(layout, &device.uploads()).ok_or(EncodeError::Settings)?;
        let upload = pool(&device, Backend::VaApi, taken, size).ok_or(EncodeError::Frames)?;
        let repack = (taken != layout).then(|| Repack { format: taken, planes: Planes::default(), picture: None });
        let source = Source {
            frames: &upload,
            width: sides.0,
            height: sides.1,
            frames_per_second: to_c(frames_per_second)?,
            // A camera's picture is BT.601's, a JPEG's by JFIF and a raw picture's as UVC sends it.
            space: CAMERA.tag(),
            range,
        };
        let steps = steps(taken, (out != size).then_some(out))
            .into_iter()
            .map(|step| CString::new(step).map_err(|_| EncodeError::Settings))
            .collect::<Result<Vec<_>, _>>()?;
        let scale = Scale::open(&device, &source, &steps)?;
        Ok(Self { scale, upload, device, layout, range, repack, size, out })
    }

    /// `decoded`, a picture of this lift's size and layout, uploaded and made NV12 on the device.
    pub fn lift(&mut self, decoded: &Decoded) -> Result<Lifted, EncodeError> {
        if layout_of(decoded).map(|(layout, _)| layout) != Some(self.layout)
            || (decoded.width(), decoded.height()) != self.size
        {
            return Err(EncodeError::FrameSize);
        }
        let mut renamed = None;
        let view: &mut Frame = match &mut self.repack {
            // The upload reads the picture as its plain layout: a reference of its own, renamed, leaves it as it is.
            None => {
                let mut view = decoded.frame().reference().ok_or(EncodeError::Upload)?;
                view.set_format(self.layout);
                renamed.insert(view)
            }
            Some(repack) => repack.repacked(decoded, self.size)?,
        };
        view.set_colour_range(self.range);
        let mut on_device = Frame::new().ok_or(EncodeError::Upload)?;
        self.upload.upload(view, &mut on_device).map_err(|_| EncodeError::Upload)?;
        on_device.set_colour_range(self.range);
        on_device.set_colour_space(CAMERA.tag());
        self.scale.run(on_device).map(Lifted)
    }

    /// The device the pictures are lifted onto, referenced again: an encoder on it takes them.
    pub(super) fn device(&self) -> Option<HwDevice> {
        self.device.again()
    }

    /// The frames the lifted pictures are drawn from: an encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.scale.frames()
    }
}

#[cfg(test)]
mod tests {
    mod lab {
        use super::super::*;
        use crate::decode::mjpeg::fixtures::decoded;
        use crate::probe::Codec;
        use capture::camera::fixtures::{GREY, RED};
        use tier::skipped;

        /// The first VA-API H.264 encoder this machine has.
        fn on_va_api() -> Option<Encoder> {
            crate::survey()
                .memory_order(Codec::H264)
                .find(|encoder| encoder.device().is_some_and(|device| device.backend == Backend::VaApi))
                .cloned()
        }

        /// `lifted` read back into memory: its luma and its chroma, NV12.
        fn read_back(lifted: &Lifted) -> (Vec<u8>, Vec<u8>) {
            let mut memory = Frame::new().expect("a frame");
            memory.fetch(&lifted.0).expect("downloaded");
            assert_eq!(memory.format(), Some(PixelFormat::Nv12));
            let plane = |index: usize, rows: usize| {
                let plane = memory.plane(index).expect("a plane");
                (0..rows)
                    .flat_map(|row| plane.bytes[row * plane.stride..row * plane.stride + 16].to_vec())
                    .collect::<Vec<u8>>()
            };
            (plane(0, 16), plane(1, 8))
        }

        fn near(samples: &[u8], value: u8) -> bool {
            samples.iter().all(|sample| sample.abs_diff(value) <= 3)
        }

        #[test]
        fn a_lifted_jpeg_is_the_encoder_s_limited_range_bt709_nv12() {
            let Some(encoder) = on_va_api() else { return skipped("no VA-API H.264 encoder") };
            let mut lift = match Lift::open(&encoder, &decoded(GREY), 30) {
                Ok(lift) => lift,
                Err(why) => return skipped(format!("this device does not lift a JPEG: {why:?}")),
            };
            let (luma, chroma) = read_back(&lift.lift(&decoded(GREY)).expect("lifted"));
            assert!(near(&luma, 126), "full-range grey 128 is limited 126: {luma:?}");
            assert!(near(&chroma, 128), "no colour: {chroma:?}");
            let (luma, chroma) = read_back(&lift.lift(&decoded(RED)).expect("lifted"));
            let (blue, red): (Vec<u8>, Vec<u8>) = chroma.chunks(2).map(|pair| (pair[0], pair[1])).unzip();
            // BT.709's limited red; BT.601's, unconverted, would be 81, 90 and 240.
            assert!(near(&luma, 63) && near(&blue, 102) && near(&red, 240), "BT.709's red: {luma:?} {chroma:?}");
            assert!(
                lift.lift(&decoded(GREY)).and_then(|lifted| lifted.surface().ok_or(EncodeError::Convert)).is_ok(),
                "its surface exports for a card"
            );
        }
    }
}

#[cfg(test)]
mod layouts {
    use super::*;
    use PixelFormat as F;

    #[test]
    fn a_picture_goes_up_as_it_is_else_as_its_sampling_and_a_packed_one_reaches_planes_first() {
        let takes = [F::Nv12, F::Yuyv422];
        assert_eq!(upload_as(F::Nv12, &takes), Some(F::Nv12));
        assert_eq!(upload_as(F::Yuyv422, &takes), Some(F::Yuyv422));
        assert_eq!(upload_as(F::Yuv422p, &takes), Some(F::Yuyv422));
        assert_eq!(upload_as(F::Yuv420p, &takes), Some(F::Nv12));
        assert_eq!(upload_as(F::Yuv422p, &[F::Nv12]), None);
        assert_eq!(steps(F::Nv12, None), [TO_NV12]);
        assert_eq!(steps(F::Yuv422p, None), [TO_NV12]);
        assert_eq!(steps(F::Yuyv422, None), [TO_PLANES, TO_NV12]);
        assert_eq!(steps(F::Nv12, Some((640, 360))), ["format=nv12:out_range=tv:out_color_matrix=bt709:w=640:h=360"]);
    }
}
