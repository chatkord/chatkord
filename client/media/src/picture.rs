//! A decoded picture in memory as its decoder or its download left it, NV12 or three planes of 8-bit 4:2:0, held by
//! reference until the window has uploaded it, so nothing is converted, copied or split on the way.

use crate::av::{Frame, PixelFormat, Plane};
use crate::colours::Colours;
use crate::size::fits;

/// How a picture's planes lie, which picks the textures and the shader that paint it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureLayout {
    /// A luma plane, then one of blue and red interleaved, half the width and height.
    Nv12,
    /// Luma, blue and red planes, the chroma half the width and height.
    I420,
}

impl PictureLayout {
    fn of(format: PixelFormat) -> Option<Self> {
        match format {
            PixelFormat::Nv12 => Some(Self::Nv12),
            PixelFormat::Yuv420p => Some(Self::I420),
            _ => None,
        }
    }

    const fn planes(self) -> usize {
        match self {
            Self::Nv12 => 2,
            Self::I420 => 3,
        }
    }
}

/// A picture's planes as they lie in memory, each with its bytes from one row to the next.
#[derive(Clone, Copy, Debug)]
pub enum Samples<'a> {
    /// Luma, then blue and red interleaved.
    Nv12([(&'a [u8], u32); 2]),
    /// Luma, blue, red.
    I420([(&'a [u8], u32); 3]),
}

impl<'a> Samples<'a> {
    /// The layout they are in.
    pub const fn layout(&self) -> PictureLayout {
        match self {
            Self::Nv12(_) => PictureLayout::Nv12,
            Self::I420(_) => PictureLayout::I420,
        }
    }

    /// The luma plane and its stride.
    pub const fn luma(&self) -> (&'a [u8], u32) {
        match self {
            Self::Nv12([luma, _]) | Self::I420([luma, _, _]) => *luma,
        }
    }
}

/// A decoded picture in memory, the decoder's or its download's own buffers; dropping it lets them be written again.
pub struct Picture {
    frame: Frame,
    layout: PictureLayout,
    width: u32,
    height: u32,
    colours: Colours,
}

// SAFETY: the frame is a reference of our own to buffers FFmpeg counts atomically; it is only read, from one thread at
// a time, after the decoder handed it out.
unsafe impl Send for Picture {}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({:?} {}x{})", self.layout, self.width, self.height)
    }
}

impl Picture {
    /// A new reference to `frame`, in the matrix and range it says, when it is NV12 or 8-bit 4:2:0 in memory of an even
    /// size a picture may have; none for any other, which is converted first.
    pub(crate) fn of(frame: &Frame) -> Option<Self> {
        Self::in_colours(frame, Colours::of(frame))
    }

    /// The same, its samples in `colours` whatever the frame says.
    pub(crate) fn in_colours(frame: &Frame, colours: Colours) -> Option<Self> {
        let layout = frame.format().and_then(PictureLayout::of).filter(|_| !frame.is_hardware())?;
        let (width, height) = frame.size();
        let even = width.is_multiple_of(2) && height.is_multiple_of(2);
        let whole = (0..layout.planes()).all(|plane| frame.plane(plane).is_some());
        if !even || !whole || !fits(width, height) {
            return None;
        }
        Some(Self { frame: frame.reference()?, layout, width, height, colours })
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// How its planes lie.
    pub fn layout(&self) -> PictureLayout {
        self.layout
    }

    /// The matrix and range its samples are in.
    pub fn colours(&self) -> Colours {
        self.colours
    }

    /// Its planes, the buffers it holds, with their strides.
    pub fn samples(&self) -> Samples<'_> {
        let plane = |index| {
            // Checked in `in_colours`: every plane of its layout is there.
            let Plane { bytes, stride } = self.frame.plane(index).unwrap_or(Plane { bytes: &[], stride: 0 });
            (bytes, u32::try_from(stride).unwrap_or(0))
        };
        match self.layout {
            PictureLayout::Nv12 => Samples::Nv12([plane(0), plane(1)]),
            PictureLayout::I420 => Samples::I420([plane(0), plane(1), plane(2)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::av::{ColourRange, ColourSpace};

    /// A blank frame of `format` and `width` × `height`, its buffers FFmpeg's own.
    fn frame(format: PixelFormat, width: u32, height: u32) -> Frame {
        Frame::software(format, (width, height)).expect("a frame")
    }

    fn planes(samples: Samples<'_>) -> Vec<(&[u8], u32)> {
        match samples {
            Samples::Nv12(planes) => planes.to_vec(),
            Samples::I420(planes) => planes.to_vec(),
        }
    }

    #[test]
    fn the_planes_are_the_frame_s_own_bytes_not_a_copy_in_either_layout() {
        for (format, layout) in [(PixelFormat::Yuv420p, PictureLayout::I420), (PixelFormat::Nv12, PictureLayout::Nv12)]
        {
            let decoded = frame(format, 64, 32);
            let picture = Picture::of(&decoded).expect("4:2:0 in memory");
            assert_eq!((picture.layout(), picture.samples().layout()), (layout, layout));
            let samples = planes(picture.samples());
            assert_eq!(samples.len(), layout.planes());
            for (index, (bytes, stride)) in samples.into_iter().enumerate() {
                let own = decoded.plane(index).expect("a plane");
                assert_eq!(bytes.as_ptr(), own.bytes.as_ptr(), "{format:?} plane {index} is the frame's buffer");
                assert_eq!(stride as usize, own.stride);
            }
            assert_eq!(picture.size(), (64, 32));
        }
    }

    #[test]
    fn another_layout_or_an_odd_size_is_not_taken_as_it_is() {
        assert!(Picture::of(&frame(PixelFormat::Yuv422p, 64, 32)).is_none());
        assert!(Picture::of(&frame(PixelFormat::Yuvj420p, 64, 32)).is_none(), "full range spelt is converted");
        assert!(Picture::of(&frame(PixelFormat::Yuv420p, 63, 32)).is_none());
        assert!(Picture::of(&frame(PixelFormat::Nv12, 64, 31)).is_none());
    }

    #[test]
    fn the_planes_carry_the_matrix_and_range_the_frame_says() {
        let mut tagged = frame(PixelFormat::Yuv420p, 64, 32);
        assert_eq!(Picture::of(&tagged).expect("planes").colours(), Colours::HD, "a frame that says nothing");
        tagged.set_colour_range(ColourRange::Full);
        tagged.set_colour_space(ColourSpace::Smpte170m);
        let colours = Picture::of(&tagged).expect("full range is painted as it is, not refused").colours();
        assert_eq!(colours, Colours { matrix: crate::Matrix::Bt601, range: crate::Range::Full });
    }
}
