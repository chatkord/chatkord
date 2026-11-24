//! A message's files as the canvas draws Discord's: its pictures as one thumbnail or a grid of several, each opening
//! the viewer, and any other file as a card with Save; and the composer's side of them: the strip of files waiting to
//! send, each with its progress, its size or why it was refused, and its way out, and the button that attaches one.

use crate::button::{ButtonClass, actionable};
use crate::typography::{coloured, line};
use crate::{Button, Icon, IconClass, Labelled, Renderer, Surfaced, TextClass, Theme, icon};
use design::{
    AccentRole, BORDER_WIDTH, ButtonVariant, Content, FILE_CARD_WIDTH, IconSize, Label, PENDING_CLOSE_ROOM,
    PENDING_PREVIEW, PENDING_REFUSED_WIDTH, PENDING_WIDTH, PICTURE_GRID, PICTURE_ONE, Radius, Role, SemanticRole,
    Semantics, Space, Status, StatusRole, Surface, TypeStyle,
};
use iced_core::image::Handle;
use iced_core::{Alignment, ContentFit, Element, Length, Padding, Widget, alignment};
use iced_widget::{Column, Row, Space as Gap, button, container, progress_bar, scrollable, stack};
use words::{Phrase, Sentence, Words};

type Part<'a, Message, R> = Element<'a, Message, Theme, R>;

/// A picture of a message, as its thumbnail shows it.
pub struct Picture<Message> {
    /// Its thumbnail, decoded; none while it is not here yet, which draws its box empty.
    pub thumbnail: Option<Handle>,
    /// What it is called, said by a screen reader: its file's name.
    pub label: Label,
    /// What a press on it sends: the viewer, opened on it.
    pub open: Message,
}

/// A file of a message that is not a picture shown in the window: a card with its name and Save.
pub struct FileCard<Message> {
    /// Its file's name, as sent.
    pub name: String,
    /// What it is, said under the name: its size and kind ("1.2 MB · PDF").
    pub note: String,
    /// What Save sends.
    pub save: Message,
}

/// Where a file waiting in the composer stands.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingState {
    /// Being sent.
    Uploading {
        /// How much of it went, from 0 to 1.
        progress: f32,
        /// The same as a person reads it ("4.1 of 12 MB").
        said: String,
    },
    /// Sent, waiting for the message that takes it.
    Ready {
        /// Its size, said.
        size: String,
    },
    /// Refused, with why, said as a sentence.
    Refused(String),
}

/// A file waiting in the composer's strip.
pub struct Pending<Message> {
    /// Its file's name.
    pub name: String,
    /// Its picture, small, where it is one; none draws the file mark.
    pub preview: Option<Handle>,
    /// Where it stands.
    pub state: PendingState,
    /// What its way out sends: the upload cancelled, or the file taken out of the message.
    pub cancel: Message,
}

/// A message's `pictures`: one as a thumbnail, several in Discord's grid (two side by side, three as one tall beside
/// two stacked, four two by two, more in rows of three), each a button that opens it.
pub fn pictures<'a, Message: Clone + 'a, R: Renderer + 'a>(pictures: Vec<Picture<Message>>) -> Part<'a, Message, R> {
    let gap = Space::Xs.px();
    let (width, height) = PICTURE_GRID;
    let count = pictures.len();
    let mut cells = pictures.into_iter();
    let mut cell = |w: f32, h: f32| cells.next().map(|picture| picture_cell(picture, w, h));
    let grid: Part<'a, Message, R> = match count {
        0 => Gap::new()._boxed(),
        1 => cell(PICTURE_ONE.0, PICTURE_ONE.1).unwrap_or_else(|| Gap::new()._boxed()),
        2 => {
            let half = (width - gap) / 2.0;
            Row::with_children([cell(half, height), cell(half, height)].into_iter().flatten()).spacing(gap)._boxed()
        }
        3 => {
            let narrow = (width - gap) / 3.0;
            let tall = cell(width - gap - narrow, height);
            let short = (height - gap) / 2.0;
            let stacked =
                Column::with_children([cell(narrow, short), cell(narrow, short)].into_iter().flatten()).spacing(gap);
            Row::with_children(tall.into_iter().chain([stacked._boxed()])).spacing(gap)._boxed()
        }
        _ => {
            let (across, wide) = if count == 4 { (2, 2.0) } else { (3, 3.0) };
            let (w, h) = ((width - gap * (wide - 1.0)) / wide, (height - gap) / 2.0);
            let rows = (0..count.div_ceil(across))
                .map(|_| Row::with_children((0..across).filter_map(|_| cell(w, h))).spacing(gap)._boxed());
            Column::with_children(rows).spacing(gap)._boxed()
        }
    };
    container(grid).padding(Padding { top: Space::Xs.px() + Space::Xxs.px(), ..Padding::ZERO })._boxed()
}

