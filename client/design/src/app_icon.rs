//! The icon a desktop shows for Chatkord: the brand pane's mark, in the dark ground's colour, on a rounded square of
//! Chatkord's own accent. One icon for both schemes, because a desktop does not repaint an app's icon with them.

use crate::accent::Accent;
use crate::color::Rgb;
use crate::palette::Palette;
use crate::role::{Role, Surface};
use crate::scheme::Scheme;

/// The icon's parts, in the unit of its side (a side of 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppIcon {
    /// The rounded square's colour.
    pub tile: Rgb,
    /// The bars' colour on it.
    pub mark: Rgb,
    /// The tile's corner radius.
    pub corner: f32,
    /// The side of the square the mark's own 24-unit box is drawn in, centred on the tile.
    pub mark_box: f32,
    /// The bars' stroke, in the mark's own units of 24.
    pub stroke: f32,
}

/// The icon drawn `side` pixels square: below 32 px the bars are heavier, so they stay apart and seen.
pub fn app_icon(side: u32) -> AppIcon {
    let stroke = match side {
        0..=16 => 2.8,
        17..=24 => 2.6,
        _ => 2.2,
    };
    AppIcon {
        tile: Accent::DEFAULT.seed(),
        mark: Palette::new(Scheme::Dark, Accent::DEFAULT).color(Role::Surface(Surface::Base)),
        corner: 58.0 / 256.0,
        mark_box: 220.8 / 256.0,
        stroke,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Ratio;

    #[test]
    fn the_mark_is_found_on_its_tile() {
        let icon = app_icon(16);
        assert!(Ratio::between(icon.mark, icon.tile).meets(Ratio::NON_TEXT));
    }

    #[test]
    fn a_small_icon_has_the_heavier_bars() {
        assert!(app_icon(16).stroke > app_icon(24).stroke && app_icon(24).stroke > app_icon(32).stroke);
        assert_eq!(app_icon(32).stroke, app_icon(512).stroke);
    }
}
