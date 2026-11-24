//! What this account has read and who is writing: the open timeline marked read at its newest message while it is seen
//! at its end in a focused window, at most once a second; each channel's unread and mention counts, as the server
//! last said and counted on from what it tells by the rule the server counts by (`proto::notify`); and who is writing
//! in a timeline, each lapsing on their own.

use super::*;
use crate::loaded::Loaded;
use crate::place::ops;
use proto::notify::{ChannelSettings, Here, Level, Mark, Mentions, Reader, Said, ServerSettings};
use proto::rules::TYPING_LAPSE;

/// The least time between two read marks of one timeline.
pub const MARK_EVERY: Duration = Duration::from_secs(1);

/// How near its end a list counts as at its end, in pixels.
const AT_END: f32 = 2.0;

/// What this account has read of a timeline and how much is unread there, whoever owns it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readings {
    /// The revision read through.
    pub read: u64,
    /// The messages after it not this account's own.
    pub unread: u32,
    /// Of those, the ones that mention this account.
    pub mentions: u32,
}

impl From<ChannelStanding> for Readings {
    fn from(standing: ChannelStanding) -> Readings {
        Readings { read: standing.read, unread: standing.unread, mentions: standing.mentions }
    }
}

impl From<TimelineStanding> for Readings {
    fn from(standing: TimelineStanding) -> Readings {
        Readings { read: standing.read, unread: standing.unread, mentions: standing.mentions }
    }
}

/// The read marks a discussion sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marked {
    /// The revision last marked, and when.
    last: Option<(u64, Instant)>,
    /// A mark waits for [`MARK_EVERY`] to pass since the last.
    waiting: bool,
}

/// Who is writing in a timeline, as the window says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typers {
    /// One member, by the name others see.
    One(String),
    /// Two, by name.
    Two(String, String),
    /// More than two.
    Several,
}

impl Discussion {
    /// Whether its list shows its newest message: at its end, the newest held being the newest there is.
    fn at_newest(&self) -> bool {
        self.scrolled_up() <= AT_END && self.timeline().is_some_and(|timeline| !timeline.newer())
    }

    /// The read mark its timeline is due, while it is in view at its newest: none when it is marked already, or a mark
    /// went out less than [`MARK_EVERY`] ago, which it then waits for. With the revision it marks.
    fn mark(&mut self, read: u64, in_view: bool, now: Instant) -> Option<(u64, Ask)> {
        let revision = self.timeline().map(chat::Timeline::revision)?;
        let marked = self.marked.last.map_or(read, |(sent, _)| sent.max(read));
        if !in_view || !self.at_newest() || revision <= marked {
            self.marked.waiting = false;
            return None;
        }
        if let Some((_, at)) = self.marked.last
            && now.saturating_duration_since(at) < MARK_EVERY
        {
            self.marked.waiting = true;
            return None;
        }
        let timeline = self.timeline_id()?.clone();
        self.marked = Marked { last: Some((revision, now)), waiting: false };
        Some((revision, ops::mark_read(&timeline, revision)))
    }

    /// When a mark that waits may go.
    pub(crate) fn mark_due(&self) -> Option<Instant> {
        self.marked.last.filter(|_| self.marked.waiting).map(|(_, at)| at + MARK_EVERY)
    }
}

impl Place {
    /// Who is writing in `timeline` at `now`, this account left out; none for nobody.
    pub fn typing(&self, timeline: &TimelineId, now: Instant) -> Option<Typers> {
        let typers = self.typing.get(timeline)?;
        let names: Vec<String> = typers
            .iter()
            .filter(|(user, told)| Some(user) != self.me() && now < *told + TYPING_LAPSE)
            .map(|(user, _)| self.name_of(user).unwrap_or_else(|| user.as_str().to_owned()))
            .collect();
        match names.as_slice() {
            [] => None,
            [one] => Some(Typers::One(one.clone())),
            [first, second] => Some(Typers::Two(first.clone(), second.clone())),
            _ => Some(Typers::Several),
        }
    }

