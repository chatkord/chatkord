//! The card that says what a watched screen is, drawn from a `screens::ShareCard`: whose stream, its size and rate, how
//! this viewer decodes it, marked where that is software, and how many pictures it did not paint. Its lines are the
//! popover's about a picture (`screens::About`): one owner for the words.

use crate::share_panel::line_of;
use crate::typography::text;
use crate::{Labelled, Renderer, Theme, badge, panel};
use design::{Elevation, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row};
use screens::{About, ShareCard};
use words::Words;

/// A watched screen's card in `words`, titled by the sharer's `name` as the place shows them, with the badge of its
/// state, and a line for what is true of it.
pub fn share_card<'a, Message: 'a, R: Renderer + 'a>(
    card: &ShareCard,
    name: &str,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let about = About::watched(card, name, words);
    let title = Row::with_children([
        text(about.title.clone(), TypeStyle::BodyStrong).width(Length::Fill)._boxed(),
        badge::<Message, R>(card.badge(), words),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let lines = std::iter::once(title._boxed())
        .chain(about.lines(words).into_iter().map(|line| line_of::<Message, R>(line.kind, line.said)));
    let lines = Column::with_children(lines).spacing(Space::S.px());
    let said = about.note.map_or_else(
        || format!("{}, {}", about.title, about.size_and_rate),
        |note| format!("{}, {}, {}", about.title, about.size_and_rate, note.said(words)),
    );
    Labelled::new(
        panel(lines._boxed(), Elevation::Raised),
        Semantics { role: SemanticRole::Status, label: crate::labels::label(&said) },
    )
    ._boxed()
}
