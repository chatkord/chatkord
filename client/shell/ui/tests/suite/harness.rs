//! The harness against itself: the comparator fails where it must and passes where it may, the fonts are ours alone,
//! and a flat surface renders exactly its palette colour, so the tolerance hides no rendering difference.

use design::{Accent, Face, Role, Scheme, Surface, TypeStyle, Weight};
use iced_core::{Length, Size};
use iced_graphics::text::cosmic_text::{Buffer, Metrics, Shaping};
use iced_graphics::text::{font_system, to_attributes};
use iced_widget::{container, text};
use ui::fonts;
use ui::testing::{self, CHANNEL_TOLERANCE, Image, PIXEL_TOLERANCE, compare};
use ui::{Surfaced, Theme};

fn flat(width: u32, height: u32, value: u8) -> Image {
    Image { size: Size::new(width, height), rgba: vec![value; (width * height * 4) as usize] }
}

#[test]
fn the_comparator_fails_a_single_changed_pixel_it_may_not_forgive() {
    const SIDE: u32 = 100;
    let reference = flat(SIDE, SIDE, 100);
    let forgiven = (PIXEL_TOLERANCE * f64::from(SIDE * SIDE)) as usize;
    let mut changed = reference.clone();
    changed.rgba[0] = 200;
    assert!(compare(&reference, &changed).unwrap().within_tolerance(), "one pixel is within the share");
    for pixel in 0..2 * forgiven {
        changed.rgba[pixel * 4] = 200;
    }
    let difference = compare(&reference, &changed).unwrap();
    assert!(!difference.within_tolerance(), "{difference:?}");
}

#[test]
fn the_comparator_forgives_antialiasing_noise_and_nothing_more() {
    let reference = flat(50, 50, 100);
    let noise = flat(50, 50, 100 + CHANNEL_TOLERANCE);
    assert!(compare(&reference, &noise).unwrap().within_tolerance());
    let shifted = flat(50, 50, 100 + CHANNEL_TOLERANCE + 1);
    assert!(!compare(&reference, &shifted).unwrap().within_tolerance(), "one step over the tolerance is a change");
    assert_eq!(compare(&reference, &flat(50, 51, 100)), None, "a different size is never the same picture");
}

#[test]
fn the_only_faces_are_the_bundled_ones() {
    testing::hermetic_fonts();
    assert_eq!(testing::font_families(), [ui::fonts::MONO_FAMILY, ui::fonts::FAMILY, ui::fonts::EMOJI_FAMILY]);
}

/// How wide `words` are set in `font`, and the family of every face that drew a glyph of them.
fn shaped(words: &str, font: iced_core::Font) -> (f32, Vec<String>) {
    testing::hermetic_fonts();
    let mut system = font_system().write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let raw = system.raw();
    let mut buffer = Buffer::new(raw, Metrics::new(14.0, 20.0));
    buffer.set_text(words, &to_attributes(font), Shaping::Advanced, None);
    buffer.shape_until_scroll(raw, false);
    let runs: Vec<_> = buffer.layout_runs().map(|run| (run.line_w, run.glyphs.to_vec())).collect();
    let width = runs.iter().map(|(width, _)| *width).fold(0.0, f32::max);
    let mut families: Vec<String> = runs
        .iter()
        .flat_map(|(_, glyphs)| glyphs.iter().map(|glyph| glyph.font_id))
        .filter_map(|id| raw.db().face(id).and_then(|face| face.families.first().map(|(name, _)| name.clone())))
        .collect();
    families.sort();
    families.dedup();
    (width, families)
}

/// One variable file serves every weight: each is set at its own width and drawn with more ink than
/// the one before, so neither the shaping nor the drawing falls back to the file's default weight.
#[test]
fn each_weight_is_its_own() {
    let widths = Weight::ALL.map(|weight| shaped("Sign in to the server", fonts::weighted(Face::Sans, weight)).0);
    assert!(widths.windows(2).all(|pair| pair[0] < pair[1]), "{widths:?}");
    let theme = Theme::new(Scheme::Light, Accent::DEFAULT);
    let ink = Weight::ALL.map(|weight| {
        let words = text("Sign in").font(fonts::weighted(Face::Sans, weight)).size(TypeStyle::Heading.size());
        let image = testing::render::<()>(
            &theme,
            container(words).class(Surfaced::On(Surface::Base)),
            Size::new(160.0, 40.0),
            1.0,
        );
        image.rgba.as_chunks::<4>().0.iter().map(|pixel| u32::from(255 - pixel[1])).sum::<u32>()
    });
    assert!(ink.windows(2).all(|pair| pair[0] < pair[1]), "{ink:?}");
}

