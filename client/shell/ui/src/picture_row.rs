//! A picture row, a server's or this account's: the picture, or the server's letter on its colour or the person's
//! initials, faint while one goes up; the buttons that choose, change or remove it; and under the row its refusal.

use crate::labels::{label, phrase};
use crate::picture::Painted;
use crate::{Button, Renderer, TextClass, Theme, coloured};
use design::{AvatarSize, ButtonVariant, Content, RAIL_RADIUS, Rgb, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container};
use screens::picture_row::{Doing, Message as RowMessage, PictureRow};
use words::{Phrase, Words};

/// The row for the server called `name`, in `colour` until it has a picture; `paint` asks the host for one it holds.
pub(crate) fn picture_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &str,
    colour: Rgb,
    row: &PictureRow,
    paint: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
    send: impl Fn(RowMessage) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let busy = row.doing() != Doing::Nothing;
    let tile = server_tile(name, colour, row.held(), AvatarSize::Profile, busy, paint);
    row_under(tile, row, send, words)
}

/// The row for the account called `name`: their picture, round, or their initials on `kind` until they have one;
/// `paint` asks the host for one it holds.
pub(crate) fn person_picture_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &str,
    kind: crate::AvatarKind,
    row: &PictureRow,
    paint: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
    send: impl Fn(RowMessage) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let busy = row.doing() != Doing::Nothing;
    let tile = person_tile(name, kind, row.held(), AvatarSize::Profile, busy, paint);
    row_under(tile, row, send, words)
}

/// A person's picture of `size`: the one the host holds for `picture`, round, else their initials on `kind`; `faint`
/// while it is not theirs yet (going up).
pub(crate) fn person_tile<'a, Message: 'a, R: Renderer + 'a>(
    name: &str,
    kind: crate::AvatarKind,
    picture: Option<proto::files::FileId>,
    size: AvatarSize,
    faint: bool,
    paint: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    match picture {
        Some(picture) => {
            held_tile(name, Painted::Picture { picture, radius: design::Radius::Full, faint }, size, paint)
        }
        None => crate::avatar(label(name), kind, size),
    }
}

/// The picture the host paints for `painted`, `size` on a side, said by `name`.
fn held_tile<'a, Message: 'a, R: Renderer + 'a>(
    name: &str,
    painted: Painted,
    size: AvatarSize,
    paint: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let side = Length::Fixed(size.px());
    let semantics = design::Semantics { role: design::SemanticRole::Image, label: label(name) };
    crate::Labelled::new(container(paint(painted)).width(side).height(side), semantics)._boxed()
}

/// `tile` with the buttons that choose, change or remove its picture beside it and the refusal under it.
fn row_under<'a, Message: Clone + 'a, R: Renderer + 'a>(
    tile: Element<'a, Message, Theme, R>,
    row: &PictureRow,
    send: impl Fn(RowMessage) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let busy = row.doing() != Doing::Nothing;
    let doing: Element<'a, Message, Theme, R> = match (row.doing(), row.held()) {
        (Doing::Uploading, _) => {
            coloured(words.phrase(Phrase::Uploading), TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed()
        }
        (Doing::Nothing | Doing::Picking, held) => {
            let choose =
                Button::new(phrase(if held.is_some() { Phrase::ChangePicture } else { Phrase::ChoosePicture }, words))
                    .variant(ButtonVariant::Secondary)
                    .compact();
            let choose = if busy { choose } else { choose.on_press(send(RowMessage::Choose)) };
            let mut buttons = vec![choose.view::<R>()];
            if held.is_some() {
                let remove = Button::new(phrase(Phrase::RemovePicture, words)).variant(ButtonVariant::Quiet).compact();
                let remove = if busy { remove } else { remove.on_press(send(RowMessage::Remove)) };
                buttons.push(remove.view::<R>());
            }
            Row::with_children(buttons).spacing(Space::Xs.px()).align_y(Alignment::Center)._boxed()
        }
    };
    let top = Row::with_children([tile, doing]).spacing(Space::S.px()).align_y(Alignment::Center);
    let mut lines: Vec<Element<'a, Message, Theme, R>> = vec![top._boxed()];
    if let Some(refused) = row.refused() {
        lines.push(crate::refusal(words.say(&refused.sentence())));
    }
    Column::with_children(lines).spacing(Space::Xs.px())._boxed()
}

/// A server's picture of `size`: the one the host holds for `picture`, else its letter on `colour`; `faint` while it is
/// not the server's yet (going up, or a server not signed in).
pub(crate) fn server_tile<'a, Message: 'a, R: Renderer + 'a>(
    name: &str,
    colour: Rgb,
    picture: Option<proto::files::FileId>,
    size: AvatarSize,
    faint: bool,
    paint: &dyn Fn(Painted) -> Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    match picture {
        Some(picture) => held_tile(name, Painted::Picture { picture, radius: RAIL_RADIUS, faint }, size, paint),
        None if faint && matches!(size, AvatarSize::Rail) => crate::avatar::faint_server(label(name), colour),
        None => crate::avatar(label(name), crate::AvatarKind::Server(colour), size),
    }
}
