//! Every kind of emoji draws in colour inside its box, from the bundled set: a COLRv0 build, because the pinned
//! text stack cannot draw a COLRv1 emoji font (Fedora's `Noto Color Emoji`).

use design::{Accent, Scheme, Surface, TypeStyle};
use iced_core::{Length, Pixels, Size};
use iced_widget::{container, text};
use ui::testing::{self, Image};
use ui::{Surfaced, Theme};

const PADDING: f32 = 12.0;
const SIZE: Size = Size::new(140.0, 80.0);

/// How many pixels are visibly coloured (not a grey), with their bounding box, or none.
fn coloured(image: &Image) -> Option<(usize, [u32; 4])> {
    const LEAST_CHANNEL_SPREAD: u8 = 40;
    let width = image.size.width;
    let mut count = 0;
    let mut bounds = [u32::MAX, u32::MAX, 0, 0];
    for (at, pixel) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel[0].abs_diff(pixel[1]) > LEAST_CHANNEL_SPREAD || pixel[1].abs_diff(pixel[2]) > LEAST_CHANNEL_SPREAD {
            let (x, y) = (at as u32 % width, at as u32 / width);
            count += 1;
            bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x), bounds[3].max(y)];
        }
    }
    (count > 0).then_some((count, bounds))
}

fn drawn(sample: &str) -> Image {
    let theme = Theme::new(Scheme::Light, Accent::DEFAULT);
    let panel = container(
        text(sample.to_string()).size(TypeStyle::Heading.size()).line_height(Pixels(TypeStyle::Heading.line_height())),
    )
    .padding(PADDING)
    .width(Length::Fill)
    .height(Length::Fill)
    .class(Surfaced::On(Surface::Raised));
    testing::render::<()>(&theme, panel, SIZE, 1.0)
}

#[test]
fn text_has_no_colour_and_every_kind_of_emoji_has_some_inside_its_box() {
    assert_eq!(coloured(&drawn("Aa")), None, "plain text is grey; a colour test that fires on it proves nothing");
    const FACE: &str = "🙂";
    // A base and a modifier, which must join into one glyph.
    const SKIN_TONE: &str = "👍🏽";
    const ZWJ_FAMILY: &str = "👨‍👩‍👧";
    // A drawn glyph at the heading size colours dozens of pixels (the thumb about 80); a blank one none.
    const LEAST_COLOURED: usize = 50;
    for emoji in [FACE, SKIN_TONE, ZWJ_FAMILY] {
        let (count, [left, top, right, bottom]) = coloured(&drawn(emoji)).unwrap_or_else(|| panic!("{emoji} is blank"));
        assert!(count > LEAST_COLOURED, "{emoji}: only {count} coloured pixels");
        assert!(left as f32 >= PADDING - 1.0 && top as f32 >= PADDING - 1.0, "{emoji} starts at {left},{top}");
        assert!(
            right as f32 <= SIZE.width - PADDING + 1.0 && bottom as f32 <= SIZE.height - PADDING + 1.0,
            "{emoji} ends at {right},{bottom}"
        );
    }
}

/// Twemoji draws each kind of emoji a body sets in it, and a body sets nothing else in it: a letter or a private-use
/// scalar is left to the text's own font.
#[test]
fn twemoji_draws_each_kind_of_emoji_a_body_sets_in_it() {
    for emoji in ["🙂", "👍🏽", "👨‍👩‍👧", "🇬🇷", "#️⃣", "❤️"] {
        assert!(ui::fonts::emoji_draws(emoji), "{emoji}");
    }
    for not in ["A", "\u{E000}", "\u{200D}", ""] {
        assert!(!ui::fonts::emoji_draws(not), "{not:?}");
    }
}
