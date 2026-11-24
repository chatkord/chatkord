//! A row of a list card that folds open: its name and a note under it on the left, a value and a chevron on the
//! right, the whole row one control; open, what it holds shows under it.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::{Icon, IconClass, Renderer, TextClass, Theme, coloured, icon};
use design::{Content, IconSize, Role, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, button, container};

/// The row `name`, with `note` under it and `value` at its end, which `toggle` folds open or shut; `inside` is what
/// it holds, shown under it while it is open.
pub(crate) fn fold<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &'a str,
    note: &'a str,
    value: &'a str,
    toggle: Message,
    inside: Option<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let words = Column::with_children([
        coloured(name, TypeStyle::BodyStrong, TextClass::Content(Content::Primary))._boxed(),
        coloured(note, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    let chevron = if inside.is_some() { Icon::ChevronUp } else { Icon::ChevronDown };
    let head = Row::with_children([
        words._boxed(),
        coloured(value, TypeStyle::MonoCaption, TextClass::Content(Content::Secondary))._boxed(),
        icon(chevron, IconSize::S, IconClass::Role(Role::Content(Content::Primary)))._boxed(),
    ])
    .spacing(Space::M.px() + Space::Xxs.px())
    .align_y(Alignment::Center);
    let pressed = button::Button::new(head)
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(crate::ROW_PADDING)
        .width(Length::Fill)
        .on_press(toggle.clone());
    // Heard whole, as it reads: what it is, what it does, its value.
    let said = crate::labels::label(&format!("{name} {note} {value}"));
    let head = actionable(pressed, Semantics { role: SemanticRole::Button, label: said }, Some(toggle));
    let Some(inside) = inside else { return head };
    let side = Space::M.px() + Space::Xxs.px();
    let inside = container(inside).padding(Padding { top: 0.0, bottom: Space::M.px(), left: side, right: side });
    Column::with_children([head, inside._boxed()]).width(Length::Fill)._boxed()
}
