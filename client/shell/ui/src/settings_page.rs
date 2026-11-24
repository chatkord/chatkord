//! The pieces every page of settings is drawn from, a server's and a person's, so no page restyles one: the page's
//! body, a field's name above it, a section (a heading, its action at the right, a card under it) and a row of a
//! card (a semibold title over a line, the control at the right).

use crate::{Labelled, Renderer, RowTone, TextClass, Theme, coloured, text};
use design::{Content, Label, SETTINGS_LIST_WIDTH, SemanticRole, Semantics, Space, Status, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container};

/// A page's body: its parts one under another, as wide as a settings column.
pub fn settings_body<'a, Message: 'a, R: Renderer + 'a>(
    parts: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    Column::with_children(parts).spacing(Space::L.px()).width(Length::Fill.max(SETTINGS_LIST_WIDTH))._boxed()
}

/// A field's name, set above the field it names.
pub fn field_label<'a, Message: 'a, R: Renderer + 'a>(name: String) -> Element<'a, Message, Theme, R> {
    coloured(name, TypeStyle::Label, TextClass::Content(Content::Primary))._boxed()
}

/// A section under its `heading`, with its `action` at the right of that line: the `card` of its rows, or its list.
pub fn settings_section<'a, Message: 'a, R: Renderer + 'a>(
    heading: Label,
    action: Option<Element<'a, Message, Theme, R>>,
    card: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let title = Labelled::new(
        text(heading.as_str().to_string(), TypeStyle::Subheading).width(Length::Fill),
        Semantics { role: SemanticRole::Heading, label: heading },
    );
    let line = Row::with_children(std::iter::once(title._boxed()).chain(action))
        .spacing(Space::S.px())
        .align_y(Alignment::Center);
    Column::with_children([line._boxed(), card]).spacing(Space::L.px())._boxed()
}

/// A row of a section's card: its `title` over the `line` that says what the title cannot, and its `control` at the
/// right, with the room every settings row has (a card of them is a `flush_card`). A `Danger` row is set in the
/// danger colour.
pub fn settings_row<'a, Message: 'a, R: Renderer + 'a>(
    title: String,
    line: Option<String>,
    control: Element<'a, Message, Theme, R>,
    tone: RowTone,
) -> Element<'a, Message, Theme, R> {
    let class = match tone {
        RowTone::Danger => TextClass::Status(Status::Danger),
        RowTone::Plain | RowTone::Quiet | RowTone::Chosen => TextClass::Content(Content::Primary),
    };
    let line = line.map(|line| coloured(line, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed());
    let words =
        Column::with_children(std::iter::once(coloured(title, TypeStyle::BodyStrong, class)._boxed()).chain(line))
            .spacing(Space::Xxs.px())
            .width(Length::Fill);
    container(Row::with_children([words._boxed(), control]).spacing(Space::M.px()).align_y(Alignment::Center))
        .padding(crate::ROW_PADDING)
        .width(Length::Fill)
        ._boxed()
}
