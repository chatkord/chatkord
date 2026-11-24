//! A person's tile in a voice channel, drawn from their `screens::Tile`, and the grid of them. The ring while
//! they are heard is a second voice: the label a screen reader hears says it too.

use crate::typography::{line, text};
use crate::{AvatarKind, Focus, Icon, IconClass, Labelled, Renderer, Surfaced, Theme, avatar, icon};
use design::{
    AvatarSize, IconSize, Radius, Role, SPEAKING_GLOW, SemanticRole, Semantics, Space, Surface, TILE_LEAST, TypeStyle,
};
use iced_core::keyboard::{Key, key::Named};
use iced_core::{Alignment, Element, Length, Padding, Widget, alignment};
use iced_widget::{Column, Row, Stack, container, scrollable};
use screens::{Connection, Tile, grid, presence_phrase};
use words::{Phrase, Words};

/// The pill at the bottom of a tile: the name, then what the person is doing as marks, in the order of the design.
fn pill<'a, Message: 'a, R: Renderer + 'a>(person: &'a Tile, words: &'a Words) -> Element<'a, Message, Theme, R> {
    let mut cells: Vec<Element<'a, Message, Theme, R>> =
        vec![line(person.name.clone(), TypeStyle::SmallStrong)._boxed()];
    // Sharing is a screen glyph, as Sharkord marks it: the screen itself is the card beside.
    if person.sharing {
        cells.push(icon(Icon::Monitor, IconSize::S, IconClass::Role(Role::OnScrim))._boxed());
    }
    let off = |glyph: Icon| icon(glyph, IconSize::S, IconClass::Role(Role::OnScrimDanger))._boxed();
    match person.connection {
        Connection::Connected => {
            // Deafening silences the microphone too: both are shown, as the person's state is both.
            if person.muted {
                cells.push(off(Icon::MicOff));
            }
            // A mute for everyone is not the person's own: it is said in words, so nothing rests on the colour.
            // The one muted says so in the stage's notice instead.
            if person.held && !person.own() {
                cells.push(text(words.phrase(Phrase::PresenceMutedByAdmin), TypeStyle::Caption)._boxed());
            }
            if person.deafened {
                cells.push(off(Icon::HeadphonesOff));
            }
        }
        // Nothing else about them is true until the connection is whole: its word stands alone.
        Connection::Connecting | Connection::Failed | Connection::Reconnecting => {
            if let Some(word) = person.badges().first() {
                cells.push(text(words.phrase(presence_phrase(*word)), TypeStyle::Caption)._boxed());
            }
        }
    }
    container(Row::with_children(cells).spacing(Space::S.px()).align_y(Alignment::Center))
        .padding(Padding::from([Space::Xs.px(), Space::M.px() - Space::Xxs.px()]))
        .class(Surfaced::ScrimPill)
        ._boxed()
}

/// The largest face that, ringed and in the middle of a card `height` tall, clears the name pill at the card's foot;
/// the small one where none does, as in the strip.
fn face(height: f32, alone: bool) -> AvatarSize {
    let pill = TypeStyle::SmallStrong.line_height() + 2.0 * Space::Xs.px();
    let room = height - 2.0 * (design::PILL_INSET + pill + SPEAKING_GLOW.width);
    let largest = if alone { AvatarSize::Alone } else { AvatarSize::Tile };
    [largest, AvatarSize::Tile, AvatarSize::Profile, AvatarSize::L, AvatarSize::Rail, AvatarSize::M]
        .into_iter()
        .find(|size| size.px() <= room)
        .unwrap_or(AvatarSize::S)
}

