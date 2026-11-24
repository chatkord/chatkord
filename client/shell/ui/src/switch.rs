//! A switch: a track with a knob at one end, beside the name of what it turns on and, where its name cannot say it, a
//! line on what that does. One button, reached by Tab and taken by Space or Enter; a screen reader hears a switch and whether it is on.

use crate::button::{ButtonClass, actionable};
use crate::{Renderer, Surfaced, TextClass, Theme, coloured, text};
use design::{
    Content, Interaction, Label, SWITCH_HEIGHT, SWITCH_KNOB, SWITCH_PAD, SWITCH_WIDTH, SemanticRole, Semantics, Space,
    TypeStyle,
};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, button, container};

/// `name`, with its `hint` under it if any, and the switch, which is `on` or not; what taking it sends is `message`.
pub fn switch<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    hint: Option<&str>,
    on: bool,
    message: Message,
) -> Element<'a, Message, Theme, R> {
    switch_offered(name, hint, on, Some(message))
}

/// [`switch`], drawn as one that cannot be used now when there is no `message`: it waits on another choice.
pub fn switch_offered<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    hint: Option<&str>,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message, Theme, R> {
    let hint =
        hint.map(|hint| coloured(hint.to_string(), TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed());
    beside(name, hint.into_iter().collect(), control(name, on, message))
}

/// [`switch`] with `lines` under its name, each its words in a style and class: a hint, a credit, a state.
pub fn switch_noted<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    lines: Vec<(String, TypeStyle, TextClass)>,
    on: bool,
    message: Message,
) -> Element<'a, Message, Theme, R> {
    let lines = lines.into_iter().map(|(line, style, class)| coloured(line, style, class)._boxed()).collect();
    beside(name, lines, control(name, on, Some(message)))
}

/// The track and its knob, `on` or not, named `name` for a screen reader; taken, it sends `message`.
fn control<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    on: bool,
    message: Option<Message>,
) -> Element<'a, Message, Theme, R> {
    let knob = container(iced_widget::Space::new())
        .width(Length::Fixed(SWITCH_KNOB))
        .height(Length::Fixed(SWITCH_KNOB))
        .class(Surfaced::Knob(design::switch(
            on,
            if message.is_some() { Interaction::Idle } else { Interaction::Disabled },
        )));
    let track = if on { container(knob).align_right(Length::Fill) } else { container(knob).align_left(Length::Fill) };
    let control = button::Button::new(track)
        .class(ButtonClass::Switch { on })
        .padding(SWITCH_PAD)
        .width(Length::Fixed(SWITCH_WIDTH))
        .height(Length::Fixed(SWITCH_HEIGHT))
        .on_press_maybe(message.clone());
    actionable(control, Semantics { role: SemanticRole::Switch { on }, label: name.clone() }, message)
}

/// `name` over `lines`, `control` at their right.
fn beside<'a, Message: 'a, R: Renderer + 'a>(
    name: &Label,
    lines: Vec<Element<'a, Message, Theme, R>>,
    control: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let words = Column::with_children(
        std::iter::once(text(name.as_str().to_string(), TypeStyle::BodyStrong)._boxed()).chain(lines),
    )
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    Row::with_children([words._boxed(), control]).spacing(Space::M.px()).align_y(Alignment::Center)._boxed()
}
