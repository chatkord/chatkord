//! The state line: what holds, said on one line across the foot of the area it is about, its rule on its top edge.
//! No box, no action, no ✕: it goes when the state is over.

use crate::labels::label;
use crate::status_dot::status_dot;
use crate::{Labelled, Renderer, Surfaced, Theme, text};
use design::{STATE_LINE_HEIGHT, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use words::Words;

/// `shown` as the line at the foot of an area, as wide as it.
pub fn state_line<'a, Message: 'a, R: Renderer + 'a>(
    shown: &screens::StateLine,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let said = words.say(&shown.said);
    let named = label(&said);
    let row = Row::with_children([status_dot(shown.kind), text(said, TypeStyle::Small)._boxed()])
        .spacing(Space::S.px())
        .align_y(Alignment::Center);
    let body = container(row)
        .padding(Padding { left: Space::L.px(), ..Padding::ZERO })
        .width(Length::Fill)
        .height(Length::Fixed(STATE_LINE_HEIGHT))
        .align_y(Alignment::Center)
        .class(Surfaced::On(design::Surface::Chrome));
    let column = Column::with_children([crate::panel_parts::hairline(), body._boxed()]);
    Labelled::new(column, Semantics { role: SemanticRole::telling(shown.kind), label: named })._boxed()
}

/// The room a state line takes, rule and all.
pub const STATE_LINE_ROOM: f32 = STATE_LINE_HEIGHT + design::BORDER_WIDTH;
