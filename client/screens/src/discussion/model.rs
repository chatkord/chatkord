//! The channel a discussion shows, the timeline it holds and everything kept beside it: pins, quotes, edits and the
//! viewer.

use super::*;
use crate::place::ops;

/// A text channel as the list shows it: its summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChannel {
    /// The channel's id at the server.
    pub id: ChannelId,
    /// Its name as the server last said it.
    pub name: String,
    /// The timeline of its messages.
    pub timeline: TimelineId,
    /// Its timeline's revision when the list was read.
    pub revision: u64,
}

/// A channel's history, as far as it has been read.
#[derive(Clone, Debug, PartialEq)]
pub enum History {
    /// The first page is asked for; what the server told meanwhile waits for it.
    Loading {
        /// What was told before the page came, applied over it in order.
        early: Vec<Early>,
    },
    /// Read.
    Ready(Timeline),
    /// The first page could not be had, and why.
    Failed(LinkError),
}

/// What the server told of a channel while its first page was being read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Early {
    /// A message as it is now.
    Told(Box<chat::Message>),
    /// A writer's messages deleted at once.
    Withdrawn(chat::Withdrawn),
}

/// A message sent and not yet answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsent {
    /// What makes a retry of it the same send.
    pub nonce: terms::Nonce,
    /// Its text, as it was drafted.
    pub body: String,
    /// Its draft, marks and all, which a refusal puts back.
    pub(super) draft: Draft,
    /// The link and the preview it went with, which a refusal puts back with the draft.
    pub(super) preview: Option<(String, PreviewMade)>,
}

/// One's own message being edited in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    /// The message.
    pub id: MessageId,
    /// Its text as edited so far.
    pub draft: Draft,
    /// Whether the edit is still typed or its save awaits an answer.
    pub stage: EditStage,
}

/// Where an edit in place stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditStage {
    /// Being typed; a refused save comes back here.
    Typing,
    /// Saved and not yet answered: the draft holds still.
    Saving,
}

/// What the composer holds for a channel, kept while another is open and across a reconnect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composing {
    /// The text.
    pub draft: Draft,
    /// The nonce of a send of this text that went unanswered, so sending it again is the same send.
    pub(crate) nonce: Option<terms::Nonce>,
    /// The message the draft answers, if it is a reply.
    pub reply: Option<Replying>,
    /// The files going with it.
    pub(crate) files: Vec<Attaching>,
    /// What the strip says of the files picked last.
    pub(crate) notice: Option<Sentence>,
    /// Where the field's cursor is, in scalars; none at its end.
    pub(crate) caret: Option<usize>,
    /// How often the composer moved the cursor itself.
    pub(crate) caret_moved: u64,
    /// How often the field was asked to take the keyboard (Reply pressed).
    pub(crate) focus_asked: u64,
    /// The chooser's selected choice.
    pub(crate) selected: usize,
    /// A query whose chooser Escape put away, by its range.
    pub(crate) dismissed: Option<std::ops::Range<usize>>,
    /// The first link of the draft and what became of its preview.
    pub(crate) preview: Option<Previewing>,
    /// The links whose preview was taken out of this draft.
    pub(crate) declined: Vec<String>,
}

/// What the place keeps of one timeline, open or not.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Held {
    /// What the composer holds.
    pub composing: Composing,
    /// The sends not yet answered, oldest first: kept whatever is open, so an answer finds its text.
    pub unsent: Vec<Unsent>,
    /// Where its list was scrolled when it was last open.
    pub(crate) scroll: Scroll,
    /// When the server was last told this account is writing here.
    pub typed_at: Option<Instant>,
}

/// A timeline's pinned messages, newest pin first, read a page at a time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins {
    /// The pins read, and the pin time and place of the last where more were pinned before it.
    pub pinned: crate::Paged<Vec<chat::Message>, proto::chatkord_v1::PinsBefore>,
    /// A page is asked and not yet answered.
    pub reading: bool,
    /// Why the last page could not be had.
    pub failed: Option<LinkError>,
}

/// A message a reply quotes, read alone.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Quote {
    Asked,
    Held(Box<chat::Message>),
    Gone,
}

/// What a reply's quoted line shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoted<'a> {
    /// The message it answers.
    Message(&'a chat::Message),
    /// It was deleted: "Original message was deleted".
    Deleted,
}

/// A picture open over the window, at its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewing {
    /// The message it was sent with.
    pub message: MessageId,
    /// The picture.
    pub file: FileId,
}

/// What came of a save asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveAnswer {
    /// The file is where the person chose.
    Saved,
    /// The person closed the dialog.
    Dismissed,
    /// This system has no dialog to ask where.
    NoDialog,
}

