//! Colour as a screen shows it (8-bit sRGB), the perceptual space accents are derived in (OKLCH,
//! Björn Ottosson's Oklab in polar form), and the contrast ratio WCAG 2.2 defines between two
//! colours. The arithmetic is in `f64`; a colour leaves as the 8-bit value that is painted, so a
//! contrast is always measured on what a person sees.

use crate::listed;

/// An opaque colour, 8 bits per channel, sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red, encoded as sRGB.
    pub r: u8,
    /// Green, encoded as sRGB.
    pub g: u8,
    /// Blue, encoded as sRGB.
    pub b: u8,
}

/// A colour with coverage: `alpha` 255 is opaque.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// The colour where it covers fully.
    pub rgb: Rgb,
    /// How much it covers: 0 is none, 255 all.
    pub alpha: u8,
}

/// A colour in OKLCH: lightness 0 to 1, chroma from 0, hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    /// Lightness: 0 is black, 1 white.
    pub l: f64,
    /// Chroma: 0 is grey; how far sRGB reaches depends on the hue and the lightness.
    pub c: f64,
    /// Hue in degrees, from 0 up to 360.
    pub h: f64,
}

listed! {
    /// How a renderer mixes a translucent colour over another: on the encoded sRGB values, or on
    /// linear light. Renderers differ, so a contract over a translucent colour holds under both.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Blend {
        /// On the encoded sRGB values.
        Gamma,
        /// On linear light.
        Linear,
    }
}

/// A WCAG 2.2 contrast ratio, from 1 (none) to 21 (black on white).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Ratio(f64);

impl Ratio {
    /// Text and images of text (WCAG 1.4.3, AA).
    pub const TEXT: Ratio = Ratio(4.5);
    /// Controls' boundaries, focus indicators, icons and large text (WCAG 1.4.11 and 1.4.3).
    pub const NON_TEXT: Ratio = Ratio(3.0);
    /// What is disabled. WCAG asks nothing of it; we keep it visible.
    pub const DISABLED: Ratio = Ratio(2.0);
    /// Text under "More contrast" (WCAG 1.4.6, AAA).
    pub const TEXT_MORE: Ratio = Ratio(7.0);
    /// Boundaries and icons under "More contrast": what AA asks of text.
    pub const NON_TEXT_MORE: Ratio = Ratio(4.5);
    /// What is disabled under "More contrast": what the standard contract asks of what must be found.
    pub const DISABLED_MORE: Ratio = Ratio(3.0);
    /// The least difference between two fills a person sees as a change: a control's answer to the pointer.
    pub const VISIBLE: Ratio = Ratio(1.1);

    /// The ratio between two colours, the same whichever is given first.
    pub fn between(a: Rgb, b: Rgb) -> Ratio {
        let (one, other) = (a.luminance(), b.luminance());
        let (light, dark) = if one > other { (one, other) } else { (other, one) };
        Ratio((light + 0.05) / (dark + 0.05))
    }

    /// The ratio as a number, from 1 to 21.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Whether this ratio is at least `required`.
    pub fn meets(self, required: Ratio) -> bool {
        self.0 >= required.0
    }
}

/// The letter on a colour: black or white, whichever contrasts more. Every colour reads at least 4.5:1 with one of
/// them, so a picked colour is drawn as picked; More contrast's 7:1 is not promised on one.
pub fn readable_on(fill: Rgb) -> Rgb {
    if Ratio::between(Rgb::WHITE, fill) >= Ratio::between(Rgb::BLACK, fill) { Rgb::WHITE } else { Rgb::BLACK }
}

impl Rgb {
    /// The darkest colour sRGB shows.
    pub const BLACK: Rgb = Rgb::hex(0x000000);
    /// The lightest colour sRGB shows.
    pub const WHITE: Rgb = Rgb::hex(0xFFFFFF);

    /// The colour of these channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    /// `0xRRGGBB`; bits above the low 24 are ignored.
    pub const fn hex(value: u32) -> Rgb {
        let [_, r, g, b] = value.to_be_bytes();
        Rgb { r, g, b }
    }

    /// Relative luminance as WCAG 2.2 defines it, 0 for black and 1 for white.
    pub fn luminance(self) -> f64 {
        let [r, g, b] = self.linear();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// This colour in OKLCH, where a lightness is moved without moving the hue.
    pub fn to_oklch(self) -> Oklch {
        let [r, g, b] = self.linear();
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
        let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
        let b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
        Oklch { l: lightness, c: a.hypot(b), h: b.atan2(a).to_degrees().rem_euclid(360.0) }
    }

    fn linear(self) -> [f64; 3] {
        [self.r, self.g, self.b].map(|channel| decode(f64::from(channel) / 255.0))
    }

    fn from_linear([r, g, b]: [f64; 3]) -> Rgb {
        Rgb { r: quantise(r), g: quantise(g), b: quantise(b) }
    }
}

impl Rgba {
    /// `rgb` covering `alpha` of what is below it.
    pub const fn new(rgb: Rgb, alpha: u8) -> Rgba {
        Rgba { rgb, alpha }
    }

