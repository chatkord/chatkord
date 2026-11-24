//! A watched shared screen: what is being watched in a line, the picture, and beside it the card that says what the
//! picture is. The picture is a slot the host fills (its GPU painter), so this lays out and decides nothing.

use crate::{Renderer, Surfaced, TextClass, Theme, coloured, share_card};
use design::{Content, Space, Surface, TypeStyle};
use iced_core::{Element, Length, Widget};
use iced_widget::{Column, Row, container};
use screens::ShareCard;
use words::Words;

/// `line` over the host's `picture`, with `card` and the name of the member it is the share of beside it when there
/// is one, in `words`.
pub fn view<'a, Message: 'a, R: Renderer + 'a>(
    line: String,
    card: Option<(&ShareCard, &str)>,
    picture: Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let mut body: Vec<Element<'a, Message, Theme, R>> =
        vec![container(picture).width(Length::Fill).height(Length::Fill)._boxed()];
    body.extend(card.map(|(card, name)| {
        container(share_card::<Message, R>(card, name, words))
            .width(Length::Fill.max(design::SHARE_CARD_WIDTH))
            ._boxed()
    }));
    let page = Column::with_children([
        coloured(line, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed(),
        Row::with_children(body).spacing(Space::M.px()).height(Length::Fill)._boxed(),
    ])
    .spacing(Space::S.px());
    container(page)
        .padding(Space::M.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}
