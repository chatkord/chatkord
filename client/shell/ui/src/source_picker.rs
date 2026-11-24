//! Our own picker of what to share, where the system has none: a dialog of Screens and Applications as cards of the
//! source's still, painted by the host, and its name; a press selects a card and Share shares it.

use crate::button::{ButtonClass, actionable};
use crate::labels::{label, phrase};
use crate::picture::Painted;
use crate::{Button, Renderer, Surfaced, TextClass, Theme, coloured, text};
use design::{
    ButtonVariant, Content, PICKER_GRID_HEIGHT, PICKER_WIDTH, Role, SOURCE_ICON, SOURCE_ICON_RADIUS,
    SOURCE_STILL_RADIUS, SemanticRole, Semantics, Space, Surface, TILE_LEAST, TypeStyle,
};
use iced_core::text::{Ellipsis, Wrapping};
use iced_core::{Alignment, Element, Length, Padding, Size, Widget};
use iced_widget::scrollable::Direction;
use iced_widget::{Column, Row, Space as Gap, button, container, scrollable};
use screens::{SourceCard, SourceKey, Sources, SourcesTab};
use words::{Phrase, Words};

/// The room inside a card, round its picture and its name.
const CARD_PAD: f32 = Space::S.px();

/// How the cards stand in a picker over a window: two across while each keeps [`TILE_LEAST`], else one, a card's
/// picture 16 by 9 of its width.
struct Cards {
    columns: usize,
    /// The picture's height.
    still: f32,
    /// A card's height: its room, picture, name's line and the gap between them.
    height: f32,
}

impl Cards {
    /// The cards in a picker over a window `width` wide: the picker is its width there, less a gutter each side.
    fn in_window(width: f32) -> Cards {
        let inside = PICKER_WIDTH.min(width - 2.0 * Space::L.px()) - 2.0 * Space::Xl.px();
        let pair = (inside - Space::L.px()) / 2.0;
        let (columns, wide) = if pair >= TILE_LEAST { (2, pair) } else { (1, inside) };
        let still = ((wide - 2.0 * CARD_PAD) * 9.0 / 16.0).max(0.0);
        Cards { columns, still, height: 2.0 * CARD_PAD + still + Space::S.px() + Space::Xxs.px() + SOURCE_ICON }
    }
}
/// What the dialog round the grid takes of the window's height: its room, title, tabs, actions and the gaps.
const CHROME: f32 = 2.0 * Space::Xl.px() + 4.0 * Space::L.px() + 3.0 * design::CONTROL_HEIGHT + 2.0 * Space::Xl.px();

/// What a press in the picker sends.
pub struct Sends<'a, Message> {
    /// Shows a tab: the screens, or the applications' windows.
    pub tab: Box<dyn Fn(SourcesTab) -> Message + 'a>,
    /// Chooses a source, ready to share.
    pub select: Box<dyn Fn(SourceKey) -> Message + 'a>,
    /// Shares the chosen source.
    pub share: Message,
    /// Closes the picker without sharing.
    pub close: Message,
}

