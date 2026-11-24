//! The colour of every role for one scheme, one accent and one contrast. The neutrals are fixed; every other role
//! keeps its seed's hue and what chroma the screen shows, its lightness moved the least that meets every pair of the
//! contract naming it. So any seed meets the contract, and the contract alone says what that is.

use crate::accent::Accent;
use crate::color::{Blend, Oklch, Ratio, Rgb, Rgba, readable_on};
use crate::contract::{Contrast, Ground, Pair, contract_at};
use crate::role::{AccentRole, Border, Content, DestructiveRole, Role, Status, StatusRole, Surface};
use crate::scheme::Scheme;

/// The step of the lightness search; fine enough that a nearer colour would not look different.
const STEP: f64 = 0.002;
/// How far a hovered and a pressed fill move from the fill, in OKLCH lightness.
const HOVER: f64 = 0.05;
const PRESSED: f64 = 0.10;

/// What the status colours grow from.
const fn status_seed(status: Status) -> Rgb {
    match status {
        Status::Success => Rgb::hex(0x5CD69B),
        Status::Warning => Rgb::hex(0xFFC857),
        Status::Danger => Rgb::hex(0xFF7B8A),
        Status::Info => Rgb::hex(0x3B82F6),
    }
}

/// A deeper amber than the warning's, so the glow round someone speaking is a colour nothing else paints.
const SPEAKING_SEED: Rgb = Rgb::hex(0xE0A100);

/// Laid over a picture under a label. Dark enough that white reads over pure white even when a
/// renderer mixes in linear light, which lightens a translucent black the most.
const SCRIM: Rgba = Rgba::new(Rgb::BLACK, 212);
/// What a dialog shuts the screen away under, in each scheme: the darkest neutral, see-through.
const DIALOG_SCRIM_DARK: Rgba = Rgba::new(Rgb::new(8, 4, 12), 168);
const DIALOG_SCRIM_LIGHT: Rgba = Rgba::new(Rgb::new(34, 26, 38), 115);

/// A muted or deafened mark on the scrim, as the design draws it.
const ON_SCRIM_DANGER: Rgb = Rgb::hex(0xFFC8CE);
/// A control on the scrim: disabled (light, as the scrim over a white picture is a mid grey), and the fills it takes
/// under the pointer (the canvas's white over a picture, opaque, so text on them holds at every contrast).
const ON_SCRIM_DISABLED: Rgb = Rgb::hex(0xDAD4DF);
const SCRIM_HOVER: Rgb = Rgb::hex(0x45404A);
const SCRIM_PRESSED: Rgb = Rgb::hex(0x554E5B);
/// White at about half over the black picture ground: the least that meets a control's edge at more contrast too.
const PICTURE_EDGE: Rgb = Rgb::hex(0x777777);

/// How much of the accent shows through where text is selected.
const SELECTION_ALPHA: u8 = 96;

/// How much of a seed a tint lays over the base: a quiet ground its own text still reads on.
const TINT_ALPHA: u8 = 41;

/// A mark drawn in a picked colour: its text and the ground under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkLook {
    /// The text, in the picked colour where it reads.
    pub text: Rgb,
    /// The ground, the picked colour laid thinly over the base.
    pub ground: Rgb,
}

/// Every colour for one scheme, one accent and one contrast.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    scheme: Scheme,
    contrast: Contrast,
    secondary: Rgb,
    disabled: Rgb,
    scrim: Rgba,
    accent: [Rgb; AccentRole::ALL.len()],
    status: [[Rgb; StatusRole::ALL.len()]; Status::ALL.len()],
    destructive: [Rgb; DestructiveRole::ALL.len()],
    speaking: Rgb,
    strong: Rgb,
}

impl Palette {
    /// The palette of the standard contract.
    pub fn new(scheme: Scheme, accent: Accent) -> Palette {
        Palette::with_contrast(scheme, accent, Contrast::Standard)
    }

