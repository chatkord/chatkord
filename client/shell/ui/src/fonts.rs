//! The typefaces bundled so a screen looks and measures the same on every system: Manrope for words, IBM Plex Mono for
//! numbers and addresses, Twemoji for emoji. Scripts they do not cover fall back to the system's.

use design::{Face, TypeStyle, Weight};
use iced_core::font::{self, Family};
use iced_core::{Font, Pixels, renderer};
use iced_graphics::text::cosmic_text::skrifa::{FontRef, MetadataProvider};
use iced_graphics::text::font_system;
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::LazyLock;

pub(crate) const SANS: &[u8] = include_bytes!("../assets/fonts/Manrope.ttf");
pub(crate) const MONO: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
pub(crate) const EMOJI: &[u8] = include_bytes!("../assets/fonts/Twemoji.Mozilla.ttf");

/// The family names the files declare.
pub const FAMILY: &str = "Manrope";
/// The family name IBM Plex Mono's file declares.
pub const MONO_FAMILY: &str = "IBM Plex Mono";
/// The family name Twemoji's file declares.
pub const EMOJI_FAMILY: &str = "Twemoji Mozilla";

/// Every file to load into the renderer's font system.
pub const FILES: [&[u8]; 3] = [SANS, MONO, EMOJI];

/// Loads the bundled fonts and drops the system's emoji, which the text stack tries first by name and may be COLRv1
/// (Fedora's `Noto Color Emoji`), which it cannot draw; Twemoji is COLRv0. Safe to call again.
pub fn install() {
    let mut system = font_system().write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let db = system.raw().db_mut();
    let emoji: Vec<_> = db
        .faces()
        .filter(|face| face.families.iter().any(|(family, _)| family.to_lowercase().contains("emoji")))
        .map(|face| face.id)
        .collect();
    for id in emoji {
        db.remove_face(id);
    }
    for file in FILES {
        system.load_font(Cow::Borrowed(file));
    }
}

/// The font a type style is set in.
pub fn font(style: TypeStyle) -> Font {
    weighted(style.face(), style.weight())
}

/// The font of a face at a weight.
pub fn weighted(face: Face, weight: Weight) -> Font {
    Font {
        family: Family::Name(match face {
            Face::Sans => FAMILY,
            Face::Mono => MONO_FAMILY,
        }),
        weight: match weight {
            Weight::Regular => font::Weight::Normal,
            Weight::Medium => font::Weight::Medium,
            Weight::SemiBold => font::Weight::Semibold,
            Weight::Bold => font::Weight::Bold,
            Weight::ExtraBold => font::Weight::ExtraBold,
        },
        ..Font::DEFAULT
    }
}

/// What a renderer starts with: the body style.
pub fn settings() -> renderer::Settings {
    let body = TypeStyle::Body;
    renderer::Settings {
        font: font(body),
        text_size: Pixels(body.size()),
        line_height: iced_core::text::LineHeight::Absolute(Pixels(body.line_height())),
        ..renderer::Settings::default()
    }
}

/// The emoji font, for a run that is one emoji it draws.
pub fn emoji() -> Font {
    Font::new(EMOJI_FAMILY)
}

/// Every scalar Twemoji maps to a glyph, read once from its own table.
static EMOJI_SCALARS: LazyLock<HashSet<u32>> = LazyLock::new(|| {
    FontRef::new(EMOJI).map(|font| font.charmap().mappings().map(|(scalar, _)| scalar).collect()).unwrap_or_default()
});

/// Whether Twemoji draws `emoji`: each of its scalars has a glyph there, the joiners, selectors and tags a sequence
/// carries aside, which shape into its glyphs and draw none of their own.
pub fn emoji_draws(emoji: &str) -> bool {
    let joins =
        |scalar: char| matches!(scalar, '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{20E3}' | '\u{E0020}'..='\u{E007F}');
    let mut drawn = emoji.chars().filter(|scalar| !joins(*scalar)).peekable();
    drawn.peek().is_some() && drawn.all(|scalar| EMOJI_SCALARS.contains(&u32::from(scalar)))
}
