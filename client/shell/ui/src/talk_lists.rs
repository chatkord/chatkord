//! The lists the content area shows in place of a discussion: the conversations and the threads this account follows,
//! newest and unread first. A row is a picture, a name, when it was last written in, what it said and a count of what
//! is unread; its press opens the conversation or the thread, whose back arrow returns here.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::discussion::Handles;
use crate::labels::label;
use crate::panel_parts::hairline;
use crate::typography::{coloured, line};
use crate::{AvatarKind, Icon, IconClass, Renderer, TextClass, Theme, avatar_pictured, count_badge, icon};
use design::{AvatarSize, Content, IconSize, MESSAGE_SIDE, Role, SemanticRole, Semantics, Space, Surface, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, button, scrollable};
use screens::{
    CentrePanel, ConversationRow, Offer, Place, PlaceMessage, PreviewLine, Roll, Rows, StatusWord, TalkMessage,
    ThreadRow, Window, WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The list `roll` of `place` in the content area: its header, then its rows, or why there are none. Folded, the header
/// opens the channels and ends with the search and the members column's toggle, `members` while it shows.
pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    roll: Roll,
    (folded, members): (bool, bool),
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let (mark, title) = match roll {
        Roll::Threads => (Icon::MessagesSquare, Phrase::Threads),
        Roll::Conversations => (Icon::MessageSquare, Phrase::DirectMessages),
    };
    let lead = folded.then(|| crate::bar_controls::channels_button::<R>(words)).into_iter().collect();
    let start = vec![
        icon(mark, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        line(words.phrase(title).into_owned(), TypeStyle::BodyStrong).width(Length::Fill)._boxed(),
    ];
    let end = if folded { crate::bar_controls::folded_end(place, members, words) } else { Vec::new() };
    let header = crate::bar_controls::header(lead, start, end);
    let body = match roll {
        Roll::Conversations => conversations(window, place, handles, words),
        Roll::Threads => threads(window, place, handles, words),
    };
    Column::with_children([header, hairline(), body]).height(Length::Fill)._boxed()
}

fn conversations<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let entries = match place.conversation_rows() {
        Rows::Ready(rows) if rows.is_empty() => {
            return shown(id, &CentrePanel::without(Roll::Conversations), words);
        }
        Rows::Ready(rows) => rows,
        Rows::Loading => return shown(id, &CentrePanel::waiting(), words),
        Rows::Failed(_) => return shown(id, &CentrePanel::failed(Roll::Conversations), words),
    };
    rows(entries.into_iter().map(|row| conversation_entry(window, place, row, handles, words)).collect())
}

fn threads<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let entries = match place.thread_rows() {
        Rows::Ready(rows) if rows.is_empty() => {
            return shown(id, &CentrePanel::without(Roll::Threads), words);
        }
        Rows::Ready(rows) => rows,
        Rows::Loading => return shown(id, &CentrePanel::waiting(), words),
        Rows::Failed(_) => return shown(id, &CentrePanel::failed(Roll::Threads), words),
    };
    rows(entries.into_iter().map(|row| thread_entry(window, place, row, handles, words)).collect())
}

/// What one row of either list is made of.
struct Entry<'a, R> {
    picture: Part<'a, R>,
    name: String,
    unread: u32,
    time: String,
    preview: String,
    note: Option<String>,
    open: Message,
}