    /// The palette of `contrast`'s contract.
    pub fn with_contrast(scheme: Scheme, accent: Accent, contrast: Contrast) -> Palette {
        let mut palette = Palette {
            scheme,
            contrast,
            secondary: content_color(scheme, Content::Secondary),
            disabled: content_color(scheme, Content::Disabled),
            scrim: SCRIM,
            accent: [extreme(scheme); AccentRole::ALL.len()],
            status: [[extreme(scheme); StatusRole::ALL.len()]; Status::ALL.len()],
            destructive: [extreme(scheme); DestructiveRole::ALL.len()],
            speaking: extreme(scheme),
            strong: extreme(scheme),
        };
        let pairs = contract_at(contrast);
        if contrast == Contrast::More {
            // Against the surfaces alone: the tints they are also read on are derived after them, to suit them.
            let on_surfaces: Vec<Pair> = pairs
                .iter()
                .copied()
                .filter(|pair| matches!(pair.ground, Ground::Role(Role::Surface(_) | Role::DisabledFill)))
                .collect();
            for content in [Content::Secondary, Content::Disabled] {
                let seed = content_color(scheme, content).to_oklch();
                palette.derive(&on_surfaces, seed, Role::Content(content));
            }
            // A label on a picture: the scrim under it darkened, the least that reads.
            let on_scrim: Vec<Pair> = pairs.iter().copied().filter(|pair| pair.ground == Ground::Scrim).collect();
            palette.scrim = (SCRIM.alpha..=u8::MAX)
                .map(|alpha| Rgba::new(SCRIM.rgb, alpha))
                .find(|scrim| {
                    let trial = Palette { scrim: *scrim, ..palette.clone() };
                    on_scrim.iter().all(|pair| pair.holds(&trial))
                })
                .unwrap_or(Rgba::new(SCRIM.rgb, u8::MAX));
        }
        palette.derive(&pairs, divider(scheme).to_oklch(), Role::Border(Border::Strong));

        let seed = accent.seed().to_oklch();
        palette.derive_fill(&pairs, seed, Role::Accent(AccentRole::Fill), Role::Accent(AccentRole::OnFill));
        let fill = palette.color(Role::Accent(AccentRole::Fill)).to_oklch().l;
        let toward_contrast = match scheme {
            Scheme::Dark => 1.0,
            Scheme::Light => -1.0,
        };
        for (role, distance) in [(AccentRole::FillHover, HOVER), (AccentRole::FillPressed, PRESSED)] {
            let target = away(fill, toward_contrast * distance);
            palette.derive(&pairs, Oklch { l: target, ..seed }, Role::Accent(role));
        }
        // A tint before the text read on it, so that text is searched against the ground it will have.
        palette.derive(&pairs, tint(scheme, accent.seed()), Role::Accent(AccentRole::Tint));
        palette.derive(&pairs, seed, Role::Accent(AccentRole::Text));
        palette.derive(&pairs, Oklch { l: fill, ..seed }, Role::Accent(AccentRole::Ring));

        for status in Status::ALL {
            let seed = status_seed(status).to_oklch();
            palette.derive_fill(
                &pairs,
                seed,
                Role::Status(status, StatusRole::Fill),
                Role::Status(status, StatusRole::OnFill),
            );
            palette.derive(&pairs, tint(scheme, status_seed(status)), Role::Status(status, StatusRole::Tint));
            palette.derive(&pairs, seed, Role::Status(status, StatusRole::Text));
        }
        // A destructive button is the danger colour, solid; under the pointer it moves away from its text, so that
        // stays readable on every shade.
        let (seed, on) = destructive_seeds(scheme);
        let seed = seed.to_oklch();
        palette.derive_fill(
            &pairs,
            seed,
            Role::Destructive(DestructiveRole::Fill),
            Role::Destructive(DestructiveRole::OnFill),
        );
        palette.derive(&pairs, on.to_oklch(), Role::Destructive(DestructiveRole::OnFill));
        let fill = palette.color(Role::Destructive(DestructiveRole::Fill)).to_oklch().l;
        let text = palette.color(Role::Destructive(DestructiveRole::OnFill)).to_oklch().l;
        let from_text = if text > fill { -1.0 } else { 1.0 };
        for (role, distance) in [(DestructiveRole::FillHover, HOVER), (DestructiveRole::FillPressed, PRESSED)] {
            palette.derive(&pairs, Oklch { l: away(fill, from_text * distance), ..seed }, Role::Destructive(role));
        }
        palette.derive(&pairs, SPEAKING_SEED.to_oklch(), Role::Speaking);
        palette
    }

