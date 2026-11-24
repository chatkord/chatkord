//! A camera's picture as it came, in memory: a JPEG decoded by FFmpeg's `mjpeg` to its own planes (held by reference,
//! never copied), or YUY2 or NV12 as sent, copied once into a frame of their own. Each is checked against the size
//! settled on before a byte of it is read, and carries its range: a JPEG's is full, a camera's raw picture limited.

use crate::av::{CodecContext, CodecRef, ColourRange, Frame, Packet, PixelFormat};
use capture::camera::{CameraError, Pixel, Shot};

// A shot is read in place, so the zeros a camera keeps after it must cover the ones FFmpeg's readers may touch.
const _: () = assert!(crate::av::INPUT_PADDING <= capture::camera::PADDING);

/// A camera's picture in memory, as it came: its format, size and range are its frame's.
pub struct Decoded {
    frame: Frame,
}

// SAFETY: the frame holds references to buffers FFmpeg counts atomically; it is only read, by one thread at a time.
unsafe impl Send for Decoded {}

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decoded({}x{})", self.width(), self.height())
    }
}

impl Decoded {
    /// Pixels across, even and nonzero.
    pub fn width(&self) -> u32 {
        self.frame.size().0
    }

    /// Pixels down, even and nonzero.
    pub fn height(&self) -> u32 {
        self.frame.size().1
    }

    /// The frame, in memory.
    pub(crate) fn frame(&self) -> &Frame {
        &self.frame
    }
}

/// What reading a camera's pictures keeps: the JPEG decoder, once one is needed.
#[derive(Default)]
pub struct Decoder {
    jpeg: Option<Jpeg>,
}

// SAFETY: the decoder's context and frame are reached only through `&mut self`, by the one thread that reads.
unsafe impl Send for Decoder {}

impl Decoder {
    /// `shot`, a picture of the format and size it says, as a frame in memory; refused as [`CameraError::Format`] when
    /// it is not a whole picture of that size.
    pub fn decode(&mut self, shot: &Shot) -> Result<Decoded, CameraError> {
        let frame = match shot.pixel() {
            Pixel::Mjpeg => {
                let jpeg = match self.jpeg.as_mut() {
                    Some(jpeg) => jpeg,
                    None => self.jpeg.insert(Jpeg::open().ok_or(CameraError::Format)?),
                };
                jpeg.decode(shot)?
            }
            Pixel::Nv12 => packed(shot, PixelFormat::Nv12)?,
            Pixel::Yuy2 => packed(shot, PixelFormat::Yuyv422)?,
        };
        let decoded = Decoded { frame };
        if (decoded.width(), decoded.height()) != shot.size() {
            return Err(CameraError::Format);
        }
        Ok(decoded)
    }
}

/// The rows of `format`'s planes a picture of `height` has, each plane's width in bytes, at `width` pixels.
fn rows(format: PixelFormat, (width, height): (usize, usize)) -> Vec<(usize, usize)> {
    match format {
        PixelFormat::Yuyv422 => vec![(height, width * 2)],
        _ => vec![(height, width), (height / 2, width)],
    }
}

/// `shot`, a camera's `format` picture in rows the camera spaced as it said, copied into a frame of its own; refused
/// when it is shorter than its rows.
fn packed(shot: &Shot, format: PixelFormat) -> Result<Frame, CameraError> {
    let size = shot.size();
    let (width, height) = (size.0 as usize, size.1 as usize);
    let planes = rows(format, (width, height));
    let stride = shot.stride().map_or(planes[0].1, |stride| stride as usize);
    let total: usize = planes.iter().map(|(rows, _)| rows).sum();
    let last = planes.last().map_or(0, |(_, bytes)| *bytes);
    // Every row of every plane is `stride` after the one before, the last only as long as its plane's rows.
    if stride < planes[0].1 || total == 0 || shot.length() < stride * (total - 1) + last {
        return Err(CameraError::Format);
    }
    let mut frame = Frame::software(format, size).map_err(|_| CameraError::Format)?;
    frame.set_colour_range(ColourRange::Limited);
    let picture = shot.picture();
    let mut at = 0;
    for (index, (count, bytes)) in planes.iter().enumerate() {
        let plane = frame.plane_mut(index).ok_or(CameraError::Format)?;
        for row in 0..*count {
            let from = picture.get(at..at + bytes).ok_or(CameraError::Format)?;
            let first = row * plane.stride;
            let to = plane.bytes.get_mut(first..first + bytes).ok_or(CameraError::Format)?;
            to.copy_from_slice(from);
            at += stride;
        }
    }
    Ok(frame)
}

/// FFmpeg's JPEG decoder, opened once for a camera's pictures.
struct Jpeg {
    context: CodecContext,
    packet: Packet,
    frame: Frame,
}

impl Jpeg {
    fn open() -> Option<Self> {
        let decoder = CodecRef::decoder(c"mjpeg")?;
        let mut context = CodecContext::new(Some(decoder))?;
        context.open().ok()?;
        Some(Self { context, packet: Packet::new()?, frame: Frame::new()? })
    }

    /// The picture `shot` holds, decoded into a reference of its own; refused as [`CameraError::Format`] when it is not
    /// one JPEG.
    fn decode(&mut self, shot: &Shot) -> Result<Frame, CameraError> {
        self.frame.unref();
        self.context.send_padded(&mut self.packet, shot.padded(), shot.length()).map_err(|_| CameraError::Format)?;
        self.context.receive_frame(&mut self.frame).map_err(|_| CameraError::Format)?;
        self.frame.reference().ok_or(CameraError::Format)
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
pub(crate) mod fixtures {
    use super::*;

    /// `jpeg`, a 16 by 16 picture, decoded as a camera's would be.
    pub(crate) fn decoded(jpeg: &[u8]) -> Decoded {
        let shot = Shot::new(jpeg, None, Pixel::Mjpeg, (16, 16), None);
        Decoder::default().decode(&shot).expect("a whole JPEG")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shot_shorter_than_its_rows_is_refused_whole() {
        let shot = Shot::new(&[0; 10], None, Pixel::Nv12, (4, 2), None);
        assert_eq!(Decoder::default().decode(&shot).err(), Some(CameraError::Format));
    }
}
