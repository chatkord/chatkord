//! The dot that gives a line of telling its status: a toast, a state line and a callout draw the same one.

use crate::{Renderer, Surfaced, Theme};
use design::{Radius, Role, STATUS_DOT, Status, StatusRole};
use iced_core::{Element, Length, Widget};
use iced_widget::{Space as Gap, container};

/// The dot of `kind`, in its status colour.
pub(crate) fn status_dot<'a, Message: 'a, R: Renderer + 'a>(kind: Status) -> Element<'a, Message, Theme, R> {
    let colour = Role::Status(kind, StatusRole::Fill);
    container(Gap::new())
        .width(Length::Fixed(STATUS_DOT))
        .height(Length::Fixed(STATUS_DOT))
        .class(Surfaced::Fill { fill: colour, text: colour, radius: Radius::Full })
        ._boxed()
}
