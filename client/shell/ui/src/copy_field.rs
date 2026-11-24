//! A value to copy, shown as a field is and never typed into: the value in the monospaced face, a button at its end.

use crate::{Button, Renderer, Surfaced, Theme, text};
use design::{ButtonVariant, CONTROL_HEIGHT, INPUT_RADIUS, Label, Space, Surface, TypeStyle};
use iced_core::text::Wrapping;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, container};

/// `value`, with the button `copy` at its end sending `on_copy`; the caller words the button ("Copy", "✓ Copied").
pub fn copy_field<'a, Message: Clone + 'a, R: Renderer + 'a>(
    value: String,
    copy: Label,
    on_copy: Message,
) -> Element<'a, Message, Theme, R> {
    let shown = text(value, TypeStyle::Mono)
        .wrapping(Wrapping::None)
        .ellipsis(iced_core::text::Ellipsis::End)
        .width(Length::Fill);
    let button = Button::new(copy).variant(ButtonVariant::Primary).compact().on_press(on_copy).view::<R>();
    let line = Row::with_children([shown._boxed(), button]).spacing(Space::S.px()).align_y(Alignment::Center);
    container(line)
        .padding(Padding { left: Space::M.px(), right: Space::Xs.px(), ..Padding::ZERO })
        .height(Length::Fixed(CONTROL_HEIGHT))
        .center_y(Length::Fixed(CONTROL_HEIGHT))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Raised, INPUT_RADIUS))
        ._boxed()
}