/// The picker over a window of `window`'s size, its pictures painted by `picture`.
pub fn source_picker<'a, Message: Clone + 'a, R: Renderer + 'a>(
    sources: &Sources,
    window: Size,
    sends: Sends<'a, Message>,
    picture: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let tabs = [(Phrase::PickScreens, SourcesTab::Screens), (Phrase::PickApplications, SourcesTab::Applications)];
    let chosen = tabs.iter().position(|(_, tab)| *tab == sources.tab()).unwrap_or(0);
    let tabs = crate::choice_track(
        phrase(Phrase::ChooseWhatToShare, words),
        tabs.into_iter().map(|(named, tab)| (phrase(named, words), (sends.tab)(tab))).collect(),
        chosen,
    );
    let cards = sources.cards(words);
    let laid = Cards::in_window(window.width);
    let rows = cards.len().div_ceil(laid.columns);
    let lines: Vec<Element<'a, Message, Theme, R>> = cards
        .chunks(laid.columns)
        .map(|pair| {
            let mut row: Vec<Element<'a, Message, Theme, R>> = pair
                .iter()
                .map(|card| source_card(card, laid.still, (sends.select)(card.key.clone()), picture))
                .collect();
            if row.len() < laid.columns {
                row.push(Gap::new().width(Length::Fill)._boxed());
            }
            Row::with_children(row).spacing(Space::L.px())._boxed()
        })
        .collect();
    let content = rows as f32 * laid.height + rows.saturating_sub(1) as f32 * Space::L.px();
    let room = PICKER_GRID_HEIGHT.min(window.height - CHROME).max(laid.height);
    let grid = scrollable(Column::with_children(lines).spacing(Space::L.px()))
        .direction(Direction::Vertical(crate::theme::scroll_bar()))
        .height(Length::Fixed(content.min(room)));
    let share = Button::new(phrase(Phrase::ShareThis, words)).variant(ButtonVariant::Primary);
    let share = match sources.selected() {
        Some(_) => share.on_press(sends.share),
        None => share,
    };
    let actions = vec![
        Button::new(phrase(Phrase::Cancel, words))
            .variant(ButtonVariant::Secondary)
            .on_press(sends.close.clone())
            .view::<R>(),
        share.view::<R>(),
    ];
    let body = Column::with_children([tabs, grid._boxed()]).spacing(Space::L.px());
    crate::dialog::wide_dialog_panel(
        phrase(Phrase::ChooseWhatToShare, words),
        body._boxed(),
        actions,
        sends.close,
        PICKER_WIDTH,
        words,
    )
}

/// A card: the source's still on the chrome's fill, then its icon, name and note on one line.
fn source_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    card: &SourceCard,
    still_height: f32,
    select: Message,
    picture: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let still =
        card.still.map_or_else(|| Gap::new()._boxed(), |still| picture(Painted::Still(still, SOURCE_STILL_RADIUS)));
    let still = container(still).center_x(Length::Fill).center_y(Length::Fixed(still_height)).clip(true).class(
        Surfaced::Fill {
            fill: Role::Surface(Surface::Chrome),
            text: Role::Content(Content::Primary),
            radius: SOURCE_STILL_RADIUS,
        },
    );
    let mut caption: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if let Some(icon) = card.icon {
        let icon = container(picture(Painted::Still(icon, SOURCE_ICON_RADIUS)))
            .width(Length::Fixed(SOURCE_ICON))
            .height(Length::Fixed(SOURCE_ICON))
            .clip(true)
            .class(Surfaced::Fill {
                fill: Role::Surface(Surface::Raised),
                text: Role::Content(Content::Primary),
                radius: SOURCE_ICON_RADIUS,
            });
        caption.push(icon._boxed());
    }
    caption.push(
        text(card.name.clone(), TypeStyle::SmallStrong)
            .wrapping(Wrapping::None)
            .ellipsis(Ellipsis::End)
            .width(Length::Fill)
            ._boxed(),
    );
    if let Some(note) = &card.note {
        caption.push(coloured(note.clone(), TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed());
    }
    let caption = Row::with_children(caption)
        .spacing(Space::S.px())
        .align_y(Alignment::Center)
        .height(Length::Fixed(SOURCE_ICON));
    let inside = Column::with_children([still._boxed(), caption._boxed()]).spacing(Space::S.px() + Space::Xxs.px());
    // The ring of a selected card is wider than the edge of the rest: the room inside gives the difference back, so
    // the picture never moves.
    let edge = design::source_card(card.selected, design::Interaction::Idle).edge_width;
    let pad = CARD_PAD + design::BORDER_WIDTH - edge;
    let pressable = button(inside)
        .padding(Padding::new(pad))
        .width(Length::Fill)
        .class(ButtonClass::Card { selected: card.selected })
        .on_press(select.clone());
    let semantics = Semantics { role: SemanticRole::Radio { chosen: card.selected }, label: label(&card.name) };
    actionable(pressable, semantics, Some(select))
}
