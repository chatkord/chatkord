//! A person's colour: the avatar's fill, the initials on it and the ground of their tile. A person picks any colour;
//! until they do, their id gives one of six swatches the design names, the same on every device.

use crate::color::{Blend, Ratio, Rgb, Rgba, readable_on};
use crate::listed;
use crate::palette::Palette;
use crate::role::{Content, Role, Surface};
use crate::scheme::Scheme;

listed! {
    /// One of the six colours another person can be, the same on every device.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Swatch {
        /// An orange clay.
        Clay,
        /// A violet.
        Violet,
        /// A teal.
        Teal,
        /// An indigo.
        Indigo,
        /// A rose.
        Rose,
        /// A green.
        Green,
    }
}

impl Swatch {
    /// The avatar's fill, as the design draws it in each scheme.
    pub const fn fill(self, scheme: Scheme) -> Rgb {
        match (scheme, self) {
            (Scheme::Dark, Swatch::Clay) => Rgb::hex(0xA0561B),
            (Scheme::Dark, Swatch::Violet) => Rgb::hex(0x8A44A8),
            (Scheme::Dark, Swatch::Teal) => Rgb::hex(0x1F7A6E),
            (Scheme::Dark, Swatch::Indigo) => Rgb::hex(0x6A4BC4),
            (Scheme::Dark, Swatch::Rose) => Rgb::hex(0xB23A72),
            (Scheme::Dark, Swatch::Green) => Rgb::hex(0x2F7D52),
            (Scheme::Light, Swatch::Clay) => Rgb::hex(0x8F4C14),
            (Scheme::Light, Swatch::Violet) => Rgb::hex(0x7B3C98),
            (Scheme::Light, Swatch::Teal) => Rgb::hex(0x1B6E63),
            (Scheme::Light, Swatch::Indigo) => Rgb::hex(0x5B3FB0),
            (Scheme::Light, Swatch::Rose) => Rgb::hex(0xA3325F),
            (Scheme::Light, Swatch::Green) => Rgb::hex(0x2B6E48),
        }
    }
}

/// A person's colour: the one they picked, or the swatch their id gives until they pick one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersonColour {
    /// The colour they picked, drawn as picked.
    Picked(Rgb),
    /// The swatch their id gives.
    Swatch(Swatch),
}

/// The colours of one person in a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersonLook {
    /// Their avatar's fill.
    pub fill: Rgb,
    /// The initials on the fill.
    pub on_fill: Rgb,
    /// What their tile is: the fill, thinly, over the window.
    pub ground: Rgb,
}

/// How much of a person's colour their tile takes at most, in each scheme.
const fn ground_alpha(scheme: Scheme) -> u8 {
    match scheme {
        Scheme::Dark => 0x38,
        Scheme::Light => 0x26,
    }
}

impl Palette {
    /// A person's colours in this palette's scheme. The fill is the colour as picked with a letter that reads on it;
    /// the tile takes it thinly, and more thinly still where the text on it would not keep this palette's contrast.
    pub fn person(&self, colour: PersonColour) -> PersonLook {
        let fill = match colour {
            PersonColour::Picked(rgb) => rgb,
            PersonColour::Swatch(swatch) => swatch.fill(self.scheme()),
        };
        let base = self.color(Role::Surface(Surface::Base));
        let texts = [Content::Primary, Content::Secondary].map(|content| self.color(Role::Content(content)));
        let reads = |ground: Rgb| texts.iter().all(|text| Ratio::between(*text, ground).meets(self.contrast().text()));
        let ground = (0..=ground_alpha(self.scheme()))
            .rev()
            .map(|alpha| Rgba::new(fill, alpha).over(base, Blend::Gamma))
            .find(|ground| reads(*ground))
            .unwrap_or(base);
        PersonLook { fill, on_fill: readable_on(fill), ground }
    }
}
