//! The marks of something new in a list: a count of what needs a look, on the accent, and the dot beside a row with
//! unread messages. Each says nothing to a screen reader; the row it marks says it in its name.

use crate::typography::text;
use crate::{Renderer, Surfaced, Theme};
use design::{AccentRole, Content, Radius, Role, Space, Surface, TypeStyle};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Space as Gap, container};

/// `count` on a pill of the accent, as round as its figure allows: a channel's mentions.
pub fn count_badge<'a, Message: 'a, R: Renderer + 'a>(count: u32) -> Element<'a, Message, Theme, R> {
    counted(count, false)
}

/// [`count_badge`], and while `stale` (the last count held of a server not connected) on the inset surface in the
/// secondary colour.
pub(crate) fn counted<'a, Message: 'a, R: Renderer + 'a>(count: u32, stale: bool) -> Element<'a, Message, Theme, R> {
    let figure = text(count.to_string(), TypeStyle::CaptionStrong).wrapping(iced_core::text::Wrapping::None);
    container(figure)
        .padding(Padding::from([0.0, Space::Xs.px() + Space::Xxs.px()]))
        .center_y(Length::Fixed(design::MENTION_BADGE))
        .class(if stale {
            Surfaced::Fill {
                fill: Role::Surface(Surface::Inset),
                text: Role::Content(Content::Secondary),
                radius: Radius::Full,
            }
        } else {
            Surfaced::Fill {
                fill: Role::Accent(AccentRole::Fill),
                text: Role::Accent(AccentRole::OnFill),
                radius: Radius::Full,
            }
        })
        ._boxed()
}

/// The dot beside a row with unread messages, in the primary content colour.
pub fn unread_dot<'a, Message: 'a, R: Renderer + 'a>() -> Element<'a, Message, Theme, R> {
    let colour = Role::Content(Content::Primary);
    container(Gap::new())
        .width(Length::Fixed(design::UNREAD_DOT))
        .height(Length::Fixed(design::UNREAD_DOT))
        .class(Surfaced::Fill { fill: colour, text: colour, radius: Radius::Full })
        ._boxed()
}