    /// The channel `id`'s standing: its words, its read mark, its counts.
    pub fn channel_standing(&self, id: &ChannelId) -> Option<ChannelStanding> {
        match self.lobby()? {
            Lobby::Ready(listed) => listed.standings.get(id).copied(),
            Lobby::Loading | Lobby::Failed(_) => None,
        }
    }

    /// Who is writing in a timeline, told whole.
    pub(super) fn typing_now(&mut self, timeline: TimelineId, typers: Vec<MemberId>, now: Instant) {
        let told: Vec<(MemberId, Instant)> = typers.into_iter().map(|user| (user, now)).collect();
        if told.is_empty() {
            self.typing.remove(&timeline);
        } else {
            self.typing.insert(timeline, told);
        }
    }

    /// Whether this window shows the server in a focused window, as the window last said, and what that makes due: the
    /// read marks of the timelines shown at their newest, and who stopped writing let go.
    pub(crate) fn viewed(&mut self, in_view: bool, now: Instant) -> Vec<Ask> {
        self.in_view = in_view;
        for typers in self.typing.values_mut() {
            typers.retain(|(_, told)| now < *told + TYPING_LAPSE);
        }
        self.typing.retain(|_, typers| !typers.is_empty());
        let mut todo = self.search_asks(now);
        todo.extend(self.previews_due(now));
        let Status::Signed(signed) = &mut self.status else { return todo };
        if !matches!(signed.lobby, Lobby::Ready(_)) {
            return todo;
        }
        let mut read = Vec::new();
        for pane in Pane::ALL {
            let Some((owner, timeline)) =
                signed.slot(pane).and_then(|open| Some((open.owner.clone(), open.timeline_id()?.clone())))
            else {
                continue;
            };
            let held = signed.tally(&owner, &timeline).map_or(0, |tally| tally.read);
            let Some((revision, mark)) = signed.slot_mut(pane).and_then(|open| open.mark(held, in_view, now)) else {
                continue;
            };
            signed.read_through(&owner, &timeline, revision);
            read.push((timeline, revision));
            todo.push(mark);
        }
        for (timeline, revision) in read {
            todo.extend(self.withdraw_if_read(&timeline, revision));
        }
        todo
    }

