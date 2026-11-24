//! The largest picture a received stream may decode to, so a sharer cannot hand a watcher a picture no GPU here can
//! hold.

/// 8K UHD, the largest a screen is shared at.
pub(crate) const MAX_PICTURE: (u32, u32) = (7680, 4320);

/// The most pixels a picture of [`MAX_PICTURE`] has: a decoder is opened to refuse a stream that asks for more.
pub(crate) const MAX_PIXELS: i64 = MAX_PICTURE.0 as i64 * MAX_PICTURE.1 as i64;

/// Whether a decoded picture of this size is shown: neither side zero or beyond [`MAX_PICTURE`].
pub(crate) fn fits(width: u32, height: u32) -> bool {
    (1..=MAX_PICTURE.0).contains(&width) && (1..=MAX_PICTURE.1).contains(&height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_beyond_8k_or_with_no_area_is_refused() {
        for (width, height) in [(3840, 2160), (7680, 4320), (1920, 1081), (2, 2)] {
            assert!(fits(width, height), "{width}x{height}");
        }
        for (width, height) in [(9000, 500), (8192, 8192), (7681, 4320), (7680, 4321), (0, 1080), (1920, 0)] {
            assert!(!fits(width, height), "{width}x{height}");
        }
    }
}
