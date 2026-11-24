//! The line under a field, in a form, a dialog or a page: 12 px, a refusal in the danger colour after an alert circle,
//! a warning stressed in the warning colour after a triangle, a help line muted with no icon. Never a toast.

use crate::{Icon, IconClass, Renderer, TextClass, Theme, coloured, icon};
use design::{Content, Role, Space, Status, StatusRole, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::Row;
use screens::{FieldSaid, FieldTone};
use words::Words;

/// A line of `tone` saying `said`.
pub(crate) fn field_message<'a, Message: 'a, R: Renderer + 'a>(
    tone: FieldTone,
    said: String,
) -> Element<'a, Message, Theme, R> {
    match tone {
        FieldTone::Refusal => marked(Icon::CircleAlert, Status::Danger, TypeStyle::Caption, said),
        FieldTone::Help => {
            coloured(said, TypeStyle::Caption, TextClass::Content(Content::Secondary)).width(Length::Fill)._boxed()
        }
    }
}

/// `said` after `mark`, both in `status`'s colour.
fn marked<'a, Message: 'a, R: Renderer + 'a>(
    mark: Icon,
    status: Status,
    style: TypeStyle,
    said: String,
) -> Element<'a, Message, Theme, R> {
    let mark = icon(mark, design::IconSize::Field, IconClass::Role(Role::Status(status, StatusRole::Text)));
    Row::with_children([mark._boxed(), coloured(said, style, TextClass::Status(status)).width(Length::Fill)._boxed()])
        .spacing(Space::Xs.px() + Space::Xxs.px())
        .align_y(Alignment::Start)
        ._boxed()
}

/// A warning saying `said`: what must be done now, as a secret shown once.
pub(crate) fn warning<'a, Message: 'a, R: Renderer + 'a>(said: String) -> Element<'a, Message, Theme, R> {
    marked(Icon::TriangleAlert, Status::Warning, TypeStyle::CaptionStrong, said)
}

/// What `said` is under a field, in `words`.
pub fn field_said<'a, Message: 'a, R: Renderer + 'a>(
    said: &FieldSaid,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    field_message(said.tone, words.say(&said.said))
}

/// A refusal saying `said`: the line a failed ask leaves under its field.
pub(crate) fn refusal<'a, Message: 'a, R: Renderer + 'a>(said: String) -> Element<'a, Message, Theme, R> {
    field_message(FieldTone::Refusal, said)
}
