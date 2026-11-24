//! The one place a `design` colour becomes an iced one. Anything else that builds a `Color` is
//! refused by `clippy.toml`, so a view cannot paint a value that is not a role.

use design::{Rgb, Rgba};
use iced_core::Color;

#[expect(clippy::disallowed_methods, reason = "this is the conversion the ban points everyone else to")]
pub(crate) fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

#[expect(clippy::disallowed_methods, reason = "this is the conversion the ban points everyone else to")]
pub(crate) fn translucent(rgba: Rgba) -> Color {
    Color::from_rgba8(rgba.rgb.r, rgba.rgb.g, rgba.rgb.b, f32::from(rgba.alpha) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_keeps_its_channels() {
        assert_eq!(color(Rgb::hex(0xFF8000)).into_rgba8(), [0xFF, 0x80, 0x00, 0xFF]);
        assert_eq!(translucent(Rgba::new(Rgb::hex(0x102030), 128)).into_rgba8(), [0x10, 0x20, 0x30, 0x80]);
    }
}
