//! A field that shows the choice taken among a few and opens a menu of them under it, as a text field is drawn. The
//! menu is `Layered`, so Escape and a press outside close it and the focus comes back to the field; whether it is open
//! is the caller's, told by messages.

use crate::button::{ButtonClass, actionable};
use crate::layer::{Layered, Placement};
use crate::menu::{Leading, MenuChoice, MenuKind, choice_row, menu_panel};
use crate::typography::line;
use crate::{Icon, IconClass, Renderer, Theme, icon, tooltip};
use design::{CONTROL_HEIGHT, Content, IconSize, Label, Popup, Role, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, button};

/// A field and the menu it opens.
pub struct Select<Message> {
    /// What the field and its menu are named for a screen reader.
    pub name: Label,
    /// What stands before the choice taken, where the field has something.
    pub leading: Option<Leading>,
    /// The choice taken, as the field shows it.
    pub shown: Label,
    /// Every choice, the one taken marked, in the order the menu lists them.
    pub choices: Vec<MenuChoice<Message>>,
    /// Whether the menu is open.
    pub open: bool,
    /// What a press on the field sends: the menu opens, or closes while it is open.
    pub toggle: Message,
    /// What Escape or a press outside the open menu sends.
    pub close: Message,
}

impl<Message: Clone> Select<Message> {
    /// The field, the room's width, with its menu under it while it is open.
    pub fn view<'a, R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R>
    where
        Message: 'a,
    {
        let Select { name, leading, shown, choices, open, toggle, close } = self;
        let style = TypeStyle::Body;
        let secondary = IconClass::Role(Role::Content(Content::Secondary));
        let content = Row::with_children(leading.map(Leading::view).into_iter().chain([
            line(shown.as_str().to_string(), style).width(Length::Fill)._boxed(),
            icon(Icon::ChevronDown, IconSize::S, secondary)._boxed(),
        ]))
        .spacing(Space::S.px() + Space::Xxs.px())
        .align_y(Alignment::Center);
        let inner = button::Button::new(content)
            .class(ButtonClass::Field)
            .padding(Padding {
                top: (CONTROL_HEIGHT - style.line_height()) / 2.0,
                bottom: (CONTROL_HEIGHT - style.line_height()) / 2.0,
                left: Space::M.px() + Space::Xxs.px(),
                right: Space::M.px(),
            })
            .width(Length::Fill)
            .on_press(toggle.clone());
        // The field is read by its name and the choice it shows, as a row with a note is.
        let said = Label::new(&format!("{} {}", name.as_str(), shown.as_str())).unwrap_or_else(|_| name.clone());
        let field = actionable(
            inner,
            Semantics { role: SemanticRole::Opens { layer: Popup::Menu, open }, label: said },
            Some(toggle),
        );
        let field = tooltip(field, &shown, iced_widget::tooltip::Position::Top);
        let menu = open
            .then(|| menu_panel(MenuKind::Choices, name, choices.into_iter().map(choice_row).collect(), Length::Fill));
        Layered::new(field, menu, Placement::Below { gap: Space::Xs.px() + Space::Xxs.px() }, close, true)._boxed()
    }
}