    /// The scheme it was made for.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// The contract it meets.
    pub fn contrast(&self) -> Contrast {
        self.contrast
    }

    /// The colour `role` is painted in.
    pub fn color(&self, role: Role) -> Rgb {
        match role {
            Role::Surface(surface) => surface_color(self.scheme, surface),
            Role::Content(Content::Primary) => content_color(self.scheme, Content::Primary),
            Role::Content(Content::Secondary) => self.secondary,
            Role::Content(Content::Disabled) => self.disabled,
            Role::Border(Border::Strong) => self.strong,
            Role::Border(Border::Subtle) => divider(self.scheme),
            Role::Accent(accent) => self.accent[accent as usize],
            Role::Status(status, part) => self.status[status as usize][part as usize],
            Role::Destructive(part) => self.destructive[part as usize],
            Role::OnScrim => Rgb::WHITE,
            Role::OnScrimDanger => ON_SCRIM_DANGER,
            Role::OnScrimDisabled => ON_SCRIM_DISABLED,
            Role::ScrimHover => SCRIM_HOVER,
            Role::ScrimPressed => SCRIM_PRESSED,
            Role::DisabledFill => disabled_fill(self.scheme),
            Role::PictureGround => Rgb::BLACK,
            Role::PictureEdge => PICTURE_EDGE,
            Role::Speaking => self.speaking,
        }
    }

    /// The highlight behind selected text: the accent's fill, translucent.
    pub fn selection(&self) -> Rgba {
        Rgba::new(self.color(Role::Accent(AccentRole::Fill)), SELECTION_ALPHA)
    }

    /// The translucent layer under a label on a picture.
    pub fn scrim(&self) -> Rgba {
        self.scrim
    }

    /// A mark in a picked colour (a role's mention): its ground, the colour laid thinly over the base as a tint is,
    /// and its text, the picked colour moved the least that reads on that ground at this contract's text ratio.
    pub fn mark(&self, picked: Rgb) -> MarkLook {
        let ground = tint(self.scheme, picked).to_rgb();
        let ratio = match self.contrast {
            Contrast::Standard => Ratio::TEXT,
            Contrast::More => Ratio::TEXT_MORE,
        };
        let text = nearest(picked.to_oklch(), |candidate| Ratio::between(candidate, ground).meets(ratio))
            .unwrap_or_else(|| readable_on(ground));
        MarkLook { text, ground }
    }

    /// The layer a dialog lays over the screen it shuts away.
    pub fn dialog_scrim(&self) -> Rgba {
        match self.scheme {
            Scheme::Dark => DIALOG_SCRIM_DARK,
            Scheme::Light => DIALOG_SCRIM_LIGHT,
        }
    }

    /// A fill and the text on it: the fill nearest the seed whose best text colour (black or
    /// white, whichever contrasts more) meets every pair that mentions either.
    fn derive_fill(&mut self, pairs: &[Pair], seed: Oklch, fill: Role, on: Role) {
        let found = nearest(seed, |candidate| {
            let mut trial = self.clone();
            trial.set(fill, candidate);
            trial.set(on, readable_on(candidate));
            holds(&trial, pairs, fill) && holds(&trial, pairs, on)
        });
        self.set(fill, found.unwrap_or(extreme(self.scheme)));
        self.set(on, readable_on(self.color(fill)));
    }

    /// One colour: the nearest to `seed` that meets every pair that mentions `role`.
    fn derive(&mut self, pairs: &[Pair], seed: Oklch, role: Role) {
        let found = nearest(seed, |candidate| {
            let mut trial = self.clone();
            trial.set(role, candidate);
            holds(&trial, pairs, role)
        });
        self.set(role, found.unwrap_or(extreme(self.scheme)));
    }

