//! A picture: checked before it is sent by the bounds the server's intake keeps, and
//! read back after it is fetched (the square PNG the server made, an emoji, a thumbnail or a picture sent) within the
//! bounds of its kind, decoded to RGBA for a front end to paint. All are CPU-bound: run them on the blocking pool.
#![forbid(unsafe_code)]

use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use proto::files::{EMOJI_SIDE, FileKind, FileRefusal, PICTURE_DECODE_BYTES, PICTURE_SIDE, PICTURE_STORED_BYTES};
use proto::files::{PICTURE_UPLOAD_SIDE, PictureType, THUMBNAIL_BOX};
use std::io::Cursor;
use terms::FileError;

/// A kept picture's pixels: [`PICTURE_SIDE`] on a side, RGBA, row by row.
#[derive(Clone, PartialEq, Eq)]
pub struct Square {
    rgba: Vec<u8>,
}

impl Square {
    /// Its width and height, in pixels.
    pub const SIDE: u32 = PICTURE_SIDE;

    /// Four bytes a pixel, `SIDE * SIDE` pixels.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The same, given up.
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

impl std::fmt::Debug for Square {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Square({side}x{side})", side = Self::SIDE)
    }
}

/// What was fetched or cached is not a picture this server kept: a caller shows the initial, or fetches again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotKept;

/// The decoder's allocation ceiling, as the server's: the largest picture taken, as RGBA, twice over.
const SENT_ALLOC: u64 = 2 * 4 * PICTURE_UPLOAD_SIDE as u64 * PICTURE_UPLOAD_SIDE as u64;

/// Whether `bytes` would be taken as a picture of `kind` (a server's or a member's, which share one intake): within
/// its size, PNG, JPEG or WebP by its first bytes, at most [`PICTURE_UPLOAD_SIDE`] on a side, and readable whole;
/// refused with the server's own refusal, before it is sent.
pub fn before_upload(bytes: &[u8], kind: FileKind) -> Result<(), FileError> {
    let refused = |refusal| FileError::Refused { kind, refusal };
    if u64::try_from(bytes.len()).map_or(true, |len| len > kind.upload_bytes()) {
        return Err(refused(FileRefusal::TooLarge));
    }
    let format = match PictureType::sniff(bytes).ok_or(refused(FileRefusal::UnsupportedType))? {
        PictureType::Png => ImageFormat::Png,
        PictureType::Jpeg => ImageFormat::Jpeg,
        PictureType::WebP => ImageFormat::WebP,
    };
    let unreadable = |failed: image::ImageError| match failed {
        image::ImageError::Limits(_) => refused(FileRefusal::Unprocessable),
        _ => FileError::Unreadable { kind },
    };
    let decoder = reader(bytes, format, PICTURE_UPLOAD_SIDE, SENT_ALLOC).into_decoder().map_err(unreadable)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > PICTURE_UPLOAD_SIDE || height > PICTURE_UPLOAD_SIDE {
        return Err(refused(FileRefusal::Unprocessable));
    }
    image::DynamicImage::from_decoder(decoder).map(drop).map_err(unreadable)
}

/// A kept picture's pixels: a PNG of at most [`PICTURE_STORED_BYTES`], exactly [`PICTURE_SIDE`] on a side, as the
/// server keeps a server's and a member's alike.
pub fn after_fetch(bytes: &[u8]) -> Result<Square, NotKept> {
    if u64::try_from(bytes.len()).map_or(true, |len| len > PICTURE_STORED_BYTES) {
        return Err(NotKept);
    }
    let kept_alloc = 2 * 4 * u64::from(PICTURE_SIDE) * u64::from(PICTURE_SIDE);
    let decoder = reader(bytes, ImageFormat::Png, PICTURE_SIDE, kept_alloc).into_decoder().map_err(|_| NotKept)?;
    if decoder.dimensions() != (PICTURE_SIDE, PICTURE_SIDE) {
        return Err(NotKept);
    }
    let picture = image::DynamicImage::from_decoder(decoder).map_err(|_| NotKept)?;
    Ok(Square { rgba: picture.into_rgba8().into_raw() })
}

/// The longest side a fetched picture is held at: larger, it is scaled down on its read, in its own proportions.
pub const SHOWN_SIDE: u32 = 2048;

/// A fetched picture's pixels, RGBA, row by row.
#[derive(Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Four bytes a pixel, `width * height` pixels.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decoded({}x{})", self.width, self.height)
    }
}

/// A fetched file of `kind` read as a picture: PNG, JPEG or WebP by its first bytes, within its kind's stored bytes,
/// no side past its kind's (an emoji's, a thumbnail's box) and at most [`PICTURE_DECODE_BYTES`] held to decode it,
/// as the server's intake took it.
pub fn shown(bytes: &[u8], kind: FileKind) -> Result<Decoded, NotKept> {
    within(bytes, kind, PICTURE_DECODE_BYTES)
}