    /// A message told in `timeline`: a new one after the read mark, not this account's own nor read as it comes, is
    /// counted on what owns the timeline, a mention too where the rule both ends read says it is one; every message
    /// of a conversation is one, and a reply in a thread followed counts on its channel as well. What the rule says
    /// alerts is a sound for the window to make (`to_window`), and, under this device's popups, a popup asked of the
    /// host; do not disturb silences both. Its writer has stopped writing.
    pub(super) fn count_told(
        &mut self,
        timeline: &TimelineId,
        message: &chat::Message,
        to_window: &mut Option<Told>,
        wall: i64,
    ) -> Vec<Ask> {
        if let (Some(typers), chat::Author::Member(author)) = (self.typing.get_mut(timeline), &message.author) {
            typers.retain(|(user, _)| user != author);
        }
        let new = message.place == message.revision && !message.deleted();
        let (Some(counted), Some(me)) = (self.owner_of(timeline), self.me().cloned()) else { return Vec::new() };
        if !new {
            return Vec::new();
        }
        if matches!(counted, Owner::Conversation(_)) {
            self.conversation_told(timeline, message);
        }
        if let (Owner::Thread { .. }, Status::Signed(signed)) = (&counted, &mut self.status) {
            signed.talk.replied(timeline, message.created_at);
        }
        let in_view = self.reads_as_it_comes(timeline);
        let mut mentions = self.mentions_in(message, &me);
        if matches!(counted, Owner::Conversation(_)) {
            mentions.reader = true;
        }
        let notify = self.notify.clone().unwrap_or_default();
        let channel = match &counted {
            Owner::Channel(id) | Owner::Thread { channel: id, .. } => Some(id.clone()),
            Owner::Conversation(_) => None,
        };
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let own = match &counted {
            Owner::Channel(id) => match &mut signed.lobby {
                Lobby::Ready(listed) => Some(Counting::Channel(listed.standings.entry(id.clone()).or_default())),
                Lobby::Loading | Lobby::Failed(_) => None,
            },
            Owner::Conversation(_) | Owner::Thread { .. } => {
                Some(Counting::Timeline(signed.talk.standings.entry(timeline.clone()).or_default()))
            }
        };
        let Some(mut own) = own else { return Vec::new() };
        let (read, followed) = own.read_and_followed();
        if in_view || mentions.from_reader || message.place <= read {
            return Vec::new();
        }
        // The rule the server counts mentions by: what alerts at the level of mentions, with nothing muted.
        let counting = ServerSettings { level: Level::Mentions, mute: None, ..notify.server };
        let mark = proto::notify::tell(counting, ChannelSettings::default(), mentions, wall);
        own.count(mark == Mark::Counted);
        // A followed thread's reply counts on its channel too, as the server counts it.
        if matches!(counted, Owner::Thread { .. })
            && followed
            && let (Some(id), Lobby::Ready(listed)) = (&channel, &mut signed.lobby)
        {
            listed.standings.entry(id.clone()).or_default().count(mark == Mark::Counted);
        }
        // Only a conversation, or a thread this account follows, calls its reader by itself.
        if matches!(counted, Owner::Thread { .. }) && !followed {
            return Vec::new();
        }
        let settings = channel.as_ref().map_or_else(ChannelSettings::default, |id| notify.channel(id));
        let here = Here {
            now: wall,
            alerts_here: self.device.is_none_or(|device| device.alerts_here),
            quiet: self.quiet(wall),
        };
        // A followed thread calls its reader by itself, as a mention does: following is asking to hear.
        let mentions = Mentions { reader: mentions.reader || matches!(counted, Owner::Thread { .. }), ..mentions };
        let told = proto::notify::tell(notify.server, settings, mentions, wall);
        if !proto::notify::alerts(told, here) {
            return Vec::new();
        }
        to_window.get_or_insert(Told::Chime(Chime::Message));
        if self.popups != choices::Popups::On {
            return Vec::new();
        }
        self.popup(&counted, channel.as_ref(), timeline, message).into_iter().collect()
    }

    /// The popup for `message`, told in `timeline`: who wrote it, where, and the start of what it says. A newer one
    /// for the same timeline replaces the older.
    fn popup(
        &mut self,
        counted: &Owner,
        channel: Option<&ChannelId>,
        timeline: &TimelineId,
        message: &chat::Message,
    ) -> Option<Ask> {
        let author = match &message.author {
            chat::Author::Member(author) => self.name_of(author).unwrap_or_else(|| author.as_str().to_owned()),
            chat::Author::App { name, .. } => name.clone(),
            chat::Author::Former(_) => return None,
        };
        let text = message.body.as_ref()?.shown();
        let server = self.server.shown().to_string();
        let channel = channel.and_then(|id| self.channel_name(id)).map(str::to_owned).unwrap_or_default();
        let (title, body) = match counted {
            Owner::Channel(_) => (Sentence::PopupTitle { author, server }, Sentence::PopupChannel { channel, text }),
            Owner::Conversation(_) => (Sentence::Plain(author), Sentence::PopupDirect { server, text }),
            Owner::Thread { .. } => (Sentence::PopupTitle { author, server }, Sentence::PopupThread { channel, text }),
        };
        self.popped.insert(timeline.clone(), message.place);
        Some(Ask::Popup { timeline: timeline.clone(), title, body })
    }

    /// The popup for `timeline` taken away if what it shows is read through `read`, here or on another device.
    pub(crate) fn withdraw_if_read(&mut self, timeline: &TimelineId, read: u64) -> Option<Ask> {
        let shown = *self.popped.get(timeline)?;
        (shown <= read).then(|| {
            self.popped.remove(timeline);
            Ask::Withdraw { timeline: timeline.clone() }
        })
    }

