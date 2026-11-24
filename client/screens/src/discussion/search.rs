//! Searching the messages this account reads, from the field at the server's level that every page of the server shows:
//! one search per place, always of the whole server. What is typed is asked of the server once typing pauses, its
//! matches listed newest first in a layer over the window as messages and as files, and a press on one opens where it
//! was written, at the message.

use super::*;
use crate::place::ops;
use chat::{Found, Home, RowKey, Scroll};
use proto::rules::{MAX_SEARCH_CHARS, SearchQuery};

/// How long typing must pause before what is typed is asked of the server.
pub const SEARCH_PAUSE: Duration = Duration::from_millis(300);

/// A search being made: what is typed, whether its layer is open, and where it stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    /// What is typed, within [`MAX_SEARCH_CHARS`] scalar values.
    pub typed: String,
    /// The layer is open; Escape and a press outside it put it away, the text staying.
    pub open: bool,
    /// Where it stands.
    pub stage: SearchStage,
}

/// Where a search stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SearchStage {
    /// What is typed holds no word yet, so there is nothing to ask.
    #[default]
    Blank,
    /// Typed, and asked once typing has paused since `at`.
    Waiting {
        /// What it looks for.
        query: SearchQuery,
        /// When it was last typed.
        at: Instant,
    },
    /// The first page is asked and not yet answered.
    Asking(SearchQuery),
    /// Answered.
    Found {
        /// What it looked for.
        query: SearchQuery,
        /// The matches read, newest first, none when nothing matches; and the last's message, where more follow it.
        found: crate::Paged<Vec<Found>, MessageId>,
        /// A page after the last is asked and not yet answered.
        reading: bool,
    },
    /// The first page could not be had.
    Failed {
        /// What it looked for.
        query: SearchQuery,
        /// Why not.
        why: LinkError,
    },
}

/// An attachment whose name matched, with the message it is attached to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoundFile<'a> {
    /// The message the file is attached to; pressing the file opens it.
    pub found: &'a Found,
    /// The file, as the message carries it.
    pub attached: &'a chat::Attachment,
}

impl FoundFile<'_> {
    /// Whether it is a picture, which its row shows as one; else a file.
    pub fn picture(&self) -> bool {
        matches!(self.attached.media, proto::files::MediaType::Picture(_))
    }
}

/// What a search layer asks of the place.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchMessage {
    /// The field or the magnifier was pressed, or Ctrl+K on any page of the server: the layer opens over what is shown,
    /// with the text and the answer it had.
    Open,
    /// Escape or a press outside the layer: it is put away, the text staying.
    Close,
    /// The field's text changed.
    Type(String),
    /// The field's clear button, or Escape in it: the text goes, the layer stays as it was.
    Clear,
    /// "Try again" over a search that could not be had.
    Retry,
    /// "Show more" under the matches.
    More,
    /// A page of the server's answer to the search asked.
    Answered {
        /// What was asked.
        query: SearchQuery,
        /// The match the page follows; none for the first page.
        before: Option<MessageId>,
        /// The page, or why it could not be had.
        page: Result<chat::Results, LinkError>,
    },
    /// A message or a file listed was pressed: the timeline opens where the message stands.
    OpenFound(MessageId),
}

impl Search {
    /// What was typed, ready to be asked at `now`.
    fn typed(text: String, now: Instant, was: Option<&Search>) -> Search {
        let stage = match SearchQuery::parse(&text) {
            Err(_) => SearchStage::Blank,
            // Another space or mark changes no word: the answer in hand stands.
            Ok(query) => match was.map(|was| &was.stage) {
                Some(
                    SearchStage::Waiting { query: held, .. }
                    | SearchStage::Asking(held)
                    | SearchStage::Found { query: held, .. }
                    | SearchStage::Failed { query: held, .. },
                ) if *held == query => was.map_or(SearchStage::Blank, |was| was.stage.clone()),
                _ => SearchStage::Waiting { query, at: now },
            },
        };
        Search { typed: text, open: true, stage }
    }

    /// The matches shown, when there are answered ones.
    pub fn found(&self) -> &[Found] {
        match &self.stage {
            SearchStage::Found { found, .. } => found.items(),
            SearchStage::Blank | SearchStage::Waiting { .. } | SearchStage::Asking(_) | SearchStage::Failed { .. } => {
                &[]
            }
        }
    }

    /// The matches whose message text matched, newest first: the layer's Messages.
    pub fn messages(&self) -> impl Iterator<Item = &Found> {
        self.found().iter().filter(|found| found.in_body)
    }

