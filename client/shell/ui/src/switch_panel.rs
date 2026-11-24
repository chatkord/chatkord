//! A titled panel of switches hanging from a button, as the call's Voice options are: each a name alone, as Sharkord's.

use crate::layer::on_panel;
use crate::{Labelled, Renderer, Theme, switch, text};
use design::{AUDIO_CONTROLS_WIDTH, Label, MENU_RADIUS, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Element, Length, Widget};
use iced_widget::Column;

/// One switch on the panel: its name, whether it is on, and what flipping it sends.
pub struct SwitchRow<Message> {
    /// Its name, and its name for a screen reader.
    pub name: Label,
    /// Whether it is on.
    pub on: bool,
    /// What flipping it sends.
    pub flip: Message,
}

/// The panel called `title`, its switches in order, as wide as Audio controls beside it.
pub fn switch_panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    rows: Vec<SwitchRow<Message>>,
) -> Element<'a, Message, Theme, R> {
    let heading = text(title.as_str().to_string(), TypeStyle::BodyStrong)._boxed();
    let rows = rows.into_iter().map(|row| switch(&row.name, None, row.on, row.flip));
    let column = Column::with_children(std::iter::once(heading).chain(rows))
        .spacing(Space::S.px() + Space::Xxs.px())
        .width(Length::Fill.max(AUDIO_CONTROLS_WIDTH - 2.0 * Space::L.px()));
    Labelled::new(
        on_panel(column._boxed(), Space::L, MENU_RADIUS),
        Semantics { role: SemanticRole::Dialog, label: title },
    )
    ._boxed()
}
