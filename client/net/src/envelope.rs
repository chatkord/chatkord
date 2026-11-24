//! The rates a picture sent may run between, a share's or a camera's, stated once when it starts. The congestion
//! controller begins at a few hundred kilobits and, left to itself, holds a sender that sends little near there and
//! caps a stream at a default far below what a screen needs; so the picture says where its estimate starts, the
//! floor under it and the ceiling above it, and the controller works inside.
//!
//! The numbers are bits per pixel per frame, a row for each size and frame rate. A row's floor is the least a moving
//! scene was sent at for a steady 30 dB, its start 35 dB and its cap 40 dB, each the most any encoder needed in the
//! `quality` sweeps on Machine A (RX 9060 XT) under Fedora 44 and Windows 11 and on Machine B (Renoir) under Fedora 44.
//! The worst is mostly libx264, the software encoder every machine has, so a row is not one GPU's numbers. NVIDIA's,
//! Intel's and older AMD encoders are not measured: a row holds for them only where they need no more than these.

use proto::rules::{CAMERA_MAX_BITS_PER_SECOND, SHARE_MAX_BITS_PER_SECOND};

/// One size's rates at one frame rate, in thousandths of a bit per pixel per frame. `0 < floor <= start <= cap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    floor: u64,
    start: u64,
    cap: u64,
}

/// The rows of a size and of every size above it up to the next, at up to 30 frames a second and above.
struct Size {
    pixels: u64,
    thirty: Row,
    sixty: Row,
}

const fn row(floor: u64, start: u64, cap: u64) -> Row {
    Row { floor, start, cap }
}

/// Smallest first: a size below the first takes its rows.
const SIZES: [Size; 4] = [
    Size { pixels: 1280 * 720, thirty: row(25, 39, 56), sixty: row(25, 38, 50) },
    Size { pixels: 1920 * 1080, thirty: row(28, 40, 47), sixty: row(18, 28, 42) },
    Size { pixels: 2560 * 1440, thirty: row(27, 28, 42), sixty: row(18, 28, 28) },
    Size { pixels: 3840 * 2160, thirty: row(18, 28, 41), sixty: row(14, 28, 28) },
];

/// The row of a picture of `width` by `height` at up to `frames_per_second`: the size at or below its pixels, which
/// asks more of each pixel than the size above.
fn row_for(width: u32, height: u32, frames_per_second: u32) -> Row {
    let pixels = u64::from(width) * u64::from(height);
    let size = SIZES.iter().rfind(|size| size.pixels <= pixels).unwrap_or(&SIZES[0]);
    if frames_per_second <= 30 { size.thirty } else { size.sixty }
}

/// Where a picture's rate may be, in bits per second. `0 < min <= start <= max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// The least the estimate falls to.
    pub min: u64,
    /// Where it starts, before any feedback.
    pub start: u64,
    /// The most it rises to.
    pub max: u64,
}

impl Envelope {
    /// The envelope of a screen of `width` by `height` at up to `frames_per_second`, never past the most a share is
    /// sent at. A `ceiling`, the lower of the person's choice and the server's, lowers all three to fit under it, in
    /// the row's proportion; none is no ceiling.
    pub fn for_screen(width: u32, height: u32, frames_per_second: u32, ceiling: Option<u64>) -> Self {
        Self::for_picture(width, height, frames_per_second, SHARE_MAX_BITS_PER_SECOND, ceiling)
    }

    /// The envelope of a camera of `width` by `height` at up to `frames_per_second`, by a screen's rows, never past the
    /// most a camera is sent at: its own, never taken from a share's. A `ceiling` lowers it as a share's.
    pub fn for_camera(width: u32, height: u32, frames_per_second: u32, ceiling: Option<u64>) -> Self {
        Self::for_picture(width, height, frames_per_second, CAMERA_MAX_BITS_PER_SECOND, ceiling)
    }

    fn for_picture(width: u32, height: u32, frames_per_second: u32, most: u64, ceiling: Option<u64>) -> Self {
        let row = row_for(width, height, frames_per_second);
        // Saturating is exact here: a product that overflows is far past `most`, which holds it.
        let pixels_a_second = (u64::from(width) * u64::from(height)).saturating_mul(u64::from(frames_per_second));
        let max = (pixels_a_second.saturating_mul(row.cap) / 1000).clamp(1, most);
        let max = ceiling.map_or(max, |ceiling| max.min(ceiling.max(1)));
        let share_of_max = |of: u64| (max * of / row.cap).max(1);
        Self { min: share_of_max(row.floor), start: share_of_max(row.start), max }
    }

