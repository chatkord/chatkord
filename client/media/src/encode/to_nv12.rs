//! A camera's picture made the packed NV12 the encoder's upload and one's own card take, on the processor: the route
//! where no device lifts it. FFmpeg's converter maps a JPEG's full range to limited range as it goes and leaves the
//! matrix, which it changes only to or from RGB: the picture stays a camera's BT.601, and says so.

use crate::av::{Conversion, View, ViewMut};
use crate::colours::{Colours, Matrix, Range};
use crate::decode::mjpeg::Decoded;
use crate::encode::Nv12;
use crate::planes::Planes;
use capture::camera::CameraError;

/// What the pictures this makes carry.
pub(crate) const COLOURS: Colours = Colours { matrix: Matrix::Bt601, range: Range::Limited };

/// What making NV12 keeps from one picture to the next: the converter.
#[derive(Default)]
pub(crate) struct ToNv12 {
    planes: Planes,
}

// SAFETY: the converter owns its scaler and shares none; FFmpeg lets one move between threads while one thread uses it at
// a time, which `&mut self` ensures.
unsafe impl Send for ToNv12 {}

impl ToNv12 {
    /// `decoded` in packed NV12 of its size.
    pub(crate) fn nv12(&mut self, decoded: &Decoded) -> Result<Nv12, CameraError> {
        self.nv12_at(decoded, (decoded.width(), decoded.height()))
    }

    /// `decoded` in packed NV12 scaled to `size`, each side even, in the one pass that converts it.
    pub(crate) fn nv12_at(&mut self, decoded: &Decoded, size: (u32, u32)) -> Result<Nv12, CameraError> {
        let (width, height) = (size.0 as usize, size.1 as usize);
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(CameraError::Format);
        }
        let mut out = vec![0u8; width * height * 3 / 2];
        let (top, bottom) = out.split_at_mut(width * height);
        let from = View::of_frame(decoded.frame()).ok_or(CameraError::Format)?;
        let mut to = ViewMut::nv12(size, top, bottom).ok_or(CameraError::Format)?;
        self.planes.scaler.scale(&from, &mut to, Conversion::Colour).map_err(|_| CameraError::Format)?;
        Nv12::new(width, height, out).map(|picture| picture.in_colours(COLOURS)).map_err(|_| CameraError::Format)
    }
}