    fn set(&mut self, role: Role, color: Rgb) {
        match role {
            Role::Accent(accent) => self.accent[accent as usize] = color,
            Role::Status(status, part) => self.status[status as usize][part as usize] = color,
            Role::Destructive(part) => self.destructive[part as usize] = color,
            Role::Speaking => self.speaking = color,
            Role::Border(Border::Strong) => self.strong = color,
            Role::Content(Content::Secondary) => self.secondary = color,
            Role::Content(Content::Disabled) => self.disabled = color,
            // The grounds, the main text and the text on the scrim are fixed, and nothing derives them.
            Role::Surface(_)
            | Role::Content(Content::Primary)
            | Role::Border(Border::Subtle)
            | Role::OnScrim
            | Role::OnScrimDanger
            | Role::OnScrimDisabled
            | Role::ScrimHover
            | Role::ScrimPressed
            | Role::DisabledFill
            | Role::PictureGround
            | Role::PictureEdge => {}
        }
    }
}

/// The lightness `distance` away from `from`, turned back if it would leave 0..=1.
fn away(from: f64, distance: f64) -> f64 {
    let there = from + distance;
    if (0.0..=1.0).contains(&there) { there } else { from - distance }
}

/// The colour with `seed`'s hue and chroma at the lightness nearest `seed`'s that `accepts`,
/// searching outward in both directions.
fn nearest(seed: Oklch, accepts: impl Fn(Rgb) -> bool) -> Option<Rgb> {
    #[expect(clippy::cast_possible_truncation, reason = "1 / STEP is a few hundred, far inside i32")]
    let steps = (1.0 / STEP).ceil() as i32;
    (0..=steps)
        .flat_map(|k| [k, -k])
        .map(|k| seed.l + f64::from(k) * STEP)
        .filter(|l| (0.0..=1.0).contains(l))
        .find_map(|l| {
            let candidate = Oklch { l, ..seed }.to_rgb();
            accepts(candidate).then_some(candidate)
        })
}

fn holds(palette: &Palette, pairs: &[Pair], role: Role) -> bool {
    pairs.iter().filter(|pair| pair.mentions(role)).all(|pair| pair.holds(palette))
}

/// The colour that always contrasts with this scheme's surfaces, where a search can end.
fn extreme(scheme: Scheme) -> Rgb {
    match scheme {
        Scheme::Dark => Rgb::WHITE,
        Scheme::Light => Rgb::BLACK,
    }
}

/// Warm night's neutrals. The window and what floats over it share a colour: height is shown by an
/// edge and a shadow.
const fn surface_color(scheme: Scheme, surface: Surface) -> Rgb {
    match (scheme, surface) {
        (Scheme::Dark, Surface::Base | Surface::Overlay) => Rgb::hex(0x17121C),
        (Scheme::Dark, Surface::Raised) => Rgb::hex(0x1F1826),
        (Scheme::Dark, Surface::Chrome) => Rgb::hex(0x120E16),
        (Scheme::Dark, Surface::Inset) => Rgb::hex(0x241C2C),
        (Scheme::Dark, Surface::Hover) => Rgb::hex(0x2E2438),
        (Scheme::Dark, Surface::Pressed) => Rgb::hex(0x3A2F44),
        (Scheme::Light, Surface::Base | Surface::Overlay) => Rgb::hex(0xFFF8F3),
        (Scheme::Light, Surface::Raised) => Rgb::hex(0xF7ECE4),
        (Scheme::Light, Surface::Chrome) => Rgb::hex(0xF1E3D9),
        (Scheme::Light, Surface::Inset) => Rgb::hex(0xFFFFFF),
        (Scheme::Light, Surface::Hover) => Rgb::hex(0xF0DED2),
        (Scheme::Light, Surface::Pressed) => Rgb::hex(0xE2D0C4),
    }
}

