//! What the composer holds beside its text: the message it answers, the files going with it, the chooser a trigger
//! opens (`@` members and, with `MENTION_EVERYONE`, roles, `@everyone` and `@here`; `#` channels; `:` emoji), and the
//! pace it tells the server it is writing at.

use super::*;
use chat::{Completion, MarkKind, Query, Trigger};
use proto::files::{AttachmentName, FileKind, FileRefusal};
use proto::rules::{Carries, MAX_FILES, TYPING_EVERY};
use terms::FileError;

/// How many choices a chooser lists at most, as Discord's.
pub const CHOICES_SHOWN: usize = 10;

/// Where a composer sends: the key of everything the place keeps of one (draft, files, chooser, unsent sends).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Compose {
    /// A timeline that exists: a channel's, a conversation's or a thread's.
    Timeline(TimelineId),
    /// The thread a message will have once its first reply is sent; it has no timeline before.
    NewThread(chat::MessageId),
}

impl Compose {
    /// The timeline it sends to, when it has one.
    pub fn timeline(&self) -> Option<&TimelineId> {
        match self {
            Compose::Timeline(timeline) => Some(timeline),
            Compose::NewThread(_) => None,
        }
    }
}

/// The message a reply being written answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replying {
    /// The message, by its id.
    pub to: chat::MessageId,
    /// Its place, by which it is asked for when the timeline does not hold it.
    pub place: u64,
    /// The reply mentions its author: on by default, as Discord's.
    pub mention_author: bool,
}

/// One upload, as the composer knows it: issued by its place, never reused, so a late report for one removed reaches
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Upload(pub(crate) u64);

/// Where a file going with a message stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachState {
    /// Being sent: so many bytes of its size, once known.
    Sending {
        /// Sent so far.
        sent: u64,
        /// Its size; none until the upload has opened it.
        of: Option<u64>,
    },
    /// Sent and kept by the server, to go with the message.
    Ready {
        /// The server's file.
        file: FileId,
        /// Its size, as the upload told it before it ended; none where it never did.
        size: Option<u64>,
    },
    /// Refused, and why: it stays in the strip until it is removed.
    Refused(Sentence),
}

/// A file going with the message being written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attaching {
    /// The upload.
    pub upload: Upload,
    /// The name it is sent with.
    pub name: AttachmentName,
    /// Where it stands.
    pub state: AttachState,
}

/// What a chooser offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A member, by the name others see.
    Member {
        /// Their id.
        id: MemberId,
        /// The name others see.
        name: String,
        /// The colour they picked, if any.
        colour: Option<proto::rules::Colour>,
    },
    /// A role.
    Role {
        /// Its id.
        id: RoleId,
        /// Its name.
        name: String,
        /// Its colour, if any.
        colour: Option<proto::rules::Colour>,
    },
    /// Everyone who can see the channel.
    Everyone,
    /// Everyone online who can see it.
    Here,
    /// A channel, of either kind.
    Channel {
        /// Its id.
        id: ChannelId,
        /// Its name.
        name: String,
        /// A voice channel's.
        voice: bool,
        /// The name of the category it is in; none for a channel in no category, offered under no heading.
        category: Option<String>,
    },
    /// One of the server's own emoji.
    Custom(ServerEmoji),
    /// A Unicode emoji.
    Unicode(&'static chat::emoji::Emoji),
}

impl Choice {
    /// What choosing it writes in the draft.
    fn completion(&self) -> Completion {
        match self {
            Choice::Member { id, name, .. } => {
                Completion { text: format!("@{name}"), mark: Some(MarkKind::Mention(id.clone())) }
            }
            Choice::Role { id, name, .. } => {
                Completion { text: format!("@{name}"), mark: Some(MarkKind::Role(id.clone())) }
            }
            Choice::Everyone => Completion { text: "@everyone".into(), mark: Some(MarkKind::Everyone) },
            Choice::Here => Completion { text: "@here".into(), mark: Some(MarkKind::Here) },
            Choice::Channel { id, name, .. } => {
                Completion { text: format!("#{name}"), mark: Some(MarkKind::Channel(id.clone())) }
            }
            Choice::Custom(emoji) => {
                Completion { text: emoji.written(), mark: Some(MarkKind::CustomEmoji(emoji.id.clone())) }
            }
            Choice::Unicode(emoji) => Completion { text: emoji.as_str().to_owned(), mark: None },
        }
    }
}

