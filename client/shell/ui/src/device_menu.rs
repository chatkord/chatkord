//! The call's list of microphones and speakers: two short lists under their headings, the one in use ticked. Chosen by
//! the arrow beside mute; it is `Layered`, so Escape and a press outside close it.

use crate::layer::{Layered, Placement};
use crate::menu::{MenuChoice, MenuKind, choice_row, menu_panel};
use crate::panel_parts::section_heading;
use crate::{Renderer, Theme};
use design::{DEVICE_MENU_WIDTH, Label, Space};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::container;

/// A list of devices under its heading.
pub struct DeviceSection<Message> {
    /// Microphones or speakers.
    pub heading: Label,
    /// Its devices, in the order shown, the one in use chosen.
    pub rows: Vec<MenuChoice<Message>>,
}

/// The menu `title`d for a screen reader, with `sections`.
pub fn device_menu<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    sections: Vec<DeviceSection<Message>>,
) -> Element<'a, Message, Theme, R> {
    let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for section in sections {
        rows.push(heading(&section.heading));
        rows.extend(section.rows.into_iter().map(choice_row));
    }
    menu_panel(MenuKind::Choices, title, rows, Length::Fill.max(DEVICE_MENU_WIDTH))
}

/// `screen`, with `menu` over it while it is `Some`, its bottom edge at `bottom` and its middle at `centre`.
pub(crate) fn device_menu_over<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    menu: Option<Element<'a, Message, Theme, R>>,
    (centre, bottom): (f32, f32),
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, menu, Placement::Anchored { centre, bottom }, on_close, true)._boxed()
}

/// A section's name, with the room the menu's rows keep around it.
fn heading<'a, Message: 'a, R: Renderer + 'a>(name: &Label) -> Element<'a, Message, Theme, R> {
    container(section_heading(name))
        .padding(Padding {
            top: Space::S.px(),
            bottom: Space::Xs.px(),
            left: Space::S.px() + Space::Xxs.px(),
            right: Space::S.px() + Space::Xxs.px(),
        })
        ._boxed()
}