/// The open channel, conversation or thread.
#[derive(Clone, Debug)]
pub struct Discussion {
    /// What owns its timeline, from which its grants, standing, name and composer are read.
    pub owner: Owner,
    /// What it shows and its composer sends to: a timeline, or the thread a first reply will begin.
    pub compose: Compose,
    /// The list it was reached from, to which its back arrow returns; none when it was not reached from one.
    pub from: Option<Roll>,
    /// A thread's root, shown on top of its replies; none for a channel or a conversation.
    pub root: Option<Root>,
    /// Its messages.
    pub history: History,
    /// One's own message being edited.
    pub editing: Option<Editing>,
    /// Why the last ask was refused, until the next one.
    pub refused: Option<LinkError>,
    /// The message whose menu is open.
    pub menu: Option<MessageId>,
    /// The rows holding a selection: built wherever the list is scrolled, so the selection outlives the scroll.
    pub(super) selected: Vec<RowKey>,
    pub(super) zone: TimeZone,
    pub(super) heights: Heights,
    /// What each row of the timeline is known by, oldest first, built when the timeline changes.
    pub(super) keys: Vec<RowKey>,
    /// Where the list is scrolled, by the row in view.
    pub(super) scroll: Scroll,
    /// How far the viewport's bottom is above the list's end: as the list last said, or where `scroll` puts it once
    /// rows below it changed.
    pub(super) from_end: f32,
    /// The distance the list last said: said again, it is the list holding its place through a resize, never a scroll.
    pub(super) reported: f32,
    /// How many times the window moved the list itself, rather than following where the list said it was.
    pub(super) moved: u64,
    pub(super) viewport: f32,
    /// How tall what stands above the rows was last drawn: the channel's start, or the line saying older messages are
    /// read.
    pub(super) head: f32,
    /// A page is asked at this end and not yet answered.
    pub(super) reading: Option<End>,
    /// The read marks sent for it.
    pub(super) marked: super::reading::Marked,
    /// Its pinned messages, while their panel is open.
    pub pins: Option<Pins>,
    /// The messages replies quote that the timeline does not hold, by place.
    quoted: std::collections::HashMap<u64, Quote>,
    /// This account's own reactions, by message, at the revision they were told at.
    pub(super) mine: std::collections::HashMap<MessageId, (u64, Vec<chat::ReactionEmoji>)>,
    /// The spoilers shown, each by its message.
    pub(super) revealed: std::collections::HashSet<(MessageId, chat::SpoilerId)>,
    /// The picture open over the window.
    pub viewing: Option<Viewing>,
    /// What went wrong with a file saved or a link opened, until the next.
    pub notice: Option<Sentence>,
    /// The emoji picker, while it is open.
    pub picking: Option<Picking>,
    /// The read mark when the channel was opened with messages unread, which the "New" line stands after; gone once
    /// this account sends here. The marks sent while it is open never move it.
    pub(super) unread_after: Option<u64>,
}

impl Discussion {
    pub(crate) fn new(owner: Owner, timeline_id: TimelineId, zone: TimeZone, scroll: Scroll) -> Discussion {
        Discussion::with(owner, Compose::Timeline(timeline_id), zone, scroll)
    }

    /// The thread on `root` before it has a timeline: its root and a composer, with no replies to read.
    pub(crate) fn new_thread(owner: Owner, root: Root, zone: TimeZone) -> Discussion {
        let mut open = Discussion::with(owner, Compose::NewThread(root.message().clone()), zone, Scroll::default());
        open.history = History::Ready(Timeline::default());
        open.reached(None, Some(root))
    }

    /// The timeline it shows, none while it is a thread yet to begin.
    pub fn timeline_id(&self) -> Option<&TimelineId> {
        self.compose.timeline()
    }

    /// Whether it shows the timeline `timeline`.
    pub(crate) fn shows(&self, timeline: &TimelineId) -> bool {
        self.timeline_id() == Some(timeline)
    }

    /// The thread begun by its first reply, which now has the timeline `timeline`: it reads the replies.
    pub(super) fn begun(&mut self, timeline: TimelineId) -> Option<Ask> {
        self.compose = Compose::Timeline(timeline);
        self.history = History::Loading { early: Vec::new() };
        self.first_page()
    }