/// A chooser open over the composer: what is being typed, what it offers, and which is chosen by Enter or Tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choosing {
    /// What is being typed.
    pub query: Query,
    /// What it offers, in order; never empty.
    pub choices: Vec<Choice>,
    /// The one Enter or Tab chooses.
    pub selected: usize,
}

/// Why the system's picker gave no files: this system has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoPicker;

/// The words a refused upload shows in the strip: a size or a quota by the server's own limit, where it said it.
fn upload_refused(failed: &FileError, limits: terms::FileLimits) -> Sentence {
    match failed {
        FileError::Refused { kind: FileKind::Attachment, refusal: FileRefusal::TooLarge } => {
            Sentence::FileTooLarge(limits.attachment)
        }
        FileError::Refused { refusal: FileRefusal::AccountFull, .. } => Sentence::FilesUsed(limits.account),
        other => failure::file(other),
    }
}

impl Composing {
    /// The files going with it, in the order shown.
    pub fn files(&self) -> &[Attaching] {
        &self.files
    }

    /// Whether every file is sent, so the message may go.
    pub fn files_ready(&self) -> bool {
        !self.files.iter().any(|file| matches!(file.state, AttachState::Sending { .. }))
    }

    /// What the message would carry beside its text: files once one is sent, which lets it go with no text.
    pub fn carries(&self) -> Carries {
        if self.ready_files().is_empty() { Carries::Nothing } else { Carries::Files }
    }

    /// The ids of the files ready to go, in order.
    pub(super) fn ready_files(&self) -> Vec<FileId> {
        self.files
            .iter()
            .filter_map(|file| match file.state {
                AttachState::Ready { file, .. } => Some(file),
                AttachState::Sending { .. } | AttachState::Refused(_) => None,
            })
            .collect()
    }

    /// What the strip says of the files picked last, when some were not taken.
    pub fn notice(&self) -> Option<&Sentence> {
        self.notice.as_ref()
    }

    /// Where the field's cursor is, in scalars; none at the end.
    pub fn caret(&self) -> Option<usize> {
        self.caret
    }

    /// How many times the composer moved the field's cursor itself (a completion); the front end puts its cursor at
    /// [`Composing::caret`] each time this changes.
    pub fn caret_moved(&self) -> u64 {
        self.caret_moved
    }

    /// How many times the field was asked to take the keyboard, as Reply asks; the front end gives it the keyboard
    /// each time this changes.
    pub fn focus_asked(&self) -> u64 {
        self.focus_asked
    }

    /// The completion being typed at the cursor, unless it was put away.
    fn query(&self) -> Option<Query> {
        let query = match self.caret {
            Some(caret) => self.draft.query_at(caret),
            None => self.draft.query(),
        }?;
        (self.dismissed.as_ref() != Some(&query.range)).then_some(query)
    }
}

impl Place {
    /// What owns `compose`: its timeline's owner, else the thread being begun that an open pane shows.
    pub(super) fn owner_of_compose(&self, compose: &Compose) -> Option<Owner> {
        match compose {
            Compose::Timeline(timeline) => self.owner_of(timeline),
            Compose::NewThread(_) => {
                let Status::Signed(signed) = &self.status else { return None };
                signed.open().find(|open| open.compose == *compose).map(|open| open.owner.clone())
            }
        }
    }

    /// The chooser open over `compose`'s composer, if a trigger is being typed and something answers it.
    pub fn choosing(&self, compose: &Compose) -> Option<Choosing> {
        let composing = self.composing(compose)?;
        let query = composing.query()?;
        let owner = self.owner_of_compose(compose)?;
        let choices = self.choices(&query, &owner);
        if choices.is_empty() {
            return None;
        }
        let selected = composing.selected.min(choices.len() - 1);
        Some(Choosing { query, choices, selected })
    }

