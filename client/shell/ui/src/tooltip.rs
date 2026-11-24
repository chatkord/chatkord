//! A tooltip: the name of a control that has only a picture, shown after the pointer rests on it.
//! It is never the only place a name is: the control carries it as its label.

use crate::typography::text;
use crate::{Renderer, Surfaced, Theme};
use design::{Elevation, Label, Radius, Space, TOOLTIP_DELAY, TypeStyle};
use iced_core::{Element, Padding, Widget};
use iced_widget::container;
use iced_widget::tooltip::{Position, Tooltip};

/// `content` with `name` shown at `position` once the pointer has rested on it.
pub fn tooltip<'a, Message: 'a, R: Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    name: &Label,
    position: Position,
) -> Element<'a, Message, Theme, R> {
    let tip = container(text(name.as_str().to_string(), TypeStyle::Caption))
        .padding(Padding::from([Space::Xs.px(), Space::S.px()]))
        .class(Surfaced::Panel(Elevation::Overlay, Radius::M));
    Tooltip::new(content.into(), tip, position).gap(Space::Xs.px()).delay(TOOLTIP_DELAY)._boxed()
}