    /// The popups taken away for a channel read through `read`: its own timeline's.
    pub(crate) fn withdraw_channel(&mut self, channel: &ChannelId, read: u64) -> Option<Ask> {
        let timeline = match self.lobby()? {
            Lobby::Ready(listed) => {
                let text = listed.text.iter().find(|held| held.id == *channel).map(|held| held.timeline.clone());
                text.or_else(|| listed.voice.iter().find(|held| held.id == *channel).map(|held| held.timeline.clone()))?
            }
            Lobby::Loading | Lobby::Failed(_) => return None,
        };
        self.withdraw_if_read(&timeline, read)
    }

    /// Whether `timeline` is shown at its newest in a focused window, so what is told there is read as it comes.
    fn reads_as_it_comes(&self, timeline: &TimelineId) -> bool {
        let Status::Signed(signed) = &self.status else { return false };
        self.in_view && signed.open().any(|open| open.shows(timeline) && open.at_newest())
    }

    /// Whether `message` calls this account, as its row is marked: by name, a role it holds, `@everyone` or `@here`,
    /// or a reply to it that mentions its author; never one of its own.
    pub fn mentions_me(&self, message: &chat::Message) -> bool {
        let Some(me) = self.me() else { return false };
        let said = self.mentions_in(message, me);
        !said.from_reader && (said.reader || said.role || said.everyone)
    }

    /// What `message` says of this account, `me`.
    fn mentions_in(&self, message: &chat::Message, me: &MemberId) -> Mentions {
        let spans = message
            .body
            .as_ref()
            .and_then(|body| proto::rules::spans_stored(body.text(), &chat::spans_of(body)).ok())
            .unwrap_or_default();
        let author = match &message.author {
            chat::Author::Member(author) => Some(author.as_str()),
            chat::Author::Former(_) | chat::Author::App { .. } => None,
        };
        let replied =
            message.reply.as_ref().filter(|_| message.mention_author).and_then(|reply| self.author_of(&reply.id));
        let roles: Vec<String> = self
            .directory
            .person(me)
            .map(|person| person.roles.iter().map(|role| role.as_str().to_owned()).collect())
            .unwrap_or_default();
        let said = Said { spans: &spans, author, mentions_replied: replied.as_ref().map(MemberId::as_str) };
        // Told as it is sent, so this account is signed in at its sending.
        proto::notify::mentions_of(said, Reader { user_id: me.as_str(), role_ids: &roles, online_at_send: true })
    }

    /// Who wrote the message `id`, where an open timeline holds it.
    fn author_of(&self, id: &chat::MessageId) -> Option<MemberId> {
        let Status::Signed(signed) = &self.status else { return None };
        signed.open().find_map(|open| match &open.timeline()?.get(id)?.author {
            chat::Author::Member(author) => Some(author.clone()),
            chat::Author::Former(_) | chat::Author::App { .. } => None,
        })
    }
}

/// A standing being counted on, of a channel or of a conversation or thread.
enum Counting<'a> {
    Channel(&'a mut ChannelStanding),
    Timeline(&'a mut TimelineStanding),
}

impl Counting<'_> {
    /// The read mark, and whether the timeline is a thread this account follows.
    fn read_and_followed(&self) -> (u64, bool) {
        match self {
            Counting::Channel(standing) => (standing.read, false),
            Counting::Timeline(standing) => (standing.read, standing.followed),
        }
    }

    fn count(&mut self, mentioned: bool) {
        match self {
            Counting::Channel(standing) => standing.count(mentioned),
            Counting::Timeline(standing) => standing.count(mentioned),
        }
    }
}

/// What a server's unread messages make of the badge: the number that alert, and whether any quiet ones are unread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unread {
    /// The unread messages that would alert: in no muted channel, at the level of each.
    pub alerting: u32,
    /// Some unread message does not alert, and the badge shows a dot when none does.
    pub quiet: bool,
}

