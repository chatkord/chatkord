//! The bar of controls at a card's top right: a pill of glyph buttons, with what one of them opened under it; and the
//! same pill at a stage's bottom centre. Each is a layer the size of what it is over; [`reveal`](crate::reveal)
//! decides when it shows.

use crate::{Button, Icon, Renderer, Surfaced, Theme};
use design::{ButtonVariant, Label, Space, TILE_LEAST};
use iced_core::{Alignment, Element, Length, Widget, alignment};
use iced_widget::{Column, Row, container};

/// One control on the bar: its glyph, its name, and what it sends.
pub struct Tool<Message> {
    /// Its glyph, the only thing it shows.
    pub icon: Icon,
    /// Its name, in its tooltip and for a screen reader.
    pub label: Label,
    /// What it sends when pressed.
    pub message: Message,
    /// Dimmed and pressing nothing, as Volume is on a paused screen until Resume.
    pub held: bool,
}

/// The bar of `tools` for a card `height` tall, with `under` (what a control opened) beneath it, as a layer over the
/// card.
pub fn card_bar<'a, Message: Clone + 'a, R: Renderer + 'a>(
    tools: Vec<Tool<Message>>,
    under: Option<Element<'a, Message, Theme, R>>,
    height: f32,
) -> Element<'a, Message, Theme, R> {
    // The strip under a pinned card is short: its buttons are small, and the bar sits closer to its corner.
    let small = height < TILE_LEAST / 2.0;
    let mut column = vec![pill(tools, small)];
    column.extend(under);
    let inset = if small { Space::Xxs.px() } else { Space::S.px() };
    container(Column::with_children(column).spacing(Space::S.px()).align_x(Alignment::End))
        .align_x(alignment::Horizontal::Right)
        .align_y(alignment::Vertical::Top)
        .padding(inset)
        .width(Length::Fill)
        .height(Length::Fill)
        ._boxed()
}

/// `tool` in the bar's pill at the bottom centre of a stage, as a layer over it: the toggle of the strip under a pinned
/// card.
pub fn stage_toggle<'a, Message: Clone + 'a, R: Renderer + 'a>(tool: Tool<Message>) -> Element<'a, Message, Theme, R> {
    container(pill(vec![tool], false))
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Bottom)
        .padding(Space::S.px())
        .width(Length::Fill)
        .height(Length::Fill)
        ._boxed()
}

/// The pill of glyph buttons both bars draw, `small` on a card in the strip.
fn pill<'a, Message: Clone + 'a, R: Renderer + 'a>(
    tools: Vec<Tool<Message>>,
    small: bool,
) -> Element<'a, Message, Theme, R> {
    let buttons: Vec<Element<'a, Message, Theme, R>> = tools
        .into_iter()
        .map(|tool| {
            let button = Button::new(tool.label).icon_only(tool.icon).variant(ButtonVariant::OnScrim);
            let button = if small { button.small() } else { button.compact() };
            let button = if tool.held { button } else { button.on_press(tool.message) };
            button.view::<R>()
        })
        .collect();
    // In the strip the glyphs touch, as the canvas draws them, so a screen's every control fits its card.
    container(Row::with_children(buttons).spacing(if small { 0.0 } else { Space::Xxs.px() }))
        .padding(Space::Xxs.px())
        .class(Surfaced::ScrimPill)
        ._boxed()
}