    /// This colour painted over an opaque one.
    pub fn over(self, below: Rgb, blend: Blend) -> Rgb {
        let alpha = f64::from(self.alpha) / 255.0;
        let mix = |top: f64, bottom: f64| top * alpha + bottom * (1.0 - alpha);
        match blend {
            Blend::Gamma => {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=255 first")]
                let channel =
                    |top: u8, bottom: u8| mix(f64::from(top), f64::from(bottom)).round().clamp(0.0, 255.0) as u8;
                Rgb::new(channel(self.rgb.r, below.r), channel(self.rgb.g, below.g), channel(self.rgb.b, below.b))
            }
            Blend::Linear => {
                let [tr, tg, tb] = self.rgb.linear();
                let [br, bg, bb] = below.linear();
                Rgb::from_linear([mix(tr, br), mix(tg, bg), mix(tb, bb)])
            }
        }
    }
}

impl Oklch {
    /// The nearest colour a screen can show: the same lightness and hue, with chroma reduced until
    /// it is inside sRGB.
    pub fn to_rgb(self) -> Rgb {
        if self.l <= 0.0 {
            return Rgb::BLACK;
        }
        if self.l >= 1.0 {
            return Rgb::WHITE;
        }
        let linear = |chroma: f64| {
            let (a, b) = (chroma * self.h.to_radians().cos(), chroma * self.h.to_radians().sin());
            let l = (self.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
            let m = (self.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
            let s = (self.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
            [
                4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
                -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
                -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
            ]
        };
        let inside = |rgb: [f64; 3]| rgb.iter().all(|channel| (-1e-9..=1.0 + 1e-9).contains(channel));
        if inside(linear(self.c)) {
            return Rgb::from_linear(linear(self.c));
        }
        let (mut low, mut high) = (0.0, self.c);
        for _ in 0..24 {
            let middle = (low + high) / 2.0;
            if inside(linear(middle)) {
                low = middle;
            } else {
                high = middle;
            }
        }
        Rgb::from_linear(linear(low))
    }
}

fn decode(encoded: f64) -> f64 {
    if encoded <= 0.040_45 { encoded / 12.92 } else { ((encoded + 0.055) / 1.055).powf(2.4) }
}

fn encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=255 first")]
fn quantise(linear: f64) -> u8 {
    (encode(linear.clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(found: f64, expected: f64, tolerance: f64) -> bool {
        (found - expected).abs() <= tolerance
    }

    /// The reference values Ottosson published for the sRGB primaries, as (L, a, b).
    #[test]
    fn oklab_matches_the_published_values_for_the_primaries() {
        for (rgb, l, a, b) in [
            (Rgb::hex(0xFF0000), 0.627_955, 0.224_863, 0.125_846),
            (Rgb::hex(0x00FF00), 0.866_440, -0.233_888, 0.179_498),
            (Rgb::hex(0x0000FF), 0.452_014, -0.032_457, -0.311_528),
            (Rgb::WHITE, 1.0, 0.0, 0.0),
        ] {
            let lch = rgb.to_oklch();
            let (found_a, found_b) = (lch.c * lch.h.to_radians().cos(), lch.c * lch.h.to_radians().sin());
            assert!(close(lch.l, l, 1e-4) && close(found_a, a, 1e-4) && close(found_b, b, 1e-4), "{rgb:?}: {lch:?}");
        }
    }

    #[test]
    fn every_colour_survives_the_trip_through_oklch() {
        for value in (0..=0xFF_FF_FF).step_by(0x01_03_07) {
            let rgb = Rgb::hex(value);
            assert_eq!(rgb.to_oklch().to_rgb(), rgb);
        }
        for grey in 0..=255 {
            let rgb = Rgb::new(grey, grey, grey);
            assert_eq!(rgb.to_oklch().to_rgb(), rgb);
        }
    }

    #[test]
    fn a_colour_outside_srgb_keeps_its_lightness_and_loses_chroma() {
        let wanted = Oklch { l: 0.7, c: 0.4, h: 145.0 };
        let shown = wanted.to_rgb().to_oklch();
        assert!(close(shown.l, 0.7, 5e-3), "{shown:?}");
        assert!(shown.c < 0.4 && shown.c > 0.1, "{shown:?}");
        assert!(close(shown.h, 145.0, 2.0), "{shown:?}");
    }

    #[test]
    fn contrast_is_as_wcag_states() {
        assert!(close(Ratio::between(Rgb::BLACK, Rgb::WHITE).value(), 21.0, 1e-9));
        assert!(close(Ratio::between(Rgb::WHITE, Rgb::WHITE).value(), 1.0, 1e-9));
        // The commonly cited lightest grey that passes AA on white.
        assert!(close(Ratio::between(Rgb::hex(0x767676), Rgb::WHITE).value(), 4.54, 5e-3));
        assert!(!Ratio::between(Rgb::hex(0x777777), Rgb::WHITE).meets(Ratio::TEXT));
        assert_eq!(Ratio::between(Rgb::hex(0x123456), Rgb::WHITE), Ratio::between(Rgb::WHITE, Rgb::hex(0x123456)));
    }

    #[test]
    fn a_translucent_colour_mixes_by_its_coverage() {
        let half_black = Rgba::new(Rgb::BLACK, 128);
        assert_eq!(half_black.over(Rgb::WHITE, Blend::Gamma), Rgb::new(127, 127, 127));
        // Half the light is a lighter grey once encoded.
        assert_eq!(half_black.over(Rgb::WHITE, Blend::Linear), Rgb::new(187, 187, 187));
        assert_eq!(Rgba::new(Rgb::hex(0x336699), 255).over(Rgb::WHITE, Blend::Linear), Rgb::hex(0x336699));
        assert_eq!(Rgba::new(Rgb::hex(0x336699), 0).over(Rgb::WHITE, Blend::Gamma), Rgb::WHITE);
    }
}