fn rows<'a, R: Renderer + 'a>(entries: Vec<Entry<'a, R>>) -> Part<'a, R> {
    let list = Column::with_children(entries.into_iter().flat_map(|entry| [entry_row(entry), hairline()]));
    scrollable(list)
        .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
        .width(Length::Fill)
        .height(Length::Fill)
        ._boxed()
}

fn entry_row<'a, R: Renderer + 'a>(entry: Entry<'a, R>) -> Part<'a, R> {
    let Entry { picture, name, unread, time, preview, note, open } = entry;
    let style = if unread > 0 { TypeStyle::BodyStrong } else { TypeStyle::Body };
    let secondary = TextClass::Content(Content::Secondary);
    let head = Row::with_children([
        line(name.clone(), style)._boxed(),
        coloured(time, TypeStyle::Stamp, secondary).wrapping(iced_core::text::Wrapping::None)._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let mut lines: Vec<Part<'a, R>> =
        vec![head._boxed(), line(preview.clone(), TypeStyle::Body).class(secondary)._boxed()];
    lines.extend(note.map(|note| line(note, TypeStyle::Caption).class(secondary)._boxed()));
    let mut cells: Vec<Part<'a, R>> = vec![picture, Column::with_children(lines).width(Length::Fill)._boxed()];
    if unread > 0 {
        cells.push(count_badge(unread));
    }
    let row = Row::with_children(cells).spacing(Space::M.px()).align_y(Alignment::Start);
    let inner = button(row)
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(Padding::from([Space::M.px(), MESSAGE_SIDE]))
        .width(Length::Fill)
        .on_press(open.clone());
    let heard = label(&format!("{name}. {preview}"));
    actionable(inner, Semantics { role: SemanticRole::ListItem, label: heard }, Some(open))
}

/// The moment `at` said by the clock on this day, else by its day.
fn when(window: &Window, at: i64, words: &Words) -> String {
    screens::chat::local(at, window.zone())
        .map(|at| words.say(&Sentence::When { at, today: window.today() }))
        .unwrap_or_default()
}

/// What a row says of the last message: "You: " before it when this account wrote it.
fn said(line: Option<PreviewLine<'_>>, words: &Words) -> String {
    match line {
        Some(PreviewLine { mine: true, text, .. }) => format!("{} {text}", words.phrase(Phrase::YouColon)),
        Some(PreviewLine { text, .. }) => text.to_owned(),
        None => String::new(),
    }
}

fn conversation_entry<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    row: ConversationRow<'a>,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Entry<'a, R> {
    let name = row.name.unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
    let picture = match &row.conversation.with {
        screens::With::Member(user_id) => {
            let colour = screens::colour_of(user_id, row.colour);
            let status = row.presence.map(StatusWord::of).filter(|status| *status != StatusWord::Offline);
            let kind = if status.is_some() { AvatarKind::Called(colour) } else { AvatarKind::Offline(colour) };
            let held = place.picture_of(user_id).and_then(handles);
            crate::person_with_status(label(&name), kind, held, status.and_then(StatusWord::dot), Surface::Base)
        }
        screens::With::Gone { .. } => {
            crate::person_with_status(label(&name), AvatarKind::Person, None, None, Surface::Base)
        }
    };
    Entry {
        picture,
        unread: row.standing.unread,
        time: row.preview.map(|line| when(window, line.at, words)).unwrap_or_default(),
        preview: said(row.preview, words),
        note: None,
        open: Message::At(place.id(), PlaceMessage::Talk(TalkMessage::OpenConversation(row.conversation.id.clone()))),
        name,
    }
}

fn thread_entry<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    row: ThreadRow<'a>,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Entry<'a, R> {
    let name = row.author.unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
    let author = row.thread.preview.as_ref().and_then(|root| root.from.as_ref());
    let kind = match author {
        Some(user_id) => AvatarKind::Called(screens::colour_of(
            user_id,
            place.directory().person(user_id).and_then(|person| person.colour),
        )),
        None => AvatarKind::Person,
    };
    let held = author.and_then(|user_id| place.picture_of(user_id)).and_then(handles);
    Entry {
        picture: avatar_pictured(label(&name), kind, AvatarSize::M, held),
        unread: row.standing.unread,
        time: row.preview.map(|line| when(window, line.at, words)).unwrap_or_default(),
        preview: said(row.preview, words),
        note: Some(format!("{} · #{}", words.say(&Sentence::Replies(row.thread.replies)), row.channel)),
        open: Message::At(place.id(), PlaceMessage::Talk(TalkMessage::OpenFollowed(row.thread.timeline.clone()))),
        name,
    }
}

/// What a list says while it has no rows to show: the panel `screens` gives for it.
fn shown<'a, R: Renderer + 'a>(id: screens::PlaceId, panel: &CentrePanel, words: &'a Words) -> Part<'a, R> {
    crate::centre::<Message, R>(
        panel,
        |offer| match offer {
            Offer::TryAgain(roll) => Message::At(id, PlaceMessage::Talk(TalkMessage::Show(roll))),
            Offer::TryNow => Message::At(id, PlaceMessage::Session(screens::SessionMessage::TryNow)),
            Offer::ShowChannels => Message::ToggleChannels,
        },
        words,
    )
}
