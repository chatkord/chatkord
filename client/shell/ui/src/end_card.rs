//! A call that is over, said in the middle of its place as a centre panel: what happened, and the ways on, of which the
//! way back to the channels is always one.

use crate::{Button, Face, Icon, Renderer, Theme};
use design::ButtonVariant;
use iced_core::Element;
use screens::EndCard;
use words::{Phrase, Words};

/// The end `card` says: `again` is sent by its way back into the call, when it has one, and `back` by the way to the
/// channels, which is loading while `leaving`.
pub fn end_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    card: &EndCard,
    again: Message,
    back: Message,
    leaving: bool,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let label = |phrase| words.label(phrase);
    let mut ways: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if let Some(way) = card.way_back {
        ways.push(Button::new(label(way.phrase())).variant(ButtonVariant::Primary).on_press(again).view::<R>());
    }
    ways.push(
        Button::new(label(Phrase::BackToChannels))
            .variant(ButtonVariant::Secondary)
            .loading(leaving)
            .on_press(back)
            .view::<R>(),
    );
    crate::centre_panel(Face::Accent(Icon::PhoneOff), Some(words.say(&card.title)), Some(words.say(&card.body)), ways)
}