    /// The attachments whose names matched, newest message first and each message's in the order sent: the layer's
    /// Files. A file its message does not carry is passed over.
    pub fn files(&self) -> impl Iterator<Item = FoundFile<'_>> {
        self.found().iter().flat_map(|found| {
            found.files.iter().filter_map(move |file| {
                let attached = found.message.attachments.iter().find(|attached| attached.file == *file)?;
                Some(FoundFile { found, attached })
            })
        })
    }

    /// Whether Show more is offered under the matches: more follow the last, and none are being read.
    pub fn more(&self) -> bool {
        matches!(&self.stage, SearchStage::Found { found, reading: false, .. } if found.more())
    }

    /// What is searched for, as the layer says it found nothing for it: what is typed, without the spaces around it.
    pub fn searched(&self) -> &str {
        self.typed.trim()
    }

    /// When a search that waits for typing to pause may be asked.
    fn due(&self) -> Option<Instant> {
        match &self.stage {
            SearchStage::Waiting { at, .. } => Some(*at + SEARCH_PAUSE),
            SearchStage::Blank | SearchStage::Asking(_) | SearchStage::Found { .. } | SearchStage::Failed { .. } => {
                None
            }
        }
    }
}

/// `page` added to the matches `found`, a match read before passed over; more follow the last where the page says so.
fn extended(found: &mut crate::Paged<Vec<Found>, MessageId>, page: chat::Results) {
    let held = found.items();
    let fresh: Vec<Found> =
        page.found.into_iter().filter(|new| held.iter().all(|old| old.message.id != new.message.id)).collect();
    let last = fresh.last().or(held.last()).map(|last| last.message.id.clone());
    found.extend(fresh, last.filter(|_| page.older));
}

/// Where a search's match was written, as its card names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundIn {
    /// A channel, by its name.
    Channel(String),
    /// A thread, by its channel's name.
    Thread(String),
    /// A conversation, by the other person's name; none for one who is gone.
    Person(Option<String>),
    /// A timeline this window does not know.
    Unknown,
}

impl Place {
    /// The search of the whole server, unless the server does not serve search: then there is none to show.
    pub fn search(&self) -> Option<&Search> {
        self.served.on(proto::rules::Switch::Search).then_some(&self.search)
    }