fn within(bytes: &[u8], kind: FileKind, budget: u64) -> Result<Decoded, NotKept> {
    if u64::try_from(bytes.len()).map_or(true, |len| len > kind.stored_bytes()) {
        return Err(NotKept);
    }
    let side = match kind {
        FileKind::ServerPicture | FileKind::ProfilePicture => PICTURE_SIDE,
        FileKind::Emoji => EMOJI_SIDE,
        FileKind::Thumbnail => THUMBNAIL_BOX.0.max(THUMBNAIL_BOX.1),
        FileKind::Attachment => u32::MAX,
    };
    let format = match PictureType::sniff(bytes).ok_or(NotKept)? {
        PictureType::Png => ImageFormat::Png,
        PictureType::Jpeg => ImageFormat::Jpeg,
        PictureType::WebP => ImageFormat::WebP,
    };
    let mut decoder = reader(bytes, format, side, budget).into_decoder().map_err(|_| NotKept)?;
    let (width, height) = decoder.dimensions();
    // A photo keeps its orientation tag through the server's intake; it is drawn the way it was taken.
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    // The decode and its RGBA copy are held at once, and a second copy to turn it: all are counted.
    let rgba = u64::from(width) * u64::from(height) * 4;
    let copies = if orientation == image::metadata::Orientation::NoTransforms { 1 } else { 2 };
    if decoder.total_bytes().saturating_add(rgba.saturating_mul(copies)) > budget {
        return Err(NotKept);
    }
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(|_| NotKept)?;
    picture.apply_orientation(orientation);
    // Shown no larger than a window shows it, and as one page of the painter's atlas.
    let picture = if width.max(height) > SHOWN_SIDE {
        picture.resize(SHOWN_SIDE, SHOWN_SIDE, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let (width, height) = (picture.width(), picture.height());
    Ok(Decoded { width, height, rgba: picture.into_rgba8().into_raw() })
}

fn reader(bytes: &[u8], format: ImageFormat, side: u32, alloc: u64) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(side);
    limits.max_image_height = Some(side);
    limits.max_alloc = Some(alloc);
    reader.limits(limits);
    reader
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use proto::files::PICTURE_UPLOAD_BYTES;

    fn encoded(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let picture = RgbaImage::from_pixel(width, height, Rgba([200, 40, 90, 255]));
        let mut bytes = Cursor::new(Vec::new());
        match format {
            ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(picture).to_rgb8().write_to(&mut bytes, format),
            _ => picture.write_to(&mut bytes, format),
        }
        .unwrap();
        bytes.into_inner()
    }

    const PICTURES: [FileKind; 2] = [FileKind::ServerPicture, FileKind::ProfilePicture];

    fn refusal(kind: FileKind, refusal: FileRefusal) -> Result<(), FileError> {
        Err(FileError::Refused { kind, refusal })
    }

    #[test]
    fn png_jpeg_and_webp_within_the_bounds_are_taken() {
        for kind in PICTURES {
            for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
                assert_eq!(before_upload(&encoded(64, 32, format), kind), Ok(()), "{kind:?} {format:?}");
            }
            let side = encoded(PICTURE_UPLOAD_SIDE, 1, ImageFormat::Png);
            assert_eq!(before_upload(&side, kind), Ok(()), "{kind:?}: the side itself");
        }
    }

    #[test]
    fn a_picture_over_the_size_is_refused_unread() {
        let mut over = encoded(8, 8, ImageFormat::Png);
        over.resize(usize::try_from(PICTURE_UPLOAD_BYTES).unwrap() + 1, 0);
        for kind in PICTURES {
            assert_eq!(before_upload(&over, kind), refusal(kind, FileRefusal::TooLarge), "{kind:?}");
        }
    }

    #[test]
    fn a_type_is_judged_by_its_first_bytes() {
        for kind in PICTURES {
            let gif = b"GIF89a\x01\x00\x01\x00";
            assert_eq!(before_upload(gif, kind), refusal(kind, FileRefusal::UnsupportedType), "{kind:?}");
            assert_eq!(before_upload(b"", kind), refusal(kind, FileRefusal::UnsupportedType), "{kind:?}");
        }
    }

    #[test]
    fn a_picture_over_the_side_is_refused() {
        for kind in PICTURES {
            let wide = encoded(PICTURE_UPLOAD_SIDE + 1, 1, ImageFormat::Png);
            assert_eq!(before_upload(&wide, kind), refusal(kind, FileRefusal::Unprocessable), "{kind:?}");
            let tall = encoded(1, PICTURE_UPLOAD_SIDE + 1, ImageFormat::WebP);
            assert_eq!(before_upload(&tall, kind), refusal(kind, FileRefusal::Unprocessable), "{kind:?}");
        }
    }

    #[test]
    fn a_picture_that_cannot_be_read_says_so() {
        let mut cut = encoded(64, 64, ImageFormat::Png);
        cut.truncate(cut.len() / 2);
        for kind in PICTURES {
            assert_eq!(before_upload(&cut, kind), Err(FileError::Unreadable { kind }), "{kind:?}");
        }
    }

    #[test]
    fn a_kept_picture_is_its_square_of_rgba() {
        let square = after_fetch(&encoded(PICTURE_SIDE, PICTURE_SIDE, ImageFormat::Png)).expect("kept");
        let side = usize::try_from(PICTURE_SIDE).unwrap();
        assert_eq!(square.rgba().len(), side * side * 4);
        assert_eq!(&square.rgba()[..4], &[200, 40, 90, 255]);
    }

    #[test]
    fn anything_but_the_square_png_the_server_makes_is_not_kept() {
        assert_eq!(after_fetch(&encoded(PICTURE_SIDE - 1, PICTURE_SIDE, ImageFormat::Png)), Err(NotKept));
        assert_eq!(after_fetch(&encoded(PICTURE_SIDE, PICTURE_SIDE, ImageFormat::Jpeg)), Err(NotKept));
        assert_eq!(after_fetch(&encoded(PICTURE_SIDE * 2, PICTURE_SIDE * 2, ImageFormat::Png)), Err(NotKept));
        let mut over = encoded(PICTURE_SIDE, PICTURE_SIDE, ImageFormat::Png);
        over.resize(usize::try_from(PICTURE_STORED_BYTES).unwrap() + 1, 0);
        assert_eq!(after_fetch(&over), Err(NotKept));
        let mut cut = encoded(PICTURE_SIDE, PICTURE_SIDE, ImageFormat::Png);
        cut.truncate(cut.len() / 2);
        assert_eq!(after_fetch(&cut), Err(NotKept));
    }

    #[test]
    fn a_fetched_picture_is_read_within_its_kinds_side_and_whole() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let read = shown(&encoded(800, 600, format), FileKind::Thumbnail).unwrap();
            assert_eq!((read.width, read.height, read.rgba.len()), (800, 600, 800 * 600 * 4), "{format:?}");
        }
        assert_eq!(shown(&encoded(EMOJI_SIDE + 1, 1, ImageFormat::Png), FileKind::Emoji), Err(NotKept));
        assert_eq!(shown(&encoded(801, 1, ImageFormat::Png), FileKind::Thumbnail), Err(NotKept));
        assert!(shown(&encoded(PICTURE_SIDE, PICTURE_SIDE, ImageFormat::Png), FileKind::ProfilePicture).is_ok());
        assert_eq!(shown(&encoded(PICTURE_SIDE + 1, 1, ImageFormat::Png), FileKind::ProfilePicture), Err(NotKept));
        assert_eq!(shown(b"not a picture at all", FileKind::Attachment), Err(NotKept));
        let large = shown(&encoded(4032, 3024, ImageFormat::Jpeg), FileKind::Attachment).unwrap();
        assert_eq!((large.width, large.height, large.rgba.len()), (SHOWN_SIDE, 1536, 2048 * 1536 * 4), "scaled down");
    }

    /// `jpeg` with an Exif segment saying `orientation`, as a phone writes one.
    fn oriented(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let [low, high] = orientation.to_le_bytes();
        let tiff = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, // little-endian, the first directory at 8
            1, 0, // one entry
            0x12, 0x01, 3, 0, 1, 0, 0, 0, low, high, 0, 0, // Orientation, SHORT, one value
            0, 0, 0, 0, // no next directory
        ];
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend(tiff);
        let length = u16::try_from(segment.len() + 2).unwrap().to_be_bytes();
        let mut out = jpeg[..2].to_vec();
        out.extend([0xFF, 0xE1, length[0], length[1]]);
        out.extend(segment);
        out.extend(&jpeg[2..]);
        out
    }

    #[test]
    fn a_photo_is_turned_the_way_its_orientation_says() {
        let taken = oriented(&encoded(60, 40, ImageFormat::Jpeg), 6);
        let read = shown(&taken, FileKind::Attachment).unwrap();
        assert_eq!((read.width, read.height), (40, 60), "turned a quarter, as the camera held it");
        let upright = shown(&oriented(&encoded(60, 40, ImageFormat::Jpeg), 1), FileKind::Attachment).unwrap();
        assert_eq!((upright.width, upright.height), (60, 40));
    }

    #[test]
    fn a_picture_whose_decode_and_copy_pass_the_budget_is_refused() {
        let picture = encoded(64, 64, ImageFormat::Jpeg);
        let held = 64 * 64 * 3 + 64 * 64 * 4;
        assert!(within(&picture, FileKind::Attachment, held).is_ok());
        assert_eq!(within(&picture, FileKind::Attachment, held - 1), Err(NotKept));
    }
}
