//! Where a picture sits in the space it is given, in whole pixels: plain arithmetic, so it is decided here and
//! checked without a window.

/// A rectangle in whole pixels: where the picture goes inside an area, with the area's own origin at (0, 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// From the area's left edge.
    pub left: u32,
    /// From the area's top edge.
    pub top: u32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

/// The largest copy of a `picture` that fits in `area` with its shape kept, centred. Nothing fits in
/// an area with no size, or a picture with none.
pub fn fit((picture_width, picture_height): (u32, u32), (area_width, area_height): (u32, u32)) -> Option<Placed> {
    if picture_width == 0 || picture_height == 0 || area_width == 0 || area_height == 0 {
        return None;
    }
    // Compare the two ratios by cross-multiplying, in a wider integer, so no rounding picks the side.
    let wider_than_area =
        u64::from(picture_width) * u64::from(area_height) > u64::from(area_width) * u64::from(picture_height);
    let (width, height) = if wider_than_area {
        let height = u64::from(area_width) * u64::from(picture_height) / u64::from(picture_width);
        (area_width, u32::try_from(height.max(1)).ok()?)
    } else {
        let width = u64::from(area_height) * u64::from(picture_width) / u64::from(picture_height);
        (u32::try_from(width.max(1)).ok()?, area_height)
    };
    Some(Placed { left: (area_width - width) / 2, top: (area_height - height) / 2, width, height })
}

/// A length on screen as a whole number of pixels: none under zero, and the largest past `u32::MAX`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a length on screen: `as` saturates, and a negative or NaN one is none"
)]
pub fn whole_pixels(length: f32) -> u32 {
    length as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_the_shape_of_the_area_fills_it() {
        assert_eq!(fit((1920, 1080), (960, 540)), Some(Placed { left: 0, top: 0, width: 960, height: 540 }));
    }

    #[test]
    fn a_wide_picture_in_a_tall_area_has_bars_above_and_below() {
        assert_eq!(fit((1600, 900), (800, 800)), Some(Placed { left: 0, top: 175, width: 800, height: 450 }));
    }

    #[test]
    fn a_tall_picture_in_a_wide_area_has_bars_at_the_sides() {
        assert_eq!(fit((900, 1600), (800, 800)), Some(Placed { left: 175, top: 0, width: 450, height: 800 }));
    }

    #[test]
    fn a_picture_is_scaled_up_to_a_bigger_area_too() {
        assert_eq!(fit((4, 3), (400, 400)), Some(Placed { left: 0, top: 50, width: 400, height: 300 }));
    }

    #[test]
    fn a_sliver_of_an_area_still_gets_a_pixel() {
        assert_eq!(fit((1000, 10), (10, 10)), Some(Placed { left: 0, top: 4, width: 10, height: 1 }));
    }

    #[test]
    fn nothing_fits_where_there_is_no_size() {
        assert_eq!(fit((0, 10), (10, 10)), None);
        assert_eq!(fit((10, 0), (10, 10)), None);
        assert_eq!(fit((10, 10), (0, 10)), None);
        assert_eq!(fit((10, 10), (10, 0)), None);
    }

    #[test]
    fn the_largest_sizes_do_not_overflow() {
        let placed = fit((u32::MAX, u32::MAX), (u32::MAX, 10)).expect("a square fits");
        assert_eq!((placed.width, placed.height), (10, 10));
    }
}