    /// The files the search found by their names, where the server searches the names of files: the layer's Files.
    pub fn found_files(&self) -> Option<Vec<FoundFile<'_>>> {
        let search = self.search().filter(|_| self.served.on(proto::rules::Switch::Search))?;
        Some(search.files().collect())
    }

    /// Where `found` was written.
    pub fn found_in(&self, found: &Found) -> FoundIn {
        match &found.home {
            Home::Thread { channel, .. } => FoundIn::Thread(self.channel_name(channel).unwrap_or_default().to_owned()),
            Home::Timeline => match self.owner_of(&found.timeline) {
                Some(Owner::Channel(id)) => FoundIn::Channel(self.channel_name(&id).unwrap_or_default().to_owned()),
                Some(Owner::Conversation(id)) => FoundIn::Person(
                    self.conversation(&id).and_then(|conversation| self.conversation_name(conversation)),
                ),
                Some(Owner::Thread { channel, .. }) => {
                    FoundIn::Thread(self.channel_name(&channel).unwrap_or_default().to_owned())
                }
                None => FoundIn::Unknown,
            },
        }
    }

    /// When the search, if it waits for typing to pause, may be asked.
    pub(crate) fn search_due(&self) -> Option<Instant> {
        self.search().and_then(Search::due)
    }

    /// What the place owes the server for the search after a change: the first page, once typing has paused.
    pub(super) fn search_asks(&mut self, now: Instant) -> Vec<Ask> {
        if !self.served.on(proto::rules::Switch::Search) {
            return Vec::new();
        }
        let SearchStage::Waiting { query, at } = &self.search.stage else { return Vec::new() };
        if now.saturating_duration_since(*at) < SEARCH_PAUSE {
            return Vec::new();
        }
        let query = query.clone();
        self.search.stage = SearchStage::Asking(query.clone());
        vec![ops::search(query, None)]
    }

    /// What is done in the search, or reported for it. Nothing from a server that does not serve search, and nothing
    /// that opens or types before this account is signed in.
    pub(crate) fn on_search(&mut self, message: SearchMessage, at: Moment) -> Vec<Ask> {
        if !self.served.on(proto::rules::Switch::Search) {
            return Vec::new();
        }
        if let SearchMessage::OpenFound(id) = message {
            return self.open_found(&id);
        }
        let signed = matches!(self.status, Status::Signed(_));
        let search = &mut self.search;
        match message {
            SearchMessage::Open if signed => search.open = true,
            SearchMessage::Type(text) if signed => {
                if text.chars().count() <= MAX_SEARCH_CHARS {
                    let next = Search::typed(text, at.now, Some(search));
                    *search = next;
                }
            }
            SearchMessage::Clear => {
                *search = Search { open: search.open, ..Search::typed(String::new(), at.now, None) };
            }
            SearchMessage::Close => search.open = false,
            SearchMessage::Retry => {
                let SearchStage::Failed { query, .. } = &search.stage else { return Vec::new() };
                let query = query.clone();
                search.stage = SearchStage::Asking(query.clone());
                return vec![ops::search(query, None)];
            }
            SearchMessage::More => {
                let SearchStage::Found { query, found, reading: reading @ false } = &mut search.stage else {
                    return Vec::new();
                };
                let Some(before) = found.next().cloned() else { return Vec::new() };
                *reading = true;
                return vec![ops::search(query.clone(), Some(before))];
            }
            SearchMessage::Answered { query, before, page } => {
                search.stage = match (std::mem::take(&mut search.stage), before, page) {
                    (SearchStage::Asking(asked), None, Ok(page)) if asked == query => {
                        let mut found = crate::Paged::new();
                        extended(&mut found, page);
                        SearchStage::Found { query, found, reading: false }
                    }
                    (SearchStage::Asking(asked), None, Err(why)) if asked == query => {
                        SearchStage::Failed { query, why }
                    }
                    (SearchStage::Found { query: held, mut found, reading: true }, Some(_), page) if held == query => {
                        if let Ok(page) = page {
                            extended(&mut found, page);
                        }
                        SearchStage::Found { query: held, found, reading: false }
                    }
                    // An answer to a question no longer asked.
                    (other, _, _) => other,
                };
            }
            SearchMessage::Open | SearchMessage::Type(_) | SearchMessage::OpenFound(_) => {}
        }
        Vec::new()
    }

    /// A result pressed: the layer is put away, the pages over the server close, and the timeline it was written in
    /// opens where the message stands, as a popup's click opens one; in the discussion already open, the list goes to
    /// the message at once.
    fn open_found(&mut self, id: &MessageId) -> Vec<Ask> {
        let Some(found) = self.search.found().iter().find(|found| found.message.id == *id).cloned() else {
            return Vec::new();
        };
        self.search.open = false;
        let mut todo = self.close_pages();
        let place = found.message.place;
        if self.discussion().is_some_and(|open| open.shows(&found.timeline)) {
            let (me, grants) = (self.me().cloned(), self.discussion_grants(Pane::Content));
            todo.extend(self.on_open(Pane::Content, DiscussionMessage::Jump(place), me.as_ref(), grants));
            return todo;
        }
        // The timeline is read around the message as it opens: where it was left is where it is read from.
        self.held.entry(Compose::Timeline(found.timeline.clone())).or_default().scroll =
            Scroll::At { row: RowKey::Message(place), offset: 0.0 };
        let zone = self.zone.clone();
        todo.extend(match found.home {
            Home::Timeline => self.open_timeline(&found.timeline, zone),
            Home::Thread { channel, root } => {
                let owner = Owner::Thread { channel, root: root.clone() };
                self.seat(owner, found.timeline, Some(Root::Brief { message: root, preview: None }), None)
            }
        });
        todo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        ADA, Clock, chatting, every_switch_on, general_timeline, message_id, said, tell, timeline_id,
    };
    use crate::harness::{sends, wire};
    use crate::place::{LobbyMessage, PageMessage, PlaceId};
    use crate::window::{Ask as WindowAsk, Message as WindowMessage, Window};
    use proto::files::FileId;
    use proto::rules::Grant;

    const GENERAL_READ: [Grant; 2] = [Grant::ChannelRead, Grant::SendMessage];

    fn query(typed: &str) -> SearchQuery {
        SearchQuery::parse(typed).unwrap()
    }

    fn searching(window: &Window) -> Search {
        window.places()[0].search().cloned().expect("a search")
    }

    fn asked(message: SearchMessage) -> Message {
        Message::Search(message)
    }

    fn type_in(window: &mut Window, clock: &Clock, owls: PlaceId, typed: &str) -> Vec<WindowAsk> {
        tell(window, clock, owls, asked(SearchMessage::Type(typed.into())))
    }

    /// The asks for a search the window owes once `by` has passed.
    fn after(window: &mut Window, clock: &mut Clock, by: Duration) -> Vec<WindowAsk> {
        clock.pass(by);
        window.update(WindowMessage::Tick, clock.at())
    }

    fn searches(todo: &[WindowAsk]) -> Vec<(Vec<String>, Option<MessageId>)> {
        todo.iter()
            .filter_map(|done| match done {
                WindowAsk::At(_, ask) => match ask.op() {
                    Some(crate::harness::Body::Search(search)) => Some((
                        search.query.split(' ').map(str::to_owned).collect(),
                        search.before.as_ref().and_then(|before| MessageId::parse(&before.message_id)),
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn found_in(timeline: &TimelineId, place: u64) -> Found {
        Found {
            message: said(place, place, ADA, "the dice are here"),
            timeline: timeline.clone(),
            home: Home::Timeline,
            files: Vec::new(),
            in_body: true,
        }
    }

    pub(super) fn answered(typed: &str, before: Option<MessageId>, found: Vec<Found>, older: bool) -> SearchMessage {
        SearchMessage::Answered { query: query(typed), before, page: Ok(chat::Results { found, older }) }
    }

    fn serving() -> (Window, Clock, PlaceId) {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        every_switch_on(&mut window, &clock, owls);
        (window, clock, owls)
    }

    #[test]
    fn typing_asks_the_server_once_typing_pauses_and_only_for_what_is_typed_last() {
        let (mut window, mut clock, owls) = serving();
        let typed = type_in(&mut window, &clock, owls, "dice");
        assert!(searches(&typed).is_empty(), "nothing is asked while typing");
        let due = clock.0 + SEARCH_PAUSE;
        assert!(typed.iter().any(|done| matches!(done, WindowAsk::WakeAt(when) if *when == due)), "{typed:?}");
        clock.pass(SEARCH_PAUSE / 2);
        type_in(&mut window, &clock, owls, "dice ro");
        let early = after(&mut window, &mut clock, SEARCH_PAUSE / 2);
        assert!(searches(&early).is_empty(), "the pause starts over with each word typed");
        let asked = after(&mut window, &mut clock, SEARCH_PAUSE / 2);
        assert_eq!(searches(&asked), [(vec!["dice".to_string(), "ro".to_string()], None)]);
        assert!(matches!(searching(&window).stage, SearchStage::Asking(_)));
        assert!(searches(&after(&mut window, &mut clock, SEARCH_PAUSE)).is_empty(), "asked once");
    }

    #[test]
    fn a_query_with_no_word_asks_nothing_and_a_space_added_changes_no_word() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "!! ?");
        assert!(matches!(searching(&window).stage, SearchStage::Blank));
        assert!(searches(&after(&mut window, &mut clock, SEARCH_PAUSE)).is_empty());
        type_in(&mut window, &clock, owls, "dice");
        let asked = after(&mut window, &mut clock, SEARCH_PAUSE);
        assert_eq!(searches(&asked).len(), 1);
        type_in(&mut window, &clock, owls, "dice ");
        assert!(matches!(searching(&window).stage, SearchStage::Asking(_)), "the same words: the ask stands");
        let too_long = "x".repeat(MAX_SEARCH_CHARS + 1);
        type_in(&mut window, &clock, owls, &too_long);
        assert_eq!(searching(&window).typed, "dice ", "a keystroke past the bound is refused, never cut");
    }

    #[test]
    fn matches_are_listed_and_an_answer_to_an_earlier_query_is_dropped() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        tell(&mut window, &clock, owls, asked(answered("dic", None, vec![found_in(&general_timeline(), 3)], false)));
        assert!(matches!(searching(&window).stage, SearchStage::Asking(_)), "not the query asked");
        let found = vec![found_in(&general_timeline(), 3), found_in(&general_timeline(), 2)];
        tell(&mut window, &clock, owls, asked(answered("dice", None, found, true)));
        let search = searching(&window);
        assert_eq!(search.found().len(), 2);
        assert!(matches!(search.stage, SearchStage::Found { ref found, reading: false, .. } if found.more()));
    }

    #[test]
    fn nothing_found_and_a_failed_ask_are_their_own_states_and_try_again_asks_at_once() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "xyzzy");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        tell(&mut window, &clock, owls, asked(answered("xyzzy", None, Vec::new(), false)));
        assert!(matches!(&searching(&window).stage, SearchStage::Found { found, .. } if found.items().is_empty()));
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let failed = SearchMessage::Answered { query: query("dice"), before: None, page: Err(LinkError::Timeout) };
        tell(&mut window, &clock, owls, asked(failed));
        assert!(matches!(searching(&window).stage, SearchStage::Failed { why: LinkError::Timeout, .. }));
        let again = tell(&mut window, &clock, owls, asked(SearchMessage::Retry));
        assert_eq!(searches(&again), [(vec!["dice".to_string()], None)]);
        assert!(matches!(searching(&window).stage, SearchStage::Asking(_)));
    }

    #[test]
    fn show_more_asks_the_page_after_the_last_match_once_and_adds_it_without_repeats() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let general = general_timeline();
        tell(
            &mut window,
            &clock,
            owls,
            asked(answered("dice", None, vec![found_in(&general, 3), found_in(&general, 2)], true)),
        );
        let more = tell(&mut window, &clock, owls, asked(SearchMessage::More));
        assert_eq!(searches(&more), [(vec!["dice".to_string()], Some(message_id(2)))]);
        let twice = tell(&mut window, &clock, owls, asked(SearchMessage::More));
        assert!(searches(&twice).is_empty(), "a page is asked once");
        let page = answered("dice", Some(message_id(2)), vec![found_in(&general, 2), found_in(&general, 1)], false);
        tell(&mut window, &clock, owls, asked(page));
        let ids: Vec<MessageId> = searching(&window).found().iter().map(|found| found.message.id.clone()).collect();
        assert_eq!(ids, [message_id(3), message_id(2), message_id(1)]);
        assert!(
            matches!(searching(&window).stage, SearchStage::Found { ref found, reading: false, .. } if !found.more())
        );
    }

    #[test]
    fn a_server_without_search_has_no_search_and_is_never_asked() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let mut clock = clock;
        let typed = type_in(&mut window, &clock, owls, "dice");
        assert!(window.places()[0].search().is_none());
        assert!(typed.iter().all(|done| !matches!(done, WindowAsk::WakeAt(_))), "{typed:?}");
        assert!(searches(&after(&mut window, &mut clock, SEARCH_PAUSE)).is_empty());
    }

    #[test]
    fn a_result_in_the_open_timeline_goes_to_its_message_and_one_in_another_opens_it_there() {
        let (mut window, mut clock, owls) = serving();
        let mut listed = crate::harness::channels_holding(GENERAL_READ.into_iter().collect());
        let ideas = timeline_id("t-ideas");
        listed.text.push(TextChannel {
            id: crate::harness::channel_id("ideas"),
            name: "ideas".into(),
            timeline: ideas.clone(),
            revision: 0,
        });
        tell(&mut window, &clock, owls, Message::Lobby(LobbyMessage::Channels(Ok(listed))));
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let here = found_in(&general_timeline(), 1);
        let there = Found {
            message: said(7, 7, ADA, "dice in ideas"),
            timeline: ideas.clone(),
            home: Home::Timeline,
            files: Vec::new(),
            in_body: true,
        };
        tell(&mut window, &clock, owls, asked(answered("dice", None, vec![there, here], false)));
        let todo = tell(&mut window, &clock, owls, asked(SearchMessage::OpenFound(message_id(1))));
        assert!(todo.is_empty(), "the message is held: the list goes to it, nothing is asked: {todo:?}");
        assert!(!searching(&window).open, "the layer is put away");
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let there = Found {
            message: said(7, 7, ADA, "dice in ideas"),
            timeline: ideas.clone(),
            home: Home::Timeline,
            files: Vec::new(),
            in_body: true,
        };
        tell(&mut window, &clock, owls, asked(answered("dice", None, vec![there], false)));
        let todo = tell(&mut window, &clock, owls, asked(SearchMessage::OpenFound(message_id(7))));
        let read = |done: &WindowAsk| matches!(done, WindowAsk::At(_, ask) if sends(ask, wire::History { timeline_id: ideas.as_str().into(), anchor: Some(wire::history::Anchor::Around(7)), limit: 0 }));
        assert!(todo.iter().any(read), "{todo:?}");
        assert_eq!(window.places()[0].discussion().and_then(|open| open.timeline_id().cloned()), Some(ideas));
    }

    #[test]
    fn a_reply_found_in_a_thread_opens_that_thread_around_it() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "dice");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let thread = timeline_id("t-thread-1");
        let home = Home::Thread { channel: crate::harness::channel_id("general"), root: message_id(1) };
        let reply = Found {
            message: said(4, 4, ADA, "dice again"),
            timeline: thread.clone(),
            home,
            files: Vec::new(),
            in_body: true,
        };
        tell(&mut window, &clock, owls, asked(answered("dice", None, vec![reply], false)));
        let todo = tell(&mut window, &clock, owls, asked(SearchMessage::OpenFound(message_id(4))));
        let read = |done: &WindowAsk| matches!(done, WindowAsk::At(_, ask) if sends(ask, wire::History { timeline_id: thread.as_str().into(), anchor: Some(wire::history::Anchor::Around(4)), limit: 0 }));
        assert!(todo.iter().any(read), "{todo:?}");
        let open = window.places()[0].discussion().cloned().expect("a discussion");
        assert!(matches!(open.owner, Owner::Thread { .. }), "{:?}", open.owner);
    }

    #[test]
    fn ctrl_k_opens_the_search_on_any_page_of_the_server() {
        use crate::window::DevicePage;
        let (mut window, clock, owls) = serving();
        let opened = |window: &Window| window.places()[0].search().is_some_and(|search| search.open);
        let shut = |window: &mut Window| {
            tell(window, &clock, owls, asked(SearchMessage::Close));
            assert!(!opened(window));
        };
        tell(&mut window, &clock, owls, asked(SearchMessage::Open));
        assert!(opened(&window), "over a discussion");
        shut(&mut window);
        tell(&mut window, &clock, owls, Message::Page(PageMessage::OpenAccount));
        tell(&mut window, &clock, owls, asked(SearchMessage::Open));
        assert!(opened(&window), "over a page of the server");
        shut(&mut window);
        window.update(WindowMessage::OpenDevice(DevicePage::Appearance), clock.at());
        tell(&mut window, &clock, owls, asked(SearchMessage::Open));
        assert!(opened(&window), "over this device's page");
        type_in(&mut window, &clock, owls, "dice");
        shut(&mut window);
        tell(&mut window, &clock, owls, asked(SearchMessage::Open));
        assert_eq!(searching(&window).typed, "dice", "opened again, the text it had stays");
    }

    fn attached(file: FileId, name: &str, media: proto::files::MediaType) -> chat::Attachment {
        let name = proto::files::AttachmentName::parse(name).unwrap();
        chat::Attachment { file, name, size: 1024, media, picture: None }
    }

    #[test]
    fn a_file_found_by_its_name_is_listed_with_its_message() {
        use proto::files::{MediaType, PictureType};
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "map");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        let general = general_timeline();
        let (map, notes, gone) = [7, 8, 9].map(|byte| FileId::from_bytes([byte; FileId::LEN])).into();
        let carrying = |place: u64, files: Vec<chat::Attachment>| {
            let mut found = found_in(&general, place);
            found.message.attachments = files;
            found
        };
        let picture = attached(map, "map.png", MediaType::Picture(PictureType::Png));
        let named = Found {
            files: vec![map, gone],
            in_body: false,
            ..carrying(2, vec![picture, attached(notes, "notes.txt", MediaType::Bytes)])
        };
        let both = Found {
            files: vec![notes],
            in_body: true,
            ..carrying(1, vec![attached(notes, "map notes.txt", MediaType::Bytes)])
        };
        tell(&mut window, &clock, owls, asked(answered("map", None, vec![named, both], false)));
        let search = searching(&window);
        assert_eq!(search.messages().map(|found| found.message.place).collect::<Vec<_>>(), [1]);
        let files: Vec<_> =
            search.files().map(|found| (found.found.message.place, found.attached.file, found.picture())).collect();
        assert_eq!(
            files,
            [(2, map, true), (1, notes, false)],
            "a name that matched lists its file, whatever the body did; one the message does not carry is passed over"
        );
        assert_eq!(window.places()[0].found_files().map(|found| found.len()), Some(2));
    }

    #[test]
    fn show_more_is_offered_while_more_follow_and_none_are_being_read() {
        let (mut window, mut clock, owls) = serving();
        type_in(&mut window, &clock, owls, "  dice ");
        after(&mut window, &mut clock, SEARCH_PAUSE);
        assert!(!searching(&window).more(), "nothing answered yet");
        assert_eq!(searching(&window).searched(), "dice");
        let general = general_timeline();
        tell(&mut window, &clock, owls, asked(answered("dice", None, vec![found_in(&general, 3)], true)));
        assert!(searching(&window).more());
        tell(&mut window, &clock, owls, asked(SearchMessage::More));
        assert!(!searching(&window).more(), "a page is being read");
    }
}