/// A tile of `size` for `person`; `picture` is drawn in place of their face, `chosen` is the round picture they chose,
/// which takes their initials' place, and `alone` says no one else is in the call. With `menu` the tile is a control
/// that opens their popover: a right-click, or Space, Enter or the menu key while it has the focus.
pub fn voice_tile<'a, Message: Clone + 'a, R: Renderer + 'a>(
    person: &'a Tile,
    size: (f32, f32),
    picture: Option<Element<'a, Message, Theme, R>>,
    chosen: Option<Element<'a, Message, Theme, R>>,
    alone: bool,
    menu: Option<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let name = crate::labels::label(&person.name);
    // A picture covers the card's own edge, so its ring is drawn over it as a layer of its own.
    let pictured = picture.is_some();
    let ring_over = pictured && person.ringed();
    let base: Element<'a, Message, Theme, R> = match picture {
        // The stack takes its size from this layer, so the picture fills the card whatever it measures itself.
        Some(picture) => container(picture).width(Length::Fill).height(Length::Fill)._boxed(),
        None => {
            let side = face(size.1, alone);
            let face = match chosen {
                Some(chosen) => {
                    let side = Length::Fixed(side.px());
                    let semantics = Semantics { role: SemanticRole::Image, label: name };
                    Labelled::new(container(chosen).width(side).height(side), semantics)._boxed()
                }
                None => avatar::<Message, R>(name, AvatarKind::Called(person.colour), side),
            };
            // The ring is the avatar's own edge, thickened: a gap of nothing between them.
            let ringed = container(face).padding(SPEAKING_GLOW.width).class(if person.ringed() {
                Surfaced::Speaking(Radius::Full)
            } else {
                Surfaced::Clear
            });
            container(ringed).center(Length::Fill)._boxed()
        }
    };
    let bottom = container(pill::<Message, R>(person, words))
        .align_y(alignment::Vertical::Bottom)
        .align_x(alignment::Horizontal::Left)
        .padding(design::PILL_INSET)
        .width(Length::Fill)
        .height(Length::Fill);
    let mut layers = vec![base, bottom._boxed()];
    if ring_over {
        layers.push(
            container(iced_widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .class(Surfaced::Ring(design::TILE_RADIUS))
                ._boxed(),
        );
    }
    let face = container(Stack::with_children(layers))
        .width(Length::Fixed(size.0))
        .height(Length::Fixed(size.1))
        .clip(true)
        .class(match pictured {
            true => Surfaced::Screen,
            false => Surfaced::Tile { colour: person.colour, ringed: person.ringed() },
        });
    let said = crate::labels::label(&person.said(words));
    match menu {
        None => Labelled::new(face, Semantics { role: SemanticRole::ListItem, label: said })._boxed(),
        Some(message) => {
            let keyed = message.clone();
            Focus::new(
                iced_widget::mouse_area(face).on_right_press(message.clone()),
                Semantics { role: SemanticRole::Button, label: said },
            )
            .on_activate(message)
            .on_key(move |key| matches!(key, Key::Named(Named::ContextMenu)).then(|| keyed.clone()))
            ._boxed()
        }
    }
}

/// A call's cards, stretched to fill the space, scrolling where there is no room for each at the least size worth
/// drawing. `cell` draws one card at the size the grid gives it, told whether it is the only one.
pub fn voice_grid<'a, T: Copy, Message: Clone + 'a, R: Renderer + 'a>(
    cards: &[T],
    width: f32,
    height: f32,
    mut cell: impl FnMut(T, (f32, f32), bool) -> Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let gap = Space::M.px();
    let Some(layout) = grid(cards.len(), width, height, gap, TILE_LEAST) else {
        return container(iced_widget::Space::new())._boxed();
    };
    let tile = layout.tile;
    let alone = cards.len() == 1;
    let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for chunk in cards.chunks(layout.columns) {
        let cells: Vec<_> = chunk.iter().map(|card| cell(*card, tile, alone)).collect();
        rows.push(Row::with_children(cells).spacing(gap)._boxed());
    }
    let body = Column::with_children(rows).spacing(gap).width(Length::Shrink);
    let stage = container(body).center(Length::Fill).class(Surfaced::On(Surface::Base));
    if layout.fits {
        stage._boxed()
    } else {
        scrollable(Column::with_children([stage._boxed()]).width(Length::Fill))
            .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
            ._boxed()
    }
}
