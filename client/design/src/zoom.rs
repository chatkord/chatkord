//! How much larger or smaller the whole window is drawn than the system's own scale: every size,
//! text and picture with it, as Discord's and Teams' zoom. A front end multiplies its scale factor by it.

/// The steps a person zooms through, in percent: Chromium's ladder, which Teams follows.
const LADDER: [u16; 10] = [67, 75, 80, 90, 100, 110, 125, 150, 175, 200];
const UNZOOMED: usize = 4;

/// One step of the ladder; the default draws at the system's scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Zoom(usize);

impl Default for Zoom {
    fn default() -> Self {
        Zoom::DEFAULT
    }
}

impl Zoom {
    /// 100 %: the system's own scale.
    pub const DEFAULT: Zoom = Zoom(UNZOOMED);

    /// Every step, smallest first.
    pub fn all() -> impl Iterator<Item = Zoom> {
        (0..LADDER.len()).map(Zoom)
    }

    /// The step of `percent`, if it is one.
    pub fn of_percent(percent: u16) -> Option<Zoom> {
        LADDER.iter().position(|step| *step == percent).map(Zoom)
    }

    /// The step in percent.
    pub fn percent(self) -> u16 {
        LADDER[self.0]
    }

    /// What the system's scale factor is multiplied by.
    pub fn factor(self) -> f32 {
        f32::from(self.percent()) / 100.0
    }

    /// The next step up; none at the top.
    pub fn zoomed_in(self) -> Option<Zoom> {
        (self.0 + 1 < LADDER.len()).then_some(Zoom(self.0 + 1))
    }

    /// The next step down; none at the bottom.
    pub fn zoomed_out(self) -> Option<Zoom> {
        self.0.checked_sub(1).map(Zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_rises_from_67_to_200_and_starts_at_100() {
        let steps: Vec<u16> = Zoom::all().map(Zoom::percent).collect();
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
        assert_eq!((steps.first(), steps.last()), (Some(&67), Some(&200)));
        assert_eq!(Zoom::default().percent(), 100);
        assert_eq!(Zoom::default().factor(), 1.0);
    }

    #[test]
    fn a_step_goes_up_and_down_one_at_a_time_and_stops_at_the_ends() {
        let mut zoom = Zoom::default();
        while let Some(up) = zoom.zoomed_in() {
            assert_eq!(up.zoomed_out(), Some(zoom));
            zoom = up;
        }
        assert_eq!(zoom.percent(), 200);
        while let Some(down) = zoom.zoomed_out() {
            zoom = down;
        }
        assert_eq!(zoom.percent(), 67);
    }

    #[test]
    fn only_a_step_of_the_ladder_is_read_from_a_percent() {
        assert_eq!(Zoom::of_percent(150).map(Zoom::percent), Some(150));
        assert_eq!(Zoom::of_percent(100), Some(Zoom::DEFAULT));
        assert_eq!(Zoom::of_percent(999), None);
        assert_eq!(Zoom::of_percent(0), None);
    }
}
