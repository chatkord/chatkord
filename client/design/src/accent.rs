//! The accent the window is painted in. It is only the seed: what is painted is derived from it per scheme
//! ([`crate::Palette`]), so every pair the contract names keeps its contrast. The window's is always Chatkord's;
//! a colour a server or a person picks is drawn as picked, never through it.

use crate::color::Rgb;

/// A seed the accent roles are derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Accent(Rgb);

impl Accent {
    /// Chatkord's own: the window's accent, and the colour of a server that picked none.
    pub const DEFAULT: Accent = Accent(Rgb::hex(0xFF8A5B));

    /// The colour as chosen, before any derivation.
    pub const fn seed(self) -> Rgb {
        self.0
    }
}

impl From<Rgb> for Accent {
    fn from(rgb: Rgb) -> Accent {
        Accent(rgb)
    }
}
