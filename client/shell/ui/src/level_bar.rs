//! How loud the microphone is: a bar on an inset track, filled in the success colour from 0 to 100, with a thin mark
//! where the noise gate opens when it is on. It is a picture of a number a person reads elsewhere, so it says nothing
//! to a screen reader.

use crate::{ProgressClass, Renderer, Surfaced, Theme};
use design::{Content, LEVEL_BAR_HEIGHT, LEVEL_MARK_WIDTH, Radius, Role};
use iced_core::{Element, Length, Widget};
use iced_widget::{Row, Space, container, progress_bar, stack};

/// The bar at `level` percent; past 100 it is full.
pub fn level_bar<'a, Message: 'a, R: Renderer + 'a>(level: u8) -> Element<'a, Message, Theme, R> {
    progress_bar(0.0..=100.0, f32::from(level.min(100)))
        .length(Length::Fill)
        .girth(LEVEL_BAR_HEIGHT)
        .class(ProgressClass::Level)
        ._boxed()
}

/// [`level_bar`] with a mark at `mark` percent along it, when there is one.
pub fn level_bar_marked<'a, Message: 'a, R: Renderer + 'a>(
    level: u8,
    mark: Option<u8>,
) -> Element<'a, Message, Theme, R> {
    let Some(mark) = mark.map(|mark| u16::from(mark.min(100))) else { return level_bar(level) };
    let line = container(Space::new().width(Length::Fixed(LEVEL_MARK_WIDTH)).height(Length::Fixed(LEVEL_BAR_HEIGHT)))
        .class(Surfaced::Fill {
            fill: Role::Content(Content::Primary),
            text: Role::Content(Content::Primary),
            radius: Radius::None,
        });
    let placed = Row::with_children([
        Space::new().width(Length::FillPortion(mark.max(1)))._boxed(),
        line._boxed(),
        Space::new().width(Length::FillPortion((100 - mark).max(1)))._boxed(),
    ])
    .width(Length::Fill);
    stack([level_bar(level), placed._boxed()]).width(Length::Fill)._boxed()
}