    fn with(owner: Owner, compose: Compose, zone: TimeZone, scroll: Scroll) -> Discussion {
        Discussion {
            owner,
            compose,
            from: None,
            root: None,
            history: History::Loading { early: Vec::new() },
            editing: None,
            refused: None,
            menu: None,
            selected: Vec::new(),
            zone,
            heights: Heights::new(FIRST_GUESS),
            keys: Vec::new(),
            scroll,
            from_end: 0.0,
            reported: 0.0,
            moved: 0,
            viewport: FIRST_VIEWPORT,
            head: 0.0,
            reading: None,
            marked: super::reading::Marked::default(),
            pins: None,
            quoted: std::collections::HashMap::new(),
            mine: std::collections::HashMap::new(),
            revealed: std::collections::HashSet::new(),
            viewing: None,
            notice: None,
            picking: None,
            unread_after: None,
        }
    }

    /// The discussion opened with `tally` its owner's: its unread, if any, stand after the read mark it holds.
    pub(crate) fn tally(mut self, tally: Option<Readings>) -> Discussion {
        self.unread_after = tally.filter(|tally| tally.unread > 0).map(|tally| tally.read);
        self
    }

    /// The discussion reached `from` a list, if it was, and with a thread's `root`.
    pub(crate) fn reached(mut self, from: Option<Roll>, root: Option<Root>) -> Discussion {
        self.from = from;
        self.root = root;
        self
    }

    /// The message the "New" line stands over: the first held past the read mark the channel was opened at that
    /// `me` did not write, when the one before it is held too; none once this account sent here.
    pub fn first_unread(&self, me: Option<&MemberId>) -> Option<u64> {
        let after = self.unread_after?;
        let timeline = self.timeline()?;
        if timeline.older() && timeline.first().is_none_or(|first| first.place > after) {
            return None;
        }
        timeline.messages().find(|message| message.place > after && !wrote(message, me)).map(|message| message.place)
    }

    /// This account's reaction `emoji` on the message `id`, shown at once; the server's count follows.
    pub(super) fn shown_mine(&mut self, id: &MessageId, revision: u64, emoji: chat::ReactionEmoji) {
        let mut mine = self.mine.get(id).map(|(_, mine)| mine.clone()).unwrap_or_default();
        mine.retain(|held| *held != emoji);
        mine.push(emoji);
        self.mine.insert(id.clone(), (revision, mine));
    }

    /// What a reply's quoted line shows for `reply`: the message from the timeline or read alone; none while it is
    /// being read.
    pub fn quote(&self, reply: &chat::Reply) -> Option<Quoted<'_>> {
        let held = self.timeline().and_then(|timeline| timeline.get(&reply.id));
        let message = match held {
            Some(message) => message,
            None => match self.quoted.get(&reply.place)? {
                Quote::Held(message) if message.id == reply.id => message,
                Quote::Held(_) | Quote::Gone => return Some(Quoted::Deleted),
                Quote::Asked => return None,
            },
        };
        Some(if message.deleted() { Quoted::Deleted } else { Quoted::Message(message) })
    }

    /// The messages replies held quote that are neither held nor asked for: each asked once, alone.
    pub(super) fn quotes_wanted(&mut self) -> Vec<Ask> {
        let (Some(timeline), Some(timeline_id)) = (self.timeline(), self.timeline_id().cloned()) else {
            return Vec::new();
        };
        let wanted: Vec<u64> = timeline
            .messages()
            .filter_map(|message| message.reply.as_ref())
            .filter(|reply| timeline.get(&reply.id).is_none())
            .map(|reply| reply.place)
            .collect();
        let mut todo = Vec::new();
        for place in wanted {
            if let std::collections::hash_map::Entry::Vacant(free) = self.quoted.entry(place) {
                free.insert(Quote::Asked);
                todo.push(ops::quoted(timeline_id.clone(), place));
            }
        }
        todo
    }

    /// Whether this account reacted to `message` with `emoji`, as the server last told it.
    pub fn reacted(&self, message: &chat::Message, emoji: &chat::ReactionEmoji) -> bool {
        let counted = message.reactions.iter().any(|reaction| reaction.emoji == *emoji);
        counted && self.mine.get(&message.id).is_some_and(|(_, mine)| mine.contains(emoji))
    }

    /// This account's own reactions as an answer to it told them, kept over what others are told, which carries none.
    pub(super) fn keep_mine(&mut self, messages: &[chat::Message]) {
        for message in messages {
            self.own_reactions(message.id.clone(), message.revision, message.mine.clone());
        }
    }

    pub(super) fn own_reactions(&mut self, id: MessageId, revision: u64, mine: Vec<chat::ReactionEmoji>) {
        match self.mine.get(&id) {
            Some((held, _)) if *held > revision => {}
            _ => {
                self.mine.insert(id, (revision, mine));
            }
        }
    }

    /// Whether the spoiler `spoiler` of `message` is shown.
    pub fn revealed(&self, message: &MessageId, spoiler: chat::SpoilerId) -> bool {
        self.revealed.contains(&(message.clone(), spoiler))
    }

    /// The picture open over the window, and the message's file it is.
    pub fn viewed(&self) -> Option<(&chat::Message, &chat::Attachment)> {
        let viewing = self.viewing.as_ref()?;
        let message = self.timeline()?.get(&viewing.message)?;
        let file = message.attachments.iter().find(|file| file.file == viewing.file)?;
        Some((message, file))
    }

    /// The thumbnails of the messages in the rows built, to fetch.
    pub(super) fn thumbnails(&self) -> Vec<(FileId, proto::files::FileKind)> {
        let built = self.window();
        let Some(timeline) = self.timeline() else { return Vec::new() };
        let shown = self.keys.get(built.first..built.end).unwrap_or_default();
        shown
            .iter()
            .filter_map(|key| match key {
                RowKey::Message(place) => timeline.at(*place),
                RowKey::Day(_) => None,
            })
            .flat_map(|message| {
                let attached = message.attachments.iter().filter_map(|file| file.picture?.thumbnail);
                let previewed = message.preview.iter().filter_map(|card| Some(card.picture?.thumbnail));
                attached.chain(previewed)
            })
            .map(|thumbnail| (thumbnail, proto::files::FileKind::Thumbnail))
            .collect()
    }

    /// A pin told: the panel shows it in its place, or no longer.
    pub(super) fn pin_told(&mut self, message: &chat::Message) {
        let Some(pins) = self.pins.as_mut() else { return };
        let pinned = pins.pinned.items_mut();
        pinned.retain(|held| held.id != message.id);
        if message.pinned_at.is_some() && !message.deleted() {
            let at = pinned.partition_point(|held| held.pinned_at > message.pinned_at);
            pinned.insert(at, message.clone());
        }
    }

    /// The time zone its days and times are read in.
    pub fn zone(&self) -> &TimeZone {
        &self.zone
    }

    /// The timeline, once read.
    pub fn timeline(&self) -> Option<&Timeline> {
        match &self.history {
            History::Ready(timeline) => Some(timeline),
            History::Loading { .. } | History::Failed(_) => None,
        }
    }
}