    /// What `query` offers in a discussion `owner` owns: members, then roles and everyone where `MENTION_EVERYONE` is
    /// held there; the channels this account sees, in a conversation only where the server offers channel marks there;
    /// the server's emoji, then Unicode's.
    fn choices(&self, query: &Query, owner: &Owner) -> Vec<Choice> {
        let typed = query.text.to_lowercase();
        let matches = |name: &str| name.to_lowercase().contains(&typed);
        let starts = |name: &str| name.to_lowercase().starts_with(&typed);
        match query.trigger {
            Trigger::Member => {
                let mut members: Vec<Choice> = self
                    .directory
                    .people()
                    .filter(|(_, person)| matches(&person.name))
                    .map(|(id, person)| Choice::Member {
                        id: id.clone(),
                        name: person.name.clone(),
                        colour: person.colour,
                    })
                    .collect();
                members.sort_by_key(|choice| match choice {
                    Choice::Member { name, .. } => (!starts(name), name.to_lowercase()),
                    _ => (true, String::new()),
                });
                members.truncate(CHOICES_SHOWN);
                if !self.grants_of(owner).holds(Grant::MentionEveryone) {
                    return members;
                }
                let roles = self
                    .roles
                    .roles()
                    .iter()
                    .filter(|role| role.kind == crate::roles::RoleKind::Made && matches(&role.name))
                    .map(|role| Choice::Role { id: role.id.clone(), name: role.name.clone(), colour: role.colour });
                let everyone = [(Choice::Everyone, "everyone"), (Choice::Here, "here")]
                    .into_iter()
                    .filter(|(_, word)| starts(word))
                    .map(|(choice, _)| choice);
                members.into_iter().chain(roles.take(CHOICES_SHOWN)).chain(everyone).collect()
            }
            Trigger::Channel => {
                let Some(Lobby::Ready(listed)) = self.lobby() else { return Vec::new() };
                let category = |id: &ChannelId| {
                    let placed = &listed.placed.get(id)?.category;
                    listed.categories.iter().find(|category| category.id == *placed).map(|c| c.name.clone())
                };
                // A channel in no category the list holds sorts first, and the list leaves it out of its order.
                let uncategorised = listed
                    .text
                    .iter()
                    .map(|channel| (&channel.id, &channel.name, false))
                    .chain(listed.voice.iter().map(|channel| (&channel.id, &channel.name, true)))
                    .filter(|(id, _, _)| category(id).is_none());
                let placed = listed.in_order().into_iter().filter_map(|entry| match entry {
                    ListEntry::Text(channel) => Some((&channel.id, &channel.name, false)),
                    ListEntry::Voice(channel) => Some((&channel.id, &channel.name, true)),
                    ListEntry::Category(_) => None,
                });
                uncategorised
                    .chain(placed)
                    .filter(|(_, name, _)| matches(name))
                    .map(|(id, name, voice)| Choice::Channel {
                        id: id.clone(),
                        name: name.clone(),
                        voice,
                        category: category(id),
                    })
                    .take(CHOICES_SHOWN)
                    .collect()
            }
            Trigger::Emoji => {
                let custom = self.emoji.search(&query.text).into_iter().cloned().map(Choice::Custom);
                let unicode = chat::emoji::search(&query.text).map(Choice::Unicode);
                custom.chain(unicode).take(CHOICES_SHOWN).collect()
            }
        }
    }

    /// The next upload's id.
    fn next_upload(&mut self) -> Upload {
        self.uploads += 1;
        Upload(self.uploads)
    }

    /// `paths` attached to `compose`'s composer, each sent at once; past [`MAX_FILES`] in the strip, the rest are
    /// refused whole, saying so, and nothing of them is sent.
    pub(super) fn attach(&mut self, compose: &Compose, paths: Vec<std::path::PathBuf>) -> Vec<Ask> {
        let composing = &mut self.held.entry(compose.clone()).or_default().composing;
        composing.notice = None;
        if composing.files.len() + paths.len() > MAX_FILES {
            composing.notice = Some(Sentence::Phrase(Phrase::TooManyFiles));
            return Vec::new();
        }
        let mut todo = Vec::new();
        for path in paths {
            let upload = self.next_upload();
            let named = path.file_name().and_then(|name| name.to_str()).map(AttachmentName::parse);
            let Some(Ok(name)) = named else {
                let composing = &mut self.held.entry(compose.clone()).or_default().composing;
                composing.notice = Some(Sentence::Phrase(Phrase::FileNameRefused));
                continue;
            };
            let state = AttachState::Sending { sent: 0, of: None };
            let composing = &mut self.held.entry(compose.clone()).or_default().composing;
            composing.files.push(Attaching { upload, name: name.clone(), state });
            todo.push(Ask::Upload { compose: compose.clone(), upload, path, name });
        }
        todo
    }

