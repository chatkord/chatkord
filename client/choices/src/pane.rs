//! How wide the server pane is kept on this device, as its splitter left it.

use design::{PANE_LEAST, PANE_MOST};

/// The pane's width as this device keeps it: whole pixels, never outside [`PANE_LEAST`] to [`PANE_MOST`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneWidth(u16);

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "the tokens are whole and small")]
impl PaneWidth {
    /// As it starts, and after a reset: its least.
    pub const LEAST: PaneWidth = PaneWidth(PANE_LEAST as u16);
    /// The widest it is kept.
    pub const MOST: PaneWidth = PaneWidth(PANE_MOST as u16);

    /// `px` wide, rounded to a whole pixel and held within its range; a value that is not a number is its least.
    pub fn new(px: f32) -> PaneWidth {
        if px.is_nan() {
            return PaneWidth::LEAST;
        }
        let whole = px.round().clamp(PANE_LEAST, PANE_MOST) as u16;
        PaneWidth(whole)
    }

    /// In logical pixels.
    pub fn px(self) -> f32 {
        f32::from(self.0)
    }

    /// In whole pixels, as it is kept.
    pub fn whole(self) -> u16 {
        self.0
    }
}

impl Default for PaneWidth {
    fn default() -> PaneWidth {
        PaneWidth::LEAST
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_width_is_whole_and_within_its_range() {
        assert_eq!(PaneWidth::new(100.0), PaneWidth::LEAST);
        assert_eq!(PaneWidth::new(9000.0), PaneWidth::MOST);
        assert_eq!(PaneWidth::new(f32::NAN), PaneWidth::LEAST);
        assert_eq!(PaneWidth::new(300.4).whole(), 300);
        assert_eq!(PaneWidth::default(), PaneWidth::LEAST);
    }
}
