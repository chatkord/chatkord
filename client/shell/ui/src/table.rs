//! A table: named columns, of equal width or each its share, over rows set apart by hairlines, in a card with a
//! hairline edge, a cell holding whatever it is given. A list card is the same card with rows of any shape and no heads.

use crate::{Labelled, Renderer, Surfaced, TextClass, Theme, coloured};
use design::{CARD_RADIUS, Content, Label, SemanticRole, Semantics, Space, Surface, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};

/// The table named `name`: its column `heads`, then each row's cells in the same order. `empty` is said in place of
/// the rows when there are none.
pub fn table<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    heads: Vec<String>,
    rows: Vec<Vec<Element<'a, Message, Theme, R>>>,
    empty: Option<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    weighted_table(name, heads.into_iter().map(|head| (head, 1)).collect(), rows, empty)
}

/// [`table`] with columns of unequal width: each head with its share of the width beside the others'.
pub(crate) fn weighted_table<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    heads: Vec<(String, u16)>,
    rows: Vec<Vec<Element<'a, Message, Theme, R>>>,
    empty: Option<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let shares: Vec<u16> = heads.iter().map(|(_, share)| *share).collect();
    let head_cells = heads.into_iter().map(|(head, share)| {
        coloured(head.to_uppercase(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary))
            .width(Length::FillPortion(share))
            ._boxed()
    });
    let head = container(Row::with_children(head_cells).spacing(Space::M.px()))
        .padding(Padding::from([Space::S.px(), Space::M.px() + Space::Xxs.px()]))
        .width(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
    let rows = rows
        .into_iter()
        .map(|cells| {
            let cells = cells
                .into_iter()
                .zip(shares.iter().copied().chain(std::iter::repeat(1)))
                .map(|(cell, share)| container(cell).width(Length::FillPortion(share))._boxed());
            Row::with_children(cells).spacing(Space::M.px()).align_y(Alignment::Center)._boxed()
        })
        .collect();
    card(Semantics { role: SemanticRole::Table, label: name }, Some(head._boxed()), rows, empty)
}

/// A list named `name` in the table's card: each row whatever it is given, set apart by hairlines, and `empty` said
/// in place of the rows when there are none.
pub(crate) fn list_card<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    rows: Vec<Element<'a, Message, Theme, R>>,
    empty: Option<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    card(Semantics { role: SemanticRole::List, label: name }, None, rows, empty)
}

/// A list named `name` in the table's card under a `head` of its own. Each row is a control that brings its own room,
/// so the card pads it only a little.
pub(crate) fn headed_card<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    head: Element<'a, Message, Theme, R>,
    rows: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let mut lines: Vec<Element<'a, Message, Theme, R>> = vec![
        container(head)
            .padding(Padding::from([Space::M.px(), Space::M.px() + Space::Xxs.px()]))
            .width(Length::Fill)
            ._boxed(),
    ];
    for row in rows {
        lines.push(crate::panel_parts::hairline());
        lines.push(container(row).padding(Space::Xs.px()).width(Length::Fill)._boxed());
    }
    framed(lines, Semantics { role: SemanticRole::List, label: name })
}

/// A list named `name` in the table's card whose rows are controls that bring their own room, set apart by hairlines.
pub(crate) fn flush_card<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    rows: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let mut lines: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for row in rows {
        if !lines.is_empty() {
            lines.push(crate::panel_parts::hairline());
        }
        lines.push(row);
    }
    framed(lines, Semantics { role: SemanticRole::List, label: name })
}

fn card<'a, Message: 'a, R: Renderer + 'a>(
    said: Semantics,
    head: Option<Element<'a, Message, Theme, R>>,
    rows: Vec<Element<'a, Message, Theme, R>>,
    empty: Option<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    let mut lines: Vec<Element<'a, Message, Theme, R>> = head.into_iter().collect();
    let none = rows.is_empty();
    for row in rows {
        if !lines.is_empty() {
            lines.push(crate::panel_parts::hairline());
        }
        lines.push(container(row).padding(ROW_PADDING).width(Length::Fill)._boxed());
    }
    if let (true, Some(empty)) = (none, empty) {
        if !lines.is_empty() {
            lines.push(crate::panel_parts::hairline());
        }
        lines.push(container(empty).padding(Padding::from([Space::L.px(), Space::M.px() + Space::Xxs.px()]))._boxed());
    }
    framed(lines, said)
}

/// A setting's row in a list card: what [`list_card`] pads each of its rows with, for a card that holds both.
pub(crate) const ROW_PADDING: Padding = Padding {
    top: Space::M.px(),
    bottom: Space::M.px(),
    left: Space::M.px() + Space::Xxs.px(),
    right: Space::M.px() + Space::Xxs.px(),
};

/// The table's card: set apart from the page on the inset surface, as the canvas lifts its cards.
fn framed<'a, Message: 'a, R: Renderer + 'a>(
    lines: Vec<Element<'a, Message, Theme, R>>,
    said: Semantics,
) -> Element<'a, Message, Theme, R> {
    Labelled::new(card_around(Column::with_children(lines)._boxed()), said)._boxed()
}

/// The card round a list that names itself to a screen reader: a list a person puts in order.
pub(crate) fn card_around<'a, Message: 'a, R: Renderer + 'a>(
    list: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    container(list).width(Length::Fill).clip(true).class(Surfaced::Framed(Surface::Inset, CARD_RADIUS))._boxed()
}
