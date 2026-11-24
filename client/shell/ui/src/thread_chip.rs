//! The chip under a message that has a thread: how many replies and when the last came, a dot before it while the
//! thread is followed and has unread replies, and a press that opens the thread.

use crate::button::{ButtonClass, actionable};
use crate::typography::coloured;
use crate::{Renderer, TextClass, Theme, unread_dot};
use design::{CHIP_GAP, CHIP_HEIGHT, Label, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::text::Wrapping;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, button, container};

/// The chip saying `said`, which a screen reader hears as `heard` (with the dot's meaning added), `unread` with the dot;
/// a press sends `open`.
pub fn thread_chip<'a, Message: Clone + 'a, R: Renderer + 'a>(
    said: String,
    heard: &Label,
    unread: bool,
    open: Message,
) -> Element<'a, Message, Theme, R> {
    let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if unread {
        cells.push(unread_dot());
    }
    cells.push(coloured(said, TypeStyle::SmallStrong, TextClass::Accent).wrapping(Wrapping::None)._boxed());
    let content = Row::with_children(cells).spacing(CHIP_GAP).align_y(Alignment::Center);
    let inner = button(container(content).center_y(Length::Fixed(CHIP_HEIGHT)))
        .class(ButtonClass::Chip { mine: false })
        .padding(Padding::from([0.0, Space::S.px() + Space::Xxs.px()]))
        .on_press(open.clone());
    let pressable = actionable(inner, Semantics { role: SemanticRole::Button, label: heard.clone() }, Some(open));
    crate::tooltip(pressable, heard, iced_widget::tooltip::Position::Top)
}