/// A server's unread as the badge counts it, and whether it is the last count held of a server not connected now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaceUnread {
    /// The count.
    pub unread: Unread,
    /// The server is not connected: the count is the last one held.
    pub stale: bool,
}

impl Place {
    /// What this server's unread messages make of the badge at `wall` (seconds since the epoch): none where no account
    /// is signed in; the last count held, marked stale, while the connection is made again.
    pub fn unread(&self, wall: i64) -> Option<PlaceUnread> {
        match &self.status {
            Status::Signed(signed) => Some(PlaceUnread { unread: self.unread_in(signed, wall), stale: false }),
            Status::Reconnecting(again) => Some(PlaceUnread { unread: again.unread, stale: true }),
            Status::Door(_) => None,
        }
    }

    /// The count of the signed-in state `signed`: a muted channel or server has no part of it, the others count as the
    /// level each holds says; every message of a conversation names its reader.
    fn unread_in(&self, signed: &SignedIn, wall: i64) -> Unread {
        let notify = self.notify.clone().unwrap_or_default();
        let mut sum = Unread::default();
        let mut add = |unread: u32, mentions: u32, level: Level| match level {
            Level::All => sum.alerting += unread,
            Level::Mentions => {
                sum.alerting += mentions;
                sum.quiet |= unread > mentions;
            }
            Level::Nothing => sum.quiet |= unread > 0,
        };
        if let Lobby::Ready(listed) = &signed.lobby {
            for (id, standing) in &listed.standings {
                if !notify.muted(id, wall) {
                    add(standing.unread, standing.mentions, notify.level(id));
                }
            }
        }
        let server_muted = notify.server.mute.is_some_and(|mute| mute.holds_at(wall));
        if let Loaded::Ready(listed) = &signed.talk.conversations
            && !server_muted
        {
            for conversation in listed {
                let unread = signed.talk.standing(&conversation.timeline).unread;
                add(unread, unread, notify.server.level);
            }
        }
        sum
    }
}

impl SignedIn {
    /// The discussion open in `pane`: a call's chat only while a call is here.
    fn slot(&self, pane: Pane) -> Option<&Discussion> {
        match pane {
            Pane::Content => self.content.discussion(),
            Pane::CallChat => self.call_chat.as_ref().filter(|_| self.call.is_some()),
            Pane::Thread => self.content.thread(),
        }
    }