/// Greek and Cyrillic are in the words' own typeface, so a line that mixes scripts reads as one.
#[test]
fn greek_and_cyrillic_are_set_in_the_bundled_face() {
    for words in ["Καλημέρα κόσμε", "Привет, мир", "Ada"] {
        let (_, families) = shaped(words, fonts::font(TypeStyle::Body));
        assert_eq!(families, [fonts::FAMILY], "{words}");
    }
    let (_, families) = shaped("example.invalid:8443", fonts::font(TypeStyle::Mono));
    assert_eq!(families, [fonts::MONO_FAMILY]);
}

/// A tolerance that absorbs a real difference is worse than none: a flat surface must come out
/// as exactly the colour the palette says, in both schemes.
#[test]
fn a_flat_surface_renders_as_exactly_its_palette_colour() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(40.0, 30.0);
        let panel = container(text("")).width(Length::Fill).height(Length::Fill).class(Surfaced::On(Surface::Raised));
        let image = testing::render::<()>(&theme, panel, size, 1.0);
        let want = theme.palette().color(Role::Surface(Surface::Raised));
        assert_eq!(image.pixel(20, 15), [want.r, want.g, want.b, 255], "{scheme:?}");
        assert_eq!(image.size, Size::new(40, 30));
    }
}

#[test]
fn a_reference_passes_what_it_was_made_from_and_fails_what_differs_leaving_evidence() {
    let dir = std::env::temp_dir().join(format!("ui-harness-{}", std::process::id()));
    let refs = testing::Refs::new(dir.join("refs"), dir.join("failures"));
    let reference = flat(8, 8, 50);
    assert!(matches!(refs.verdict("one", &reference), Err(testing::Mismatch::Missing(_))), "no silent first pass");
    refs.accept("one", &reference).unwrap();
    refs.verdict("one", &reference).unwrap();

    assert!(matches!(refs.verdict("one", &flat(8, 8, 90)), Err(testing::Mismatch::Pixels(_))));
    for evidence in ["one-actual.png", "one-expected.png", "one-diff.png"] {
        assert!(dir.join("failures").join(evidence).exists(), "{evidence}");
    }
    assert!(matches!(refs.verdict("one", &flat(8, 9, 50)), Err(testing::Mismatch::Size { .. })));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Text on a raised surface renders the same twice and is the reference in both schemes; it is Greek, so a font
/// fallback that drew boxes would be in the picture.
#[test]
fn text_on_a_surface_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let sample = container(text("Σύνδεση στον διακομιστή. Sign in, Aa 123"))
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(Surfaced::On(Surface::Raised));
        let image = testing::render::<()>(&theme, sample, Size::new(320.0, 40.0), 1.0);
        let again = {
            let sample = container(text("Σύνδεση στον διακομιστή. Sign in, Aa 123"))
                .padding(8)
                .width(Length::Fill)
                .height(Length::Fill)
                .class(Surfaced::On(Surface::Raised));
            testing::render::<()>(&theme, sample, Size::new(320.0, 40.0), 1.0)
        };
        assert_eq!(image, again, "a render is the same twice in one process");
        ui::refs!().assert(&format!("text-{scheme:?}").to_lowercase(), &image);
    }
}

/// Emoji beside text, drawn from the bundled set: a face, a skin tone, a ZWJ family, in a line of Latin and Greek.
#[test]
fn emoji_in_a_line_of_text_are_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let style = design::TypeStyle::Title;
        let line = container(
            text("Καλημέρα 🙂 👍🏽 👨‍👩‍👧 ok").size(style.size()).line_height(iced_core::Pixels(style.line_height())),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
        let image = testing::render::<()>(&theme, line, Size::new(320.0, 44.0), 1.0);
        ui::refs!().assert(&format!("emoji-{scheme:?}").to_lowercase(), &image);
    }
}
