//! A badge: what someone in a call, or the call, is doing, in a word on a colour. The word is what
//! says it; the colour is only a second voice.

use crate::typography::text;
use crate::{Labelled, Renderer, Surfaced, Theme};
use design::{Presence, Radius, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Element, Padding, Widget};
use iced_widget::container;
use screens::presence_phrase;
use words::Words;

/// A badge for `presence`, whose word is the phrase the table gives it.
pub fn badge<'a, Message: 'a, R: Renderer + 'a>(
    presence: Presence,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let look = design::badge(presence);
    let word = words.phrase(presence_phrase(presence));
    let label = words.label(presence_phrase(presence));
    let pill = container(text(word, TypeStyle::Caption).wrapping(iced_core::text::Wrapping::None))
        .padding(Padding::from([Space::Xxs.px(), Space::S.px()]))
        .class(Surfaced::Fill { fill: look.fill, text: look.text, radius: Radius::Full });
    Labelled::new(pill, Semantics { role: SemanticRole::Status, label })._boxed()
}