    /// The same, to change.
    fn slot_mut(&mut self, pane: Pane) -> Option<&mut Discussion> {
        match pane {
            Pane::Content => self.content.discussion_mut(),
            Pane::CallChat if self.call.is_none() => None,
            Pane::CallChat => self.call_chat.as_mut(),
            Pane::Thread => self.content.beside().and_then(Option::as_mut),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::WindowMessage;
    use crate::harness::{ADA, Clock, chatting, discussion, general_timeline, member, said, tell, told};
    use crate::place::{self, DiscussionMessage, Typers};
    use proto::chatkord_v1::{ChannelStanding, Event, event};
    use std::time::{Duration, Instant};

    fn marks(todo: &[crate::Ask]) -> Vec<u64> {
        todo.iter()
            .filter_map(|done| match done {
                crate::Ask::At(_, ask) => match ask.op() {
                    Some(crate::harness::Body::MarkRead(mark)) => Some(mark.revision),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_open_timeline_is_marked_read_at_its_newest_in_a_focused_window_at_most_once_a_second() {
        let mut clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        assert_eq!(marks(&window.update(WindowMessage::Focused, clock.at())), [5], "focused at its end: marked");
        let todo = tell(&mut window, &clock, owls, told(said(2, 6, "bo", "hi")));
        assert_eq!(marks(&todo), Vec::<u64>::new(), "a second has not passed");
        let due = clock.0 + super::MARK_EVERY;
        assert!(todo.iter().any(|done| matches!(done, crate::Ask::WakeAt(when) if *when == due)), "{todo:?}");
        clock.pass(Duration::from_millis(999));
        assert!(marks(&window.update(WindowMessage::Tick, clock.at())).is_empty());
        clock.pass(Duration::from_millis(1));
        assert_eq!(marks(&window.update(WindowMessage::Tick, clock.at())), [6], "a second on, the newest");
        window.update(WindowMessage::FocusLost, clock.at());
        clock.pass(Duration::from_secs(5));
        assert!(marks(&tell(&mut window, &clock, owls, told(said(3, 7, "bo", "there")))).is_empty(), "not focused");
    }

    #[test]
    fn a_message_calls_its_reader_by_name_but_never_by_their_own_hand() {
        let clock = Clock(Instant::now());
        let (window, _) = chatting(&clock);
        let place = &window.places()[0];
        let mark = chat::Mark { range: 0..4, kind: chat::MarkKind::Mention(member(ADA)) };
        let mut calling = said(2, 6, "bo", "@ada hi");
        calling.body = Some(chat::Body::new("@ada hi", vec![mark.clone()]).unwrap());
        assert!(place.mentions_me(&calling), "bo names ada");
        let mut own = said(3, 7, ADA, "@ada me");
        own.body = Some(chat::Body::new("@ada me", vec![mark]).unwrap());
        assert!(!place.mentions_me(&own), "never one's own");
        assert!(!place.mentions_me(&said(4, 8, "bo", "hello")), "nor one naming no one");
    }

    #[test]
    fn unread_and_mentions_come_from_the_standing_then_count_on_from_what_is_told() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let standing = ChannelStanding {
            channel_id: "general".into(),
            grants: vec!["CHANNEL_READ".into(), "SEND_MESSAGE".into()],
            read: 5,
            unread: 3,
            mentions: 1,
        };
        let heard =
            place::Message::heard(&Event { cursor: Vec::new(), kind: Some(event::Kind::ChannelStanding(standing)) });
        tell(&mut window, &clock, owls, heard.expect("read"));
        let counts = |window: &crate::window::Window| {
            let standing = window.places()[0].channel_standing(&crate::harness::channel_id("general")).unwrap();
            (standing.unread, standing.mentions)
        };
        assert_eq!(counts(&window), (3, 1), "as the server counted");
        tell(&mut window, &clock, owls, told(said(6, 6, "bo", "hey")));
        assert_eq!(counts(&window), (4, 1));
        let mut mention = said(7, 7, "bo", "@Ada you up?");
        let mark = chat::Mark { range: 0..4, kind: chat::MarkKind::Mention(member(ADA)) };
        mention.body = Some(chat::Body::new("@Ada you up?", vec![mark]).unwrap());
        tell(&mut window, &clock, owls, told(mention));
        assert_eq!(counts(&window), (5, 2), "a mention counts on both");
        tell(&mut window, &clock, owls, told(said(6, 9, "bo", "hey, edited")));
        tell(&mut window, &clock, owls, told(said(8, 8, ADA, "mine")));
        assert_eq!(counts(&window), (5, 2), "an edit and one's own are not new to read");
    }

    #[test]
    fn a_writer_shows_until_they_lapse_or_send() {
        let mut clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let typing = DiscussionMessage::Typing { timeline: general_timeline(), users: vec![member("bo"), member(ADA)] };
        tell(&mut window, &clock, owls, discussion(typing));
        let shown =
            |window: &crate::window::Window, clock: &Clock| window.places()[0].typing(&general_timeline(), clock.0);
        assert_eq!(shown(&window, &clock), Some(Typers::One("bo".into())), "this account is never shown");
        clock.pass(proto::rules::TYPING_LAPSE - Duration::from_millis(1));
        assert!(shown(&window, &clock).is_some());
        clock.pass(Duration::from_millis(1));
        assert_eq!(shown(&window, &clock), None, "lapsed");
        let again = DiscussionMessage::Typing { timeline: general_timeline(), users: vec![member("bo")] };
        tell(&mut window, &clock, owls, discussion(again));
        tell(&mut window, &clock, owls, told(said(9, 9, "bo", "done")));
        assert_eq!(shown(&window, &clock), None, "a send ends it");
    }
}
