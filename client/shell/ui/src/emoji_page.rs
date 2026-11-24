//! Server settings › Emoji, Discord's page: how many the server holds of the most it may and the way to upload one,
//! who may use them, then a card for each (its picture, `:name:`, who added it, rename and delete), one renamed in place
//! across its row.

use crate::labels::{label, phrase};
use crate::typography::{coloured, line};
use crate::{AvatarKind, Button, Icon, Labelled, Renderer, Surfaced, TextClass, TextInput, Theme, avatar_pictured};
use design::{
    AvatarSize, ButtonVariant, Content, EMOJI_CARD_HEIGHT, EMOJI_CARD_PICTURE, EMOJI_CARD_WIDTH, EMOJI_LIST_WIDTH,
    FORM_WIDTH, PersonColour, Radius, SETTINGS_LIST_WIDTH, SemanticRole, Semantics, Space, Status, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container};
use words::{Phrase, Sentence, Words};

/// A server emoji as its card shows it.
pub struct EmojiCard<Message> {
    /// Its name, without colons.
    pub name: String,
    /// Its picture, once the host has it.
    pub picture: Option<iced_core::image::Handle>,
    /// The name of who added it.
    pub added_by: String,
    /// Their colour.
    pub added_colour: PersonColour,
    /// Their picture, once the host has it; their initials without one.
    pub added_picture: Option<crate::Held>,
    /// What its pencil sends: rename it here.
    pub rename: Message,
    /// What its bin sends: ask to delete it.
    pub delete: Message,
}

/// The emoji being renamed in place: its name as typed so far and what typing, Enter and Escape send.
pub struct Renaming<'a, Message> {
    /// Which card it is, by its place in the list.
    pub at: usize,
    /// The name as typed, without colons.
    pub draft: &'a str,
    /// What each change of the name sends.
    pub on_change: Box<dyn Fn(String) -> Message + 'a>,
    /// What Enter and "save" send.
    pub on_submit: Message,
    /// What Escape and "cancel" send.
    pub on_escape: Message,
    /// The name was refused for its shape: said under the field.
    pub refused: bool,
    /// A new emoji, named before it is added, rather than one renamed.
    pub new: bool,
}

/// The page's action, on its heading's line: `held` of `most`, in the warning's colour when full, and Upload emoji,
/// off when full or with no `upload` (one already under way), else sending it.
pub fn emoji_action<'a, Message: Clone + 'a, R: Renderer + 'a>(
    held: u32,
    most: u32,
    upload: Option<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let full = held >= most;
    let class = if full { TextClass::Status(Status::Warning) } else { TextClass::Content(Content::Secondary) };
    let count = coloured(words.say(&Sentence::Used { used: held, of: most }), TypeStyle::Mono, class);
    let mut button =
        Button::new(phrase(Phrase::UploadEmoji, words)).icon(Icon::Upload).variant(ButtonVariant::Primary).compact();
    if let Some(upload) = upload.filter(|_| !full) {
        button = button.on_press(upload);
    }
    Row::with_children([count._boxed(), button.view::<R>()]).spacing(Space::M.px()).align_y(Alignment::Center)._boxed()
}

/// The page's content under its heading: who may use the emoji of `server`, that it is full when `held` reaches
/// `most`, what was last `refused` with what puts it away, then `cards`, the one `renaming` across its row.
pub fn emoji_page<'a, Message: Clone + 'a, R: Renderer + 'a>(
    server: &str,
    held: u32,
    most: u32,
    refused: Option<(String, Message)>,
    cards: Vec<EmojiCard<Message>>,
    renaming: Option<Renaming<'a, Message>>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let note = coloured(
        words.say(&Sentence::EmojiNote(server.to_string())),
        TypeStyle::Small,
        TextClass::Content(Content::Secondary),
    )
    .width(Length::Fill);
    let mut parts: Vec<Element<'a, Message, Theme, R>> =
        vec![container(note).width(Length::Fill.max(SETTINGS_LIST_WIDTH))._boxed()];
    if held >= most {
        let said = screens::Callout::new(Sentence::EmojiFull(server.to_string())).of(Status::Warning);
        let full = crate::callout::<Message, R>(&said, crate::CalloutParts::default(), words);
        parts.push(container(full).width(Length::Fill.max(SETTINGS_LIST_WIDTH))._boxed());
    }
    if let Some((said, dismiss)) = refused {
        let said = screens::Callout::new(Sentence::Said(said)).of(Status::Danger).dismissable();
        let press = crate::CalloutParts { act: None, away: Some(dismiss), under: None };
        let shown = crate::callout::<Message, R>(&said, press, words);
        parts.push(container(shown).width(Length::Fill.max(SETTINGS_LIST_WIDTH))._boxed());
    }
    let (renamed_at, mut renaming) = match renaming {
        Some(renaming) => (Some(renaming.at), Some(renaming)),
        None => (None, None),
    };
    let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    let mut run: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for (at, card) in cards.into_iter().enumerate() {
        match renaming.take_if(|_| renamed_at == Some(at)) {
            Some(renaming) => {
                if !run.is_empty() {
                    rows.push(wrapped(std::mem::take(&mut run)));
                }
                rows.push(renamed(card, renaming, words));
            }
            None => run.push(emoji_card(card, words)),
        }
    }
    if !run.is_empty() {
        rows.push(wrapped(run));
    }
    let list = Column::with_children(rows).spacing(Space::S.px()).width(Length::Fill.max(EMOJI_LIST_WIDTH));
    parts.push(
        Labelled::new(list, Semantics { role: SemanticRole::List, label: phrase(Phrase::Emoji, words) })._boxed(),
    );
    Column::with_children(parts).spacing(Space::L.px())._boxed()
}