    /// A rate the engine asks for, kept inside the envelope: its target is a budget, and the share's ceiling holds.
    pub fn clamp(&self, bits_per_second: u64) -> u64 {
        bits_per_second.clamp(self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P720: (u32, u32) = (1280, 720);
    const P1080: (u32, u32) = (1920, 1080);
    const P1440: (u32, u32) = (2560, 1440);
    const P2160: (u32, u32) = (3840, 2160);

    fn row_of((width, height): (u32, u32), frames_per_second: u32) -> Row {
        row_for(width, height, frames_per_second)
    }

    #[test]
    fn every_row_is_ordered_and_never_zero() {
        for size in &SIZES {
            for row in [size.thirty, size.sixty] {
                assert!(0 < row.floor && row.floor <= row.start && row.start <= row.cap, "{row:?}");
            }
        }
        assert!(SIZES.windows(2).all(|pair| pair[0].pixels < pair[1].pixels), "smallest first");
    }

    #[test]
    fn a_size_takes_the_row_at_or_below_its_pixels() {
        assert_eq!(row_of(P720, 30), row(25, 39, 56));
        assert_eq!(row_of((1920, 1079), 60), row_of(P720, 60), "a line short of 1080p is 720p's");
        assert_eq!(row_of(P1080, 60), row(18, 28, 42));
        assert_eq!(row_of((2560, 1439), 30), row_of(P1080, 30));
        assert_eq!(row_of(P1440, 30), row(27, 28, 42));
        assert_eq!(row_of((3840, 2159), 60), row_of(P1440, 60));
        assert_eq!(row_of(P2160, 60), row(14, 28, 28));
    }

    #[test]
    fn past_the_table_a_size_takes_its_nearest_row() {
        assert_eq!(row_of((320, 180), 30), row_of(P720, 30), "below 720p, 720p's");
        assert_eq!(row_of((0, 0), 60), row_of(P720, 60));
        assert_eq!(row_of((7680, 4320), 30), row_of(P2160, 30), "above 2160p, 2160p's");
        assert_eq!(row_of((u32::MAX, u32::MAX), 60), row_of(P2160, 60));
    }

    #[test]
    fn up_to_30_frames_a_second_is_30_s_row_and_above_it_60_s() {
        assert_eq!(row_of(P1080, 30), row(28, 40, 47));
        assert_eq!(row_of(P1080, 1), row_of(P1080, 30));
        assert_eq!(row_of(P1080, 31), row_of(P1080, 60));
        assert_eq!(row_of(P1080, 144), row_of(P1080, 60));
    }

    #[test]
    fn the_shape_of_the_screen_does_not_matter_its_pixels_do() {
        assert_eq!(Envelope::for_screen(1080, 1920, 60, None), Envelope::for_screen(1920, 1080, 60, None));
    }

    #[test]
    fn a_screen_is_given_its_row_s_bits_for_its_pixels_and_rate() {
        let four_k = Envelope::for_screen(3840, 2160, 60, None);
        assert_eq!(four_k, Envelope { min: 6_967_296, start: 13_934_592, max: 13_934_592 });
        let full_hd = Envelope::for_screen(1920, 1080, 30, None);
        assert_eq!(full_hd, Envelope { min: 1_741_824, start: 2_488_320, max: 2_923_776 });
    }

    #[test]
    fn a_screen_past_4k_is_held_to_the_most_a_share_is_sent_at() {
        let most = SHARE_MAX_BITS_PER_SECOND;
        let sixteen_k = Envelope::for_screen(15360, 8640, 60, None);
        assert_eq!(sixteen_k, Envelope { min: most / 2, start: most, max: most }, "held in 2160p60's proportion");
        let past_every_count = Envelope::for_screen(u32::MAX, u32::MAX, u32::MAX, None);
        assert_eq!(past_every_count, sixteen_k, "a count past any product is held, never wrapped");
    }

    #[test]
    fn the_ceiling_lowers_the_whole_envelope_in_its_row_s_proportion_and_never_raises_it() {
        let low = Envelope::for_screen(1920, 1080, 60, Some(4_000_000));
        assert_eq!(low, Envelope { min: 1_714_285, start: 2_666_666, max: 4_000_000 });
        let high = Envelope::for_screen(1920, 1080, 60, Some(1_000_000_000));
        assert_eq!(high, Envelope::for_screen(1920, 1080, 60, None));
    }

    #[test]
    fn a_camera_has_its_own_most_and_a_screen_s_rows_under_it() {
        let hd = Envelope::for_camera(1280, 720, 30, None);
        assert_eq!(hd, Envelope { min: 691_200, start: 1_078_272, max: 1_548_288 });
        let full_hd = Envelope::for_camera(1920, 1080, 30, None);
        assert_eq!(full_hd, Envelope::for_screen(1920, 1080, 30, None), "under its most");
        let four_k = Envelope::for_camera(3840, 2160, 30, None);
        assert_eq!(four_k, Envelope::for_screen(3840, 2160, 30, Some(CAMERA_MAX_BITS_PER_SECOND)), "held to it");
        assert_eq!(Envelope::for_screen(3840, 2160, 30, None).max, 10_202_112, "a screen's most is not a camera's");
    }

    #[test]
    fn a_rate_asked_for_is_kept_inside_the_envelope() {
        let e = Envelope::for_screen(1920, 1080, 60, None);
        assert_eq!((e.min, e.max), (2_239_488, 5_225_472));
        assert_eq!((e.clamp(9_000_000), e.clamp(3_000_000), e.clamp(1)), (e.max, 3_000_000, e.min));
    }

    #[test]
    fn the_envelope_is_ordered_and_never_zero() {
        let cases = [(2, 2, 1, None), (3840, 2160, 60, Some(0)), (1, 1, 1, Some(1)), (0, 0, 0, None)];
        for (width, height, fps, ceiling) in cases {
            let e = Envelope::for_screen(width, height, fps, ceiling);
            assert!(0 < e.min && e.min <= e.start && e.start <= e.max, "{e:?}");
        }
    }
}
