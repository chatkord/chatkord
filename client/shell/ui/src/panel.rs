//! A panel: what the server rail, the channel list and a voice tile are drawn on.

use crate::{Renderer, Surfaced, Theme};
use design::{CARD_RADIUS, Elevation, Space};
use iced_core::{Element, Widget};
use iced_widget::container;

/// `content` on a panel at `elevation`, padded by the scale.
pub fn panel<'a, Message: 'a, R: Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    elevation: Elevation,
) -> Element<'a, Message, Theme, R> {
    container(content.into()).padding(Space::L.px()).class(Surfaced::Panel(elevation, CARD_RADIUS))._boxed()
}
