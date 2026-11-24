//! A dialog: a panel in the middle of the window over a scrim, its ✕ in the corner. Holding the focus inside it and
//! closing it on Escape or a press outside are `Layered`'s; this is the panel and what puts it over a screen.

use crate::layer::{Layered, Placement, on_panel};
use crate::typography::text;
use crate::{Button, Icon, Labelled, Renderer, Theme};
use design::{ButtonVariant, DIALOG_RADIUS, DIALOG_WIDTH, Label, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget, alignment};
use iced_widget::{Column, Row, container, scrollable, stack};
use words::{Phrase, Words};

/// A dialog's panel around `content`, named `name` for a screen reader, with the ✕ in its corner sending `close`.
pub(crate) fn dialog_frame<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    role: SemanticRole,
    content: Element<'a, Message, Theme, R>,
    close: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    framed(name, role, content, close, DIALOG_WIDTH, words)
}

/// A dialog's panel as [`dialog_frame`], `width` wide.
fn framed<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    role: SemanticRole,
    content: Element<'a, Message, Theme, R>,
    close: Message,
    width: f32,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let shut = Button::new(words.label(Phrase::Close))
        .icon_only(Icon::Close)
        .variant(ButtonVariant::Ghost)
        .compact()
        .on_press(close)
        .view::<R>();
    // The ✕'s middle level with the title's, near the corner.
    let corner = container(shut).align_right(Length::Fill).padding(Padding {
        top: Space::M.px(),
        right: Space::M.px(),
        ..Padding::ZERO
    });
    // As wide as it is drawn where the window allows, and what does not fit its height scrolls inside it.
    let scrolled =
        scrollable(content).direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()));
    let panel = on_panel(scrolled._boxed(), Space::Xl, DIALOG_RADIUS);
    let framed = container(stack([panel, corner._boxed()])).width(Length::Fill.max(width));
    Labelled::new(framed, Semantics { role, label: name })._boxed()
}

/// A panel for a dialog: its `title` (also its name for a screen reader), what it says, and its actions on the right.
pub fn dialog_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    body: Element<'a, Message, Theme, R>,
    actions: Vec<Element<'a, Message, Theme, R>>,
    close: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    titled(SemanticRole::Dialog, title, body, actions, close, DIALOG_WIDTH, words)
}

/// A panel for a dialog as [`dialog_panel`], `width` wide: one that holds more than words.
pub(crate) fn wide_dialog_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    body: Element<'a, Message, Theme, R>,
    actions: Vec<Element<'a, Message, Theme, R>>,
    close: Message,
    width: f32,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    titled(SemanticRole::Dialog, title, body, actions, close, width, words)
}

/// A dialog that asks to confirm what cannot be undone: drawn as [`dialog_panel`], said as an alert.
pub(crate) fn alert_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    body: Element<'a, Message, Theme, R>,
    actions: Vec<Element<'a, Message, Theme, R>>,
    close: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    titled(SemanticRole::AlertDialog, title, body, actions, close, DIALOG_WIDTH, words)
}

fn titled<'a, Message: Clone + 'a, R: Renderer + 'a>(
    role: SemanticRole,
    title: Label,
    body: Element<'a, Message, Theme, R>,
    actions: Vec<Element<'a, Message, Theme, R>>,
    close: Message,
    width: f32,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    // Clear of the ✕.
    let heading = container(text(title.as_str().to_string(), TypeStyle::Title))
        .padding(Padding { right: Space::Xxl.px(), ..Padding::ZERO });
    let actions = Row::with_children(actions).spacing(Space::S.px()).align_y(Alignment::Center);
    let buttons = container(actions.wrap().vertical_spacing(Space::S.px()))
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Right)
        .padding(Padding { top: Space::Xs.px(), ..Padding::ZERO });
    let content = Column::with_children([heading._boxed(), body, buttons._boxed()]).spacing(design::DIALOG_GAP);
    framed(title, role, content._boxed(), close, width, words)
}

/// `screen`, with `dialog` over it while it is `Some`. `on_close` is what Escape and a press outside send.
pub fn dialog<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    dialog: Option<Element<'a, Message, Theme, R>>,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, dialog, Placement::Centred, on_close, false)._boxed()
}
