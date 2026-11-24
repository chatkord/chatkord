//! The tools over a hovered message, as Discord's: React, Reply and More on a small raised bar at the row's top right,
//! each an icon with its name in a tooltip.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::{Icon, IconClass, Renderer, Surfaced, Theme, icon, tooltip};
use design::{Content, Elevation, HOVER_TOOL, HOVER_TOOLS_PAD, IconSize, Popup, Radius, Role, SemanticRole, Semantics};
use iced_core::{Element, Length, Widget};
use iced_widget::tooltip::Position;
use iced_widget::{Row, button, container};
use words::{Phrase, Words};

/// What the tools over a hovered message send: a reaction picked for it, a reply to it, and its menu.
#[derive(Clone, Debug)]
pub struct HoverTools<Message> {
    /// Opens the emoji picker for this message; none where reacting is not allowed.
    pub react: Option<Message>,
    /// Starts a reply to it; none where sending is not allowed.
    pub reply: Option<Message>,
    /// Opens its menu, which is open while `menu_open`.
    pub more: Message,
    /// Its menu is open now.
    pub menu_open: bool,
}

/// The bar of `tools`, said in `words`: only the tools that are given, More always last.
pub fn hover_bar<'a, Message: Clone + 'a, R: Renderer + 'a>(
    tools: HoverTools<Message>,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if let Some(react) = tools.react {
        let role = SemanticRole::Opens { layer: Popup::Dialog, open: false };
        cells.push(tool(Icon::SmilePlus, Phrase::React, role, react, words));
    }
    if let Some(reply) = tools.reply {
        cells.push(tool(Icon::Reply, Phrase::Reply, SemanticRole::Button, reply, words));
    }
    let role = SemanticRole::Opens { layer: Popup::Menu, open: tools.menu_open };
    cells.push(tool(Icon::More, Phrase::MoreActions, role, tools.more, words));
    container(Row::with_children(cells).spacing(HOVER_TOOLS_PAD))
        .padding(HOVER_TOOLS_PAD)
        .class(Surfaced::Panel(Elevation::Overlay, Radius::M))
        ._boxed()
}

fn tool<'a, Message: Clone + 'a, R: Renderer + 'a>(
    glyph: Icon,
    name: Phrase,
    role: SemanticRole,
    message: Message,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let label = crate::labels::phrase(name, words);
    let shown = icon(glyph, IconSize::Bar, IconClass::Role(Role::Content(Content::Secondary)));
    let inner = button::Button::new(container(shown).center(Length::Fixed(HOVER_TOOL)))
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(0)
        .on_press(message.clone());
    let pressable = actionable(inner, Semantics { role, label: label.clone() }, Some(message));
    tooltip(pressable, &label, Position::Top)
}
