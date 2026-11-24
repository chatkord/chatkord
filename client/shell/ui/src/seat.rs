//! Someone listed under a voice channel in the side pane, drawn from their `screens::Tile`: their picture, their name,
//! and their marks (a camera, a screen, a crossed microphone, crossed headphones), or that they are on another device.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::typography::{coloured, text};
use crate::{AvatarKind, Held, Icon, IconClass, Renderer, Surfaced, TextClass, Theme, avatar_pictured, icon};
use design::{
    AvatarSize, Content, IconSize, Label, PERSON_ROW_HEIGHT, PERSON_ROW_INDENT, Radius, Role, SemanticRole, Semantics,
    Space, Status, StatusRole, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, button, container};
use screens::Tile;
use words::{Phrase, Words};

/// One person's row, with the `picture` they chose where the host holds one. A press does `on_press` where there is
/// something to do (the seat on another device is taken back here); `on_menu`, someone else in the call, opens their
/// popover on a right-click, or on Space or Enter while the row has the focus. A row with neither is shown, and Tab
/// passes it.
pub fn seat_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    seat: &Tile,
    picture: Option<Held>,
    on_press: Option<Message>,
    on_menu: Option<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let name = crate::labels::label(&seat.name);
    let picture = avatar_pictured::<Message, R>(name, AvatarKind::Called(seat.colour), AvatarSize::Xs, picture);
    // The ring of a person who is heard is the picture's own edge, thickened.
    let picture = container(picture).padding(Space::Xxs.px()).class(if seat.ringed() {
        Surfaced::Ring(Radius::Full)
    } else {
        Surfaced::Clear
    });
    let heard = seat.own() || seat.ringed();
    let mut cells: Vec<Element<'a, Message, Theme, R>> = vec![
        picture._boxed(),
        coloured(
            seat.name.clone(),
            TypeStyle::Small,
            TextClass::Content(if heard { Content::Primary } else { Content::Secondary }),
        )
        .wrapping(iced_core::text::Wrapping::None)
        .width(Length::Fill)
        ._boxed(),
    ];
    if seat.elsewhere {
        cells.push(
            container(text(words.phrase(Phrase::OtherDevice), TypeStyle::Caption))
                .padding(Padding::from([1.0, Space::S.px() - Space::Xxs.px()]))
                .class(Surfaced::Framed(design::Surface::Raised, Radius::Full))
                ._boxed(),
        );
    }
    let live = IconClass::Role(Role::Accent(design::AccentRole::Text));
    if seat.camera != screens::CameraSeen::Off {
        cells.push(icon(Icon::Video, IconSize::S, live)._boxed());
    }
    if seat.sharing {
        cells.push(icon(Icon::Monitor, IconSize::S, live)._boxed());
    }
    let danger = IconClass::Role(Role::Status(Status::Danger, StatusRole::Text));
    if seat.muted && !seat.elsewhere {
        cells.push(icon(Icon::MicOff, IconSize::S, danger)._boxed());
    }
    if seat.deafened && !seat.elsewhere {
        cells.push(icon(Icon::HeadphonesOff, IconSize::S, danger)._boxed());
    }
    // The ring's room is outside the picture's own, so the name stands where the design has it.
    let content = Row::with_children(cells).spacing(Space::S.px() - Space::Xxs.px()).align_y(Alignment::Center);
    // The row is as tall as the design draws it: the picture and the ring round it, and what is left is padding.
    let tall = IconSize::M.px() + 2.0 * Space::Xxs.px();
    let inner = button::Button::new(content)
        .class(ButtonClass::Row { selected: false, tone: RowTone::Quiet })
        .padding(Padding {
            top: ((PERSON_ROW_HEIGHT - tall) / 2.0).max(0.0),
            bottom: ((PERSON_ROW_HEIGHT - tall) / 2.0).max(0.0),
            left: PERSON_ROW_INDENT - Space::Xxs.px(),
            right: Space::S.px(),
        })
        .width(Length::Fill)
        .on_press_maybe(on_press.clone());
    let semantics = Semantics { role: SemanticRole::ListItem, label: seat_label(seat, words) };
    let row = actionable(inner, semantics, on_press.or_else(|| on_menu.clone()));
    match on_menu {
        Some(menu) => iced_widget::mouse_area(row).on_right_press(menu)._boxed(),
        None => row,
    }
}

/// What a screen reader says of the row: who, and what they are doing or where.
fn seat_label(seat: &Tile, words: &Words) -> Label {
    let mut said = seat.said(words);
    if seat.camera != screens::CameraSeen::Off {
        said.push_str(", ");
        said.push_str(&words.phrase(Phrase::CameraOn));
    }
    if seat.elsewhere {
        said.push_str(", ");
        said.push_str(&words.phrase(Phrase::OtherDevice));
    }
    crate::labels::label(&said)
}
