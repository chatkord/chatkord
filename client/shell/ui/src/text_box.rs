//! A box of text that scrolls past a height of its own, on the raised surface: a licence's text, a debug report.

use crate::{Renderer, Surfaced, Theme};
use design::{Content, Radius, Role, Space, Surface};
use iced_core::{Element, Length, Widget};
use iced_widget::{container, scrollable};

/// `text` in a raised, rounded box that grows to `height` and scrolls beyond it.
pub(crate) fn text_box<'a, Message: 'a, R: Renderer + 'a>(
    text: Element<'a, Message, Theme, R>,
    height: f32,
) -> Element<'a, Message, Theme, R> {
    let scrolled = scrollable(container(text).padding(Space::S.px() + Space::Xxs.px()).width(Length::Fill))
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .width(Length::Fill);
    container(scrolled)
        .height(Length::Shrink.max(height))
        .width(Length::Fill)
        .clip(true)
        .class(Surfaced::Fill {
            fill: Role::Surface(Surface::Raised),
            text: Role::Content(Content::Primary),
            radius: Radius::S,
        })
        ._boxed()
}