const fn content_color(scheme: Scheme, content: Content) -> Rgb {
    match (scheme, content) {
        (Scheme::Dark, Content::Primary) => Rgb::hex(0xF4EEF7),
        (Scheme::Dark, Content::Secondary) => Rgb::hex(0xB5A8BE),
        (Scheme::Dark, Content::Disabled) => Rgb::hex(0x958A9E),
        (Scheme::Light, Content::Primary) => Rgb::hex(0x221A26),
        (Scheme::Light, Content::Secondary) => Rgb::hex(0x62556A),
        (Scheme::Light, Content::Disabled) => Rgb::hex(0x75687B),
    }
}

/// The fill of a button that cannot be used, as the design draws it in each scheme.
const fn disabled_fill(scheme: Scheme) -> Rgb {
    match scheme {
        Scheme::Dark => Rgb::hex(0x2A2231),
        Scheme::Light => Rgb::hex(0xEFE3DA),
    }
}

/// A seed laid thinly over the base, as a browser blends it.
fn tint(scheme: Scheme, seed: Rgb) -> Oklch {
    Rgba::new(seed, TINT_ALPHA).over(surface_color(scheme, Surface::Base), Blend::Gamma).to_oklch()
}

/// The divider, and the seed the strong edge is derived from.
const fn divider(scheme: Scheme) -> Rgb {
    match scheme {
        Scheme::Dark => Rgb::hex(0x3E3249),
        Scheme::Light => Rgb::hex(0xDCC9BD),
    }
}

/// A destructive button's fill and its text, as the design draws them in each scheme.
const fn destructive_seeds(scheme: Scheme) -> (Rgb, Rgb) {
    match scheme {
        Scheme::Dark => (Rgb::hex(0xFF7B8A), Rgb::hex(0x2A0710)),
        Scheme::Light => (Rgb::hex(0xA82B43), Rgb::WHITE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_is_the_accent_fill_and_translucent() {
        let palette = Palette::new(Scheme::Dark, Accent::DEFAULT);
        let selection = palette.selection();
        assert_eq!(selection.rgb, palette.color(Role::Accent(AccentRole::Fill)));
        assert!(selection.alpha > 0 && selection.alpha < 255);
    }

    /// A picked colour's mark reads on its own ground at the text ratio, and keeps the colour where it already
    /// reads.
    #[test]
    fn a_marks_text_reads_on_its_ground() {
        for scheme in Scheme::ALL {
            for contrast in [Contrast::Standard, Contrast::More] {
                let palette = Palette::with_contrast(scheme, Accent::DEFAULT, contrast);
                for picked in [Rgb::hex(0xE2A6FF), Rgb::hex(0x7CB7FF), Rgb::hex(0xFFFF00), Rgb::hex(0x101010)] {
                    let mark = palette.mark(picked);
                    let wanted = if contrast == Contrast::More { Ratio::TEXT_MORE } else { Ratio::TEXT };
                    assert!(Ratio::between(mark.text, mark.ground).meets(wanted), "{scheme:?} {picked:?}: {mark:?}");
                }
            }
        }
        let light_on_dark = Palette::new(Scheme::Dark, Accent::DEFAULT).mark(Rgb::hex(0xE2A6FF));
        assert_eq!(light_on_dark.text, Rgb::hex(0xE2A6FF), "a colour that reads stays as picked");
    }

    /// What a shared screen is drawn on is black whichever scheme the window is in.
    #[test]
    fn the_picture_ground_is_the_same_black_in_both_schemes() {
        let [dark, light] = Scheme::ALL.map(|scheme| Palette::new(scheme, Accent::DEFAULT).color(Role::PictureGround));
        assert_eq!((dark, light), (Rgb::BLACK, Rgb::BLACK));
    }

    /// The strong edge is the divider's own colour, moved only as far as the contract needs.
    #[test]
    fn the_strong_edge_keeps_the_dividers_hue() {
        for scheme in Scheme::ALL {
            let strong = Palette::new(scheme, Accent::DEFAULT).color(Role::Border(Border::Strong)).to_oklch();
            let seed = divider(scheme).to_oklch();
            assert!(strong.c > 0.01 && (strong.h - seed.h).abs() < 4.0, "{scheme:?}: {strong:?} from {seed:?}");
        }
    }
}