    /// What an upload said of itself: its progress, or that it is kept or refused.
    pub(super) fn uploaded(
        &mut self,
        compose: &Compose,
        upload: Upload,
        said: Result<Uploaded, FileError>,
    ) -> Vec<Ask> {
        let limits = self.limits;
        let Some(file) = self
            .held
            .get_mut(compose)
            .and_then(|held| held.composing.files.iter_mut().find(|file| file.upload == upload))
        else {
            return Vec::new();
        };
        let told = match file.state {
            AttachState::Sending { of, .. } => of,
            AttachState::Ready { size, .. } => size,
            AttachState::Refused(_) => None,
        };
        file.state = match said {
            Ok(Uploaded::Sent { sent, of }) => AttachState::Sending { sent, of: Some(of) },
            Ok(Uploaded::Kept(id)) => AttachState::Ready { file: id, size: told },
            Err(failed) => AttachState::Refused(upload_refused(&failed, limits)),
        };
        Vec::new()
    }

    /// A file taken out of the strip: one still being sent is stopped.
    pub(super) fn remove_file(&mut self, compose: &Compose, upload: Upload) -> Vec<Ask> {
        let Some(composing) = self.held.get_mut(compose).map(|held| &mut held.composing) else { return Vec::new() };
        let Some(at) = composing.files.iter().position(|file| file.upload == upload) else { return Vec::new() };
        let removed = composing.files.remove(at);
        match removed.state {
            AttachState::Sending { .. } => vec![Ask::CancelUpload { upload }],
            AttachState::Ready { .. } | AttachState::Refused(_) => Vec::new(),
        }
    }

    /// The choice at `index` of the chooser over `compose`'s composer (its selected one with none) written in the
    /// draft, the cursor after it.
    pub(super) fn choose(&mut self, compose: &Compose, index: Option<usize>) {
        let Some(choosing) = self.choosing(compose) else { return };
        let Some(choice) = choosing.choices.get(index.unwrap_or(choosing.selected)) else { return };
        let completion = choice.completion();
        let Some(composing) = self.held.get_mut(compose).map(|held| &mut held.composing) else { return };
        if let Some(caret) = composing.draft.complete(&choosing.query, completion) {
            composing.caret = Some(caret);
            composing.caret_moved += 1;
            composing.selected = 0;
            composing.nonce = None;
        }
    }

    /// The chooser's selection moved by `by`, wrapping at either end.
    pub(super) fn select_choice(&mut self, compose: &Compose, by: isize) {
        let Some(choosing) = self.choosing(compose) else { return };
        let Some(composing) = self.held.get_mut(compose).map(|held| &mut held.composing) else { return };
        let count = isize::try_from(choosing.choices.len()).unwrap_or(isize::MAX);
        let at = isize::try_from(choosing.selected).unwrap_or(0);
        composing.selected = usize::try_from((at + by).rem_euclid(count)).unwrap_or(0);
    }

    /// Escape in the composer: the chooser goes first, then the reply.
    pub(super) fn escape(&mut self, compose: &Compose) {
        let query = self.choosing(compose).map(|choosing| choosing.query.range);
        let Some(composing) = self.held.get_mut(compose).map(|held| &mut held.composing) else { return };
        match query {
            Some(range) => composing.dismissed = Some(range),
            None => composing.reply = None,
        }
    }

    /// The composer's text changed: its marks follow the edit, and the server is told this account is writing, at
    /// most once every [`TYPING_EVERY`], where the composer has a timeline to be writing in.
    pub(super) fn drafted(&mut self, compose: &Compose, text: String, may_send: bool, now: Instant) -> Vec<Ask> {
        let held = self.held.entry(compose.clone()).or_default();
        let composing = &mut held.composing;
        if composing.draft.text() == text {
            return Vec::new();
        }
        composing.draft.edit(text);
        composing.nonce = None;
        composing.caret = None;
        composing.selected = 0;
        if composing
            .dismissed
            .as_ref()
            .is_some_and(|range| composing.draft.query().map(|q| q.range).as_ref() != Some(range))
        {
            composing.dismissed = None;
        }
        let writing = !composing.draft.text().trim().is_empty();
        let due = held.typed_at.is_none_or(|told| now.saturating_duration_since(told) >= TYPING_EVERY);
        let typing = compose.timeline().filter(|_| writing && due && may_send);
        if typing.is_some() {
            held.typed_at = Some(now);
        }
        self.link_followed(compose, now, may_send);
        typing.map(|timeline| Ask::Typing { timeline: timeline.clone() }).into_iter().collect()
    }
}

