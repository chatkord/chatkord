//! A message's emoji draws in colour with this machine's own fonts installed beside the bundled ones, as the shell
//! has them: the body sets each emoji in the emoji font, so a system font covering its code point never draws it
//! first. Each test runs in a process of its own (nextest): the font system is one per process, and the other tests draw
//! with the bundled fonts alone.

use design::{Accent, MessageSize, Scheme, Space, Surface};
use iced_core::{Length, Size};
use iced_widget::container;
use screens::chat::Body;
use ui::testing::{self, Image, Renderer};
use ui::{RichBody, Surfaced, Theme};

/// How many pixels are visibly coloured, not a grey.
fn coloured(image: &Image) -> usize {
    const LEAST_CHANNEL_SPREAD: u8 = 40;
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0].abs_diff(p[1]) > LEAST_CHANNEL_SPREAD || p[1].abs_diff(p[2]) > LEAST_CHANNEL_SPREAD)
        .count()
}

#[test]
fn a_messages_emoji_draws_in_colour_over_this_machines_fonts() {
    let words = words::Words::english();
    let theme = Theme::new(Scheme::Light, Accent::DEFAULT);
    let face = Body::plain("🙂");
    let shown = container(RichBody::<()>::new(&face, MessageSize::DEFAULT, &words).view::<Renderer>())
        .padding(Space::S.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
    let image = testing::render_on_this_machine::<()>(&theme, shown, Size::new(48.0, 40.0), 1.0);
    // A 14 px face colours about a hundred pixels; a system font's monochrome glyph none.
    let count = coloured(&image);
    assert!(count > 40, "🙂 drew {count} coloured pixels: a system font drew it");
}