/// One picture of the grid, `width` by `height`: its thumbnail cut to fill the box, rounded inside a hairline edge.
fn picture_cell<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picture: Picture<Message>,
    width: f32,
    height: f32,
) -> Part<'a, Message, R> {
    let inner_radius = Radius::M.px() - BORDER_WIDTH;
    let shown: Part<'a, Message, R> = match picture.thumbnail {
        Some(handle) => iced_widget::image(handle)
            .content_fit(ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fill)
            .border_radius(inner_radius)
            ._boxed(),
        None => Gap::new().width(Length::Fill).height(Length::Fill)._boxed(),
    };
    let framed = container(shown)
        .padding(BORDER_WIDTH)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .class(Surfaced::Framed(Surface::Raised, Radius::M));
    let pressed = button(framed).class(ButtonClass::Swatch).padding(0).on_press(picture.open.clone());
    actionable(pressed, Semantics { role: SemanticRole::Button, label: picture.label }, Some(picture.open))
}

/// A file's card: the file mark in the accent, its name and what it is, and Save.
pub fn file_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    card: FileCard<Message>,
    words: &Words,
) -> Part<'a, Message, R> {
    let name = crate::labels::label(&card.name);
    let save = Button::new(crate::labels::phrase(Phrase::Save, words))
        .icon(Icon::Download)
        .variant(ButtonVariant::Secondary)
        .compact()
        .on_press(card.save)
        .view::<R>();
    let said = Column::with_children([
        line(card.name, TypeStyle::BodyStrong)._boxed(),
        coloured(card.note, TypeStyle::Stamp, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    let row = Row::with_children([
        icon(Icon::File, IconSize::L, IconClass::Role(Role::Accent(AccentRole::Text)))._boxed(),
        said._boxed(),
        save,
    ])
    .spacing(Space::M.px())
    .align_y(Alignment::Center);
    let framed = container(row)
        .padding(Padding {
            top: Space::S.px() + Space::Xxs.px(),
            bottom: Space::S.px() + Space::Xxs.px(),
            left: Space::M.px(),
            right: Space::Xs.px() + Space::Xxs.px(),
        })
        .width(Length::Fixed(FILE_CARD_WIDTH))
        .class(Surfaced::Framed(Surface::Raised, Radius::M));
    let card = container(framed).padding(Padding { top: Space::Xs.px() + Space::Xxs.px(), ..Padding::ZERO });
    Labelled::new(card, Semantics { role: SemanticRole::ListItem, label: name })._boxed()
}

/// The composer's strip of `files` waiting to send, side by side, scrolling across where they are wider than it.
pub fn pending_strip<'a, Message: Clone + 'a, R: Renderer + 'a>(
    files: Vec<Pending<Message>>,
    words: &Words,
) -> Part<'a, Message, R> {
    let tiles = Row::with_children(files.into_iter().map(|file| pending_tile(file, words))).spacing(Space::S.px());
    let across = scrollable(tiles)
        .direction(iced_widget::scrollable::Direction::Horizontal(crate::theme::scroll_bar()))
        .width(Length::Fill);
    let strip = container(across).padding(Space::S.px() + Space::Xxs.px()).width(Length::Fill);
    Labelled::new(
        strip,
        Semantics { role: SemanticRole::List, label: crate::labels::phrase(Phrase::FilesToSend, words) },
    )
    ._boxed()
}