/// Cards side by side, as many to a row as fit.
fn wrapped<'a, Message: 'a, R: Renderer + 'a>(
    cards: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    Row::with_children(cards).spacing(Space::S.px()).wrap().vertical_spacing(Space::S.px())._boxed()
}

/// An emoji's picture, or its `:name:` until the host has it.
fn picture<'a, Message: 'a, R: Renderer + 'a>(
    name: &str,
    handle: Option<iced_core::image::Handle>,
) -> Element<'a, Message, Theme, R> {
    let side = Length::Fixed(EMOJI_CARD_PICTURE);
    let shown: Element<'a, Message, Theme, R> = match handle {
        Some(handle) => iced_widget::image(handle).width(side).height(side)._boxed(),
        None => line(format!(":{name}:"), TypeStyle::Caption).class(TextClass::Content(Content::Secondary))._boxed(),
    };
    let held = container(shown).center(side);
    Labelled::new(held, Semantics { role: SemanticRole::Image, label: label(&format!(":{name}:")) })._boxed()
}

/// A card: the picture, `:name:` over who added it, then rename and delete.
fn emoji_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    card: EmojiCard<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let named = format!(":{}:", card.name);
    let by = Row::with_children([
        avatar_pictured(
            label(&card.added_by),
            AvatarKind::Called(card.added_colour),
            AvatarSize::Mark,
            card.added_picture,
        ),
        line(words.say(&Sentence::AddedBy(card.added_by.clone())), TypeStyle::Caption)
            .class(TextClass::Content(Content::Secondary))
            ._boxed(),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    let words_of = Column::with_children([line(named, TypeStyle::BodyStrong)._boxed(), by._boxed()])
        .spacing(Space::Xxs.px())
        .width(Length::Fill);
    let tool = |icon: Icon, said: Sentence, message: Message| {
        Button::new(label(&words.say(&said)))
            .icon_only(icon)
            .variant(ButtonVariant::Quiet)
            .compact()
            .on_press(message)
            .view::<R>()
    };
    let row = Row::with_children([
        picture(&card.name, card.picture),
        words_of._boxed(),
        tool(Icon::Pencil, Sentence::RenameEmoji(card.name.clone()), card.rename),
        tool(Icon::Trash, Sentence::DeleteEmoji(card.name.clone()), card.delete),
    ])
    .spacing(Space::M.px())
    .align_y(Alignment::Center);
    let framed = container(row)
        .padding(card_padding())
        .width(Length::Fixed(EMOJI_CARD_WIDTH))
        .center_y(Length::Fixed(EMOJI_CARD_HEIGHT))
        .class(Surfaced::Framed(Surface::Inset, Radius::M));
    Labelled::new(framed, Semantics { role: SemanticRole::ListItem, label: label(&format!(":{}:", card.name)) })
        ._boxed()
}

/// The card's room inside its edge, as the canvas pads it: less at the end, where its buttons' own room is.
fn card_padding() -> Padding {
    Padding { top: Space::S.px(), bottom: Space::S.px(), left: Space::M.px(), right: Space::Xs.px() + Space::Xxs.px() }
}

/// The card being renamed, across its row in the accent's edge: its picture, the field between colons, the refusal
/// when there is one, and how to leave or save it.
fn renamed<'a, Message: Clone + 'a, R: Renderer + 'a>(
    card: EmojiCard<Message>,
    renaming: Renaming<'a, Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let Renaming { draft, on_change, on_submit, on_escape, refused, new, .. } = renaming;
    let colon = || coloured(":", TypeStyle::Body, TextClass::Content(Content::Secondary));
    let room = Space::M.px() + Space::Xs.px();
    let named = if new {
        phrase(Phrase::NameEmoji, words)
    } else {
        label(&words.say(&Sentence::RenameEmoji(card.name.clone())))
    };
    let input = TextInput::new(named, draft)
        .on_input(on_change)
        .on_submit(on_submit.clone())
        .invalid(refused)
        .leading_room(room)
        .trailing_room(room)
        .view::<R>();
    let marks = Row::with_children([colon()._boxed(), Gap::new().width(Length::Fill)._boxed(), colon()._boxed()])
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, Space::M.px()]));
    let field =
        crate::over_field::over_field(input, container(marks).width(Length::Fill).center_y(Length::Fill)._boxed());
    let link = |said: Phrase, message: Message| {
        Button::new(phrase(said, words)).variant(ButtonVariant::Link).small().on_press(message).view::<R>()
    };
    let hint = |said: Phrase| {
        coloured(words.phrase(said).into_owned(), TypeStyle::Caption, TextClass::Content(Content::Secondary))
    };
    let under = Row::with_children([
        hint(Phrase::EditEscape)._boxed(),
        link(Phrase::EditCancel, on_escape.clone()),
        hint(Phrase::EditEnter)._boxed(),
        link(Phrase::EditSave, on_submit),
    ])
    .spacing(Space::Xs.px())
    .align_y(Alignment::Center);
    let mut column: Vec<Element<'a, Message, Theme, R>> = vec![crate::focus::take_focus(field)._boxed()];
    if refused {
        column.push(crate::refusal(words.phrase(Phrase::EmojiNameRule).into_owned()));
    }
    column.push(under._boxed());
    let editing = Column::with_children(column).spacing(Space::Xs.px()).width(Length::Fill.max(FORM_WIDTH));
    let row = Row::with_children([picture(&card.name, card.picture), editing._boxed()])
        .spacing(Space::M.px())
        .align_y(Alignment::Center);
    let framed =
        container(row).padding(card_padding()).width(Length::Fill).class(Surfaced::Editing(Surface::Inset, Radius::M));
    crate::on_escape(framed, on_escape, None)._boxed()
}
