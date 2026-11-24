//! The mark beside a choice of a few: a ring, with a dot in it when it is the one taken. The ring is the accent's when
//! taken and the strong edge colour when not, and the inside is the card's own surface.

use crate::{Renderer, Surfaced, Theme};
use design::{AccentRole, Border, Content, RADIO_DOT, RADIO_RING, RADIO_SIZE, Radius, Role, Surface};
use iced_core::{Element, Length, Widget};
use iced_widget::{Space, container};

fn round(fill: Role) -> Surfaced {
    Surfaced::Fill { fill, text: Role::Content(Content::Primary), radius: Radius::Full }
}

pub(crate) fn radio_dot<'a, Message: 'a, R: Renderer + 'a>(chosen: bool) -> Element<'a, Message, Theme, R> {
    let room = RADIO_SIZE - 2.0 * RADIO_RING;
    let held: Element<'a, Message, Theme, R> = if chosen {
        container(Space::new())
            .width(Length::Fixed(RADIO_DOT))
            .height(Length::Fixed(RADIO_DOT))
            .class(round(Role::Accent(AccentRole::Fill)))
            ._boxed()
    } else {
        Space::new()._boxed()
    };
    let inside = container(held).center(Length::Fixed(room)).class(round(Role::Surface(Surface::Base)));
    let ring = if chosen { Role::Accent(AccentRole::Fill) } else { Role::Border(Border::Strong) };
    container(inside)
        .padding(RADIO_RING)
        .width(Length::Fixed(RADIO_SIZE))
        .height(Length::Fixed(RADIO_SIZE))
        .class(round(ring))
        ._boxed()
}