impl ToNv12 {
    /// `nv12`, a camera's own NV12, scaled to `size`, each side even.
    pub(crate) fn scaled(&mut self, nv12: &Nv12, size: (u32, u32)) -> Result<Nv12, CameraError> {
        let (width, height) = (size.0 as usize, size.1 as usize);
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(CameraError::Format);
        }
        let to_u32 = |value: usize| u32::try_from(value).map_err(|_| CameraError::Format);
        let own = (to_u32(nv12.width())?, to_u32(nv12.height())?);
        let from = View::nv12(own, nv12.luma(), nv12.chroma()).ok_or(CameraError::Format)?;
        let mut out = vec![0u8; width * height * 3 / 2];
        let (top, bottom) = out.split_at_mut(width * height);
        let mut to = ViewMut::nv12(size, top, bottom).ok_or(CameraError::Format)?;
        self.planes.scaler.scale(&from, &mut to, Conversion::Colour).map_err(|_| CameraError::Format)?;
        Nv12::new(width, height, out).map(|picture| picture.in_colours(nv12.colours())).map_err(|_| CameraError::Format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::mjpeg::Decoder;
    use capture::camera::{Pixel, Shot};

    /// `bytes`, a picture the camera settled as `pixel` of `size`, in NV12.
    fn nv12(bytes: &[u8], stride: Option<u32>, pixel: Pixel, size: (u32, u32)) -> Result<Nv12, CameraError> {
        let decoded = Decoder::default().decode(&Shot::new(bytes, stride, pixel, size, None))?;
        ToNv12::default().nv12(&decoded)
    }

    #[test]
    fn nv12_rows_past_the_picture_s_width_are_left_out() {
        // 4 by 2: two luma rows and a chroma row, each padded to 6 bytes.
        let padded = [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0, 9, 10, 11, 12];
        let picture = nv12(&padded, Some(6), Pixel::Nv12, (4, 2)).unwrap();
        assert_eq!(picture.luma(), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(picture.chroma(), [9, 10, 11, 12]);
    }

    #[test]
    fn a_picture_shorter_than_its_size_is_refused_whole() {
        for pixel in [Pixel::Nv12, Pixel::Yuy2] {
            assert_eq!(nv12(&[0; 10], None, pixel, (4, 2)).err(), Some(CameraError::Format), "{pixel:?}");
        }
    }

    #[test]
    fn yuy2_becomes_nv12_of_its_brightness_and_colour() {
        // Two rows of mid grey, then black: Y U Y V each pair of pixels.
        let (width, height) = (16usize, 4usize);
        let mut yuy2 = Vec::new();
        for row in 0..height {
            let luma = if row < 2 { 128 } else { 16 };
            for _ in 0..width / 2 {
                yuy2.extend_from_slice(&[luma, 128, luma, 128]);
            }
        }
        let picture = nv12(&yuy2, None, Pixel::Yuy2, (16, 4)).unwrap();
        let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
        assert!(picture.luma()[..width * 2].iter().all(|y| near(*y, 128)), "{:?}", picture.luma());
        assert!(picture.luma()[width * 2..].iter().all(|y| near(*y, 16)), "{:?}", picture.luma());
        assert!(picture.chroma().iter().all(|c| near(*c, 128)), "neutral colour: {:?}", picture.chroma());
    }

    #[test]
    fn a_camera_s_red_keeps_its_bt601_samples_and_is_said_to_be_bt601_in_limited_range() {
        // BT.601's limited-range red as Y, U and V; made BT.709 it would be 63, 102 and 240.
        let (width, height) = (16usize, 4usize);
        let yuy2: Vec<u8> = (0..width * height / 2).flat_map(|_| [81, 90, 81, 240]).collect();
        let picture = nv12(&yuy2, None, Pixel::Yuy2, (16, 4)).unwrap();
        let near = |a: u8, b: u8| a.abs_diff(b) <= 1;
        assert!(picture.luma().iter().all(|y| near(*y, 81)), "{:?}", picture.luma());
        assert!(picture.chroma().chunks(2).all(|c| near(c[0], 90) && near(c[1], 240)), "{:?}", picture.chroma());
        assert_eq!(picture.colours(), Colours { matrix: Matrix::Bt601, range: Range::Limited });
    }

    #[test]
    fn a_picture_is_scaled_down_as_it_is_made_nv12_and_an_odd_size_is_refused() {
        let (width, height) = (16usize, 8usize);
        let mut yuy2 = Vec::new();
        for _ in 0..height {
            for _ in 0..width / 2 {
                yuy2.extend_from_slice(&[128, 128, 128, 128]);
            }
        }
        let decoded =
            Decoder::default().decode(&Shot::new(&yuy2, None, Pixel::Yuy2, (16, 8), None)).expect("a whole picture");
        let mut to = ToNv12::default();
        let small = to.nv12_at(&decoded, (8, 4)).expect("scaled");
        assert_eq!((small.width(), small.height()), (8, 4));
        assert_eq!(small.luma().len(), 8 * 4);
        assert!(small.luma().iter().all(|y| y.abs_diff(128) <= 2), "{:?}", small.luma());
        assert_eq!(to.nv12_at(&decoded, (7, 4)).err(), Some(CameraError::Format), "an odd side");
    }

    // Windows' FFmpeg has no MJPEG decoder: Media Foundation hands its cameras' pictures over as NV12.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_jpeg_is_decoded_to_nv12_and_one_of_another_size_or_none_is_refused() {
        use capture::camera::fixtures::GREY;
        let picture = nv12(GREY, None, Pixel::Mjpeg, (16, 16)).expect("a JPEG of its size");
        assert_eq!((picture.width(), picture.height()), (16, 16));
        let near = |a: u8, b: u8| a.abs_diff(b) <= 3;
        // Full-range grey 128 is limited-range 126: the converter maps a JPEG's range to the encoder's.
        assert!(picture.luma().iter().all(|y| near(*y, 126)), "{:?}", picture.luma());
        assert!(picture.chroma().iter().all(|c| near(*c, 128)), "{:?}", picture.chroma());
        let wrong = nv12(GREY, None, Pixel::Mjpeg, (32, 16));
        assert_eq!(wrong.err(), Some(CameraError::Format), "a picture of another size");
        let none = nv12(&[0xFF, 0xD8, 0, 1], None, Pixel::Mjpeg, (16, 16));
        assert_eq!(none.err(), Some(CameraError::Format), "not a whole JPEG");
    }
}