pub(super) fn quote_read(
    open: &mut Discussion,
    place: u64,
    page: Result<chat::Page, LinkError>,
) -> ControlFlow<Vec<Ask>, Vec<Ask>> {
    let quote = match page {
        Ok(page) => match page.messages.into_iter().find(|message| message.place == place) {
            Some(message) => Quote::Held(Box::new(message)),
            None => Quote::Gone,
        },
        // Asked again the next time the rows change.
        Err(_) => {
            open.quoted.remove(&place);
            return ControlFlow::Break(Vec::new());
        }
    };
    open.quoted.insert(place, quote);
    ControlFlow::Continue(Vec::new())
}

#[cfg(test)]
mod tests {
    use crate::harness::wire;
    use crate::harness::{Clock, chatting, discussion, general_timeline, message_id, said, tell, told};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;

    #[test]
    fn a_reply_whose_original_is_not_held_asks_for_it_alone_once_and_a_gone_one_reads_deleted() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let mut reply = said(9, 9, "bo", "opening it now");
        reply.reply = Some(chat::Reply { id: message_id(3), place: 3 });
        let todo = tell(&mut window, &clock, owls, told(reply.clone()));
        let asked = |todo: &[crate::Ask]| {
            todo.iter().filter(|done| matches!(done, crate::Ask::At(_, ask) if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.limit == 1 && read.anchor == Some(wire::history::Anchor::Around(3))))).count()
        };
        assert_eq!(asked(&todo), 1, "{todo:?}");
        let mut edited = reply.clone();
        edited.revision = 10;
        assert_eq!(asked(&tell(&mut window, &clock, owls, told(edited))), 0, "asked once");
        let open = |window: &Window| window.places()[0].discussion().cloned().unwrap();
        let quoted = reply.reply.clone().unwrap();
        assert_eq!(open(&window).quote(&quoted), None, "being read");
        let empty = chat::Page { messages: Vec::new(), older: true, newer: true, revision: 10 };
        let page = place::DiscussionMessage::Quoted { timeline: general_timeline(), place: 3, page: Ok(empty) };
        tell(&mut window, &clock, owls, discussion(page));
        assert_eq!(open(&window).quote(&quoted), Some(place::Quoted::Deleted), "Original message was deleted");
        let mut held_reply = said(10, 11, "bo", "and again");
        held_reply.reply = Some(chat::Reply { id: message_id(1), place: 1 });
        assert_eq!(asked(&tell(&mut window, &clock, owls, told(held_reply))), 0, "one held is read from the timeline");
    }
}