/// What an upload says of itself as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uploaded {
    /// So many bytes of its size are sent.
    Sent {
        /// Sent so far.
        sent: u64,
        /// Its size.
        of: u64,
    },
    /// The server kept it, as this file.
    Kept(FileId),
}

#[cfg(test)]
mod tests {
    use crate::WindowMessage;
    use crate::harness::{ADA, Clock, chatting, discussion, general_compose, member, standing, tell};
    use crate::place::{self, AttachState, Choice, DiscussionMessage};
    use proto::files::{FileKind, FileRefusal};
    use proto::rules::Grant;
    use std::time::{Duration, Instant};
    use words::Sentence;

    fn composing(window: &crate::window::Window) -> place::Composing {
        window.places()[0].composing(&general_compose()).cloned().unwrap_or_default()
    }

    fn upload_of(todo: &[crate::Ask]) -> place::Upload {
        let [crate::Ask::At(_, place::Ask::Upload { upload, .. })] = todo else { panic!("{todo:?}") };
        *upload
    }

    #[test]
    fn a_refused_upload_stays_in_the_strip_with_its_sentence_and_a_cancel_ends_its_request() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::AttachFiles]);
        let drop = |name: &str| discussion(DiscussionMessage::Dropped(vec![std::path::PathBuf::from(name)]));
        let first = upload_of(&tell(&mut window, &clock, owls, drop("season-recap.mp4")));
        let too_big = terms::FileError::Refused { kind: FileKind::Attachment, refusal: FileRefusal::TooLarge };
        let said = DiscussionMessage::Upload { compose: general_compose(), upload: first, said: Err(too_big) };
        tell(&mut window, &clock, owls, discussion(said));
        let refused = composing(&window).files().first().map(|file| file.state.clone());
        assert_eq!(refused, Some(AttachState::Refused(Sentence::FileTooLarge(None))), "back in the strip, saying why");
        let second = upload_of(&tell(&mut window, &clock, owls, drop("map-pack.zip")));
        let todo = tell(&mut window, &clock, owls, discussion(DiscussionMessage::RemoveFile(second)));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::CancelUpload { upload })] if *upload == second),
            "{todo:?}"
        );
        assert_eq!(composing(&window).files().len(), 1, "the refused one stays until it is removed");
        let todo = tell(&mut window, &clock, owls, discussion(DiscussionMessage::RemoveFile(first)));
        assert!(todo.is_empty() && composing(&window).files().is_empty(), "a refused one asks nothing to stop");
    }

    /// Opening a channel gives its composer the keyboard, as every peer app does, so what is typed next is a message.
    #[test]
    fn an_opened_channel_gives_its_composer_the_keyboard() {
        let clock = Clock(Instant::now());
        let (window, _) = chatting(&clock);
        assert_eq!(composing(&window).focus_asked(), 1, "asked once, as #general opened");
    }

    #[test]
    fn a_file_dropped_on_the_window_gives_its_composer_the_keyboard_so_enter_sends() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::AttachFiles]);
        let asked = composing(&window).focus_asked();
        let dropped = crate::window::Message::FilesDropped(vec![std::path::PathBuf::from("match-point.png")]);
        window.update(dropped, clock.at());
        assert_eq!(composing(&window).focus_asked(), asked + 1, "the field is asked to take the keyboard");
    }

    /// A picture needs no words: a file the server kept goes alone, where an empty composer sends nothing.
    #[test]
    fn a_file_the_server_kept_is_sent_with_no_text_and_nothing_is_sent_of_an_empty_composer() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::AttachFiles]);
        let nothing = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        assert!(nothing.is_empty(), "{nothing:?}");
        let dropped = DiscussionMessage::Dropped(vec![std::path::PathBuf::from("match-point.png")]);
        let upload = upload_of(&tell(&mut window, &clock, owls, discussion(dropped)));
        let file = proto::files::FileId::parse("0190f5a000007000800000000000aaaa").unwrap();
        let said = Ok(place::Uploaded::Kept(file));
        tell(
            &mut window,
            &clock,
            owls,
            discussion(DiscussionMessage::Upload { compose: general_compose(), upload, said }),
        );
        let todo = tell(&mut window, &clock, owls, discussion(DiscussionMessage::Send));
        let [crate::Ask::At(_, place::Ask::SendMessage { body, files, .. })] = &todo[..] else { panic!("{todo:?}") };
        assert_eq!((body.as_str(), files), ("", &vec![file]));
    }

    #[test]
    fn typing_is_told_at_most_once_every_three_seconds_while_the_draft_changes() {
        let mut clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let typed = |window: &mut crate::window::Window, clock: &Clock, text: &str| {
            let todo = tell(window, clock, owls, discussion(DiscussionMessage::Draft(text.into())));
            todo.iter().filter(|done| matches!(done, crate::Ask::At(_, place::Ask::Typing { .. }))).count()
        };
        assert_eq!(typed(&mut window, &clock, "h"), 1);
        assert_eq!(typed(&mut window, &clock, "he"), 0, "told already");
        clock.pass(Duration::from_millis(2_999));
        assert_eq!(typed(&mut window, &clock, "hel"), 0);
        clock.pass(Duration::from_millis(1));
        assert_eq!(typed(&mut window, &clock, "hell"), 1, "three seconds on");
        assert_eq!(typed(&mut window, &clock, "hell"), 0, "no change, nothing told");
    }

    #[test]
    fn a_mark_of_a_channel_the_reader_cannot_see_opens_nothing() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let (seen, unseen) = (crate::harness::channel_id("general"), crate::harness::channel_id("staff-only"));
        assert!(window.places()[0].opens_channel(&seen));
        assert!(!window.places()[0].opens_channel(&unseen), "drawn as its text alone");
        let pressed = crate::window::Message::Clicked(owls, crate::window::Clicked::Channel(unseen));
        assert!(window.update(pressed, clock.at()).is_empty(), "no press");
    }

    #[test]
    fn a_channel_in_no_category_is_offered_under_no_heading() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let mut listed = crate::harness::channels();
        listed.placed.clear();
        tell(&mut window, &clock, owls, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::Draft("see #".into())));
        let choosing = window.places()[0].choosing(&general_compose()).expect("a chooser");
        let offered: Vec<_> = choosing
            .choices
            .iter()
            .filter_map(|choice| match choice {
                Choice::Channel { name, category, .. } => Some((name.as_str(), category.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(offered, [("general", None), ("Lounge", None)], "offered, none left out");
    }

    #[test]
    fn the_member_chooser_offers_roles_and_everyone_only_with_mention_everyone() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let bo = (
            member("bo"),
            place::Named { name: "Bo".into(), colour: None, roles: Vec::new(), picture: None },
            proto::status::Shown::Online,
        );
        let page = place::PeoplePage { entries: vec![bo], next: None, presence: 0 };
        tell(&mut window, &clock, owls, place::Message::People(place::PeopleMessage::Page(Ok(page))));
        let regulars = crate::harness::role_entry("regulars", "Regulars", proto::rules::Place::at(2), &[]);
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::RolesListed(Ok(vec![regulars]))));
        let kinds = |window: &mut crate::window::Window| {
            tell(window, &clock, owls, discussion(DiscussionMessage::Draft("who’s hosting? @".into())));
            let choosing = window.places()[0].choosing(&general_compose()).expect("a chooser");
            choosing
                .choices
                .iter()
                .map(|choice| match choice {
                    Choice::Member { .. } => "member",
                    Choice::Role { .. } => "role",
                    Choice::Everyone => "everyone",
                    Choice::Here => "here",
                    _ => "other",
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(&mut window), ["member"], "a member sees members only");
        standing(&mut window, &clock, owls, &[Grant::ChannelRead, Grant::SendMessage, Grant::MentionEveryone]);
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::Draft(String::new())));
        assert_eq!(kinds(&mut window), ["member", "role", "everyone", "here"]);
        tell(&mut window, &clock, owls, discussion(DiscussionMessage::Choose(Some(1))));
        let draft = composing(&window).draft;
        assert_eq!(draft.text(), "who’s hosting? @Regulars ");
        assert_eq!(draft.marks().len(), 1, "the role's mark goes with its text");
        let _ = (ADA, WindowMessage::Tick);
    }
}