/// One waiting file: its picture or file mark, its name, then its progress, its size or why it was refused, and its
/// way out at the corner.
fn pending_tile<'a, Message: Clone + 'a, R: Renderer + 'a>(
    file: Pending<Message>,
    words: &Words,
) -> Part<'a, Message, R> {
    let refused = matches!(file.state, PendingState::Refused(_));
    let named = crate::labels::label(&file.name);
    let preview: Part<'a, Message, R> = match file.preview.filter(|_| !refused) {
        Some(handle) => iced_widget::image(handle)
            .content_fit(ContentFit::Cover)
            .width(Length::Fixed(PENDING_PREVIEW))
            .height(Length::Fixed(PENDING_PREVIEW))
            .border_radius(Radius::S.px())
            ._boxed(),
        None => {
            let tone = if refused {
                Role::Status(Status::Danger, StatusRole::Text)
            } else {
                Role::Content(Content::Secondary)
            };
            container(icon(Icon::File, IconSize::M, IconClass::Role(tone)))
                .center(Length::Fixed(PENDING_PREVIEW))
                .class(Surfaced::Framed(Surface::Base, Radius::S))
                ._boxed()
        }
    };
    let quiet = |said: String| coloured(said, TypeStyle::Stamp, TextClass::Content(Content::Secondary))._boxed();
    let mut said: Vec<Part<'a, Message, R>> = vec![line(file.name.clone(), TypeStyle::SmallStrong)._boxed()];
    let close = match file.state {
        PendingState::Uploading { progress, said: done } => {
            said.push(
                progress_bar(0.0..=1.0, progress.clamp(0.0, 1.0)).length(Length::Fill).girth(Space::Xs.px())._boxed(),
            );
            said.push(quiet(done));
            Close::Cancel
        }
        PendingState::Ready { size } => {
            said.push(quiet(size));
            Close::Remove
        }
        PendingState::Refused(why) => {
            said.push(crate::refusal(why));
            Close::Remove
        }
    };
    let close_label = match close {
        Close::Cancel => crate::labels::phrase(Phrase::CancelUpload, words),
        Close::Remove => crate::labels::label(&words.say(&Sentence::RemoveFile(file.name.clone()))),
    };
    let row =
        Row::with_children([preview, Column::with_children(said).spacing(Space::Xs.px()).width(Length::Fill)._boxed()])
            .spacing(Space::S.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
    let width = if refused { PENDING_REFUSED_WIDTH } else { PENDING_WIDTH };
    let class =
        if refused { Surfaced::Callout(Some(Status::Danger)) } else { Surfaced::Framed(Surface::Raised, Radius::M) };
    let tile = container(row)
        .padding(Padding { top: Space::S.px(), bottom: Space::S.px(), left: Space::S.px(), right: PENDING_CLOSE_ROOM })
        .width(Length::Fixed(width))
        .class(class);
    let out = Button::new(close_label)
        .icon_only(Icon::Close)
        .variant(ButtonVariant::Quiet)
        .small()
        .on_press(file.cancel)
        .view::<R>();
    let corner = container(out)
        .padding(Space::Xs.px() + Space::Xxs.px())
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Right)
        .align_y(alignment::Vertical::Top);
    let item = stack([tile._boxed(), corner._boxed()]);
    Labelled::new(item, Semantics { role: SemanticRole::ListItem, label: named })._boxed()
}

/// A waiting file's way out: its upload stopped, or the file taken out of the message.
enum Close {
    Cancel,
    Remove,
}

/// The composer's way to attach files: a paperclip, which `attach` opens the system's picker with; none where this
/// account may not attach.
pub fn attach_button<'a, Message: Clone + 'a, R: Renderer + 'a>(
    attach: Option<Message>,
    words: &Words,
) -> Part<'a, Message, R> {
    let mut button = Button::new(crate::labels::phrase(Phrase::AttachFile, words))
        .icon_only(Icon::Paperclip)
        .variant(ButtonVariant::Quiet)
        .compact();
    if let Some(attach) = attach {
        button = button.on_press(attach);
    }
    button.view::<R>()
}
