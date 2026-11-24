//! Threads: the ones this account follows, and opening, starting and following one.

use super::*;

/// A thread this account follows, as the Threads list holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowedThread {
    /// Its timeline.
    pub timeline: TimelineId,
    /// The channel its root is in.
    pub channel: ChannelId,
    /// Its root message.
    pub root: MessageId,
    /// The root in brief; none once it was deleted.
    pub preview: Option<Brief>,
    /// When its last living reply was sent, in seconds since the epoch; none with none.
    pub last_reply_at: Option<i64>,
    /// Its living replies, as the server counted them when it listed the thread.
    pub replies: u32,
}

impl FollowedThread {
    fn read(told: &wire::FollowedThread) -> Option<(FollowedThread, TimelineId, TimelineStanding)> {
        let standing = told.standing.as_ref()?;
        let timeline = TimelineId::parse(&standing.timeline_id)?;
        let thread = FollowedThread {
            timeline: timeline.clone(),
            channel: ChannelId::parse(&told.channel_id)?,
            root: MessageId::parse(&told.root_message_id)?,
            preview: told.root.as_ref().map(Brief::read),
            last_reply_at: told.last_reply_at,
            replies: told.replies,
        };
        Some((thread, timeline, TimelineStanding::read(standing)))
    }
}

/// The root of a thread, for the pane that shows it on top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Root {
    /// The message whole, from the channel's timeline.
    Whole(Box<chat::Message>),
    /// What the Threads list knows of it.
    Brief {
        /// Its id.
        message: MessageId,
        /// The start of it; none once it was deleted.
        preview: Option<Brief>,
    },
}

impl Root {
    /// The root message's id.
    pub fn message(&self) -> &MessageId {
        match self {
            Root::Whole(message) => &message.id,
            Root::Brief { message, .. } => message,
        }
    }

    /// When it was sent, in seconds since the epoch; none once it was deleted and only its id is known.
    pub fn sent_at(&self) -> Option<i64> {
        match self {
            Root::Whole(message) => Some(message.created_at),
            Root::Brief { preview, .. } => preview.as_ref().map(|brief| brief.at),
        }
    }
}

/// A followed thread as the list's row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadRow<'a> {
    /// The thread.
    pub thread: &'a FollowedThread,
    /// Its channel's name.
    pub channel: &'a str,
    /// Who wrote its root, as others see them.
    pub author: Option<String>,
    /// What this account has read of it.
    pub standing: TimelineStanding,
    /// Its root in brief.
    pub preview: Option<PreviewLine<'a>>,
}

impl Talk {
    /// The followed thread with this timeline.
    pub(super) fn thread(&self, timeline: &TimelineId) -> Option<&FollowedThread> {
        match &self.followed {
            Loaded::Ready(listed) => listed.iter().find(|held| held.timeline == *timeline),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    /// A reply told live in the followed thread `timeline`, sent at `at`: its row counts it and takes its time.
    pub(crate) fn replied(&mut self, timeline: &TimelineId, at: i64) {
        let Loaded::Ready(listed) = &mut self.followed else { return };
        if let Some(held) = listed.iter_mut().find(|held| held.timeline == *timeline) {
            held.replies = held.replies.saturating_add(1);
            held.last_reply_at = Some(at);
        }
    }
}

impl Place {
    /// The followed threads as the list shows them, ordered as the conversations are; a thread whose channel is not
    /// listed is left out.
    pub fn thread_rows(&self) -> Rows<ThreadRow<'_>> {
        let Status::Signed(signed) = &self.status else { return Rows::Loading };
        let Lobby::Ready(channels) = &signed.lobby else { return Rows::Loading };
        match &signed.talk.followed {
            Loaded::Loading => Rows::Loading,
            Loaded::Failed(failed) => Rows::Failed(failed.clone()),
            Loaded::Ready(listed) => {
                let me = self.me();
                let mut rows: Vec<ThreadRow<'_>> = listed
                    .iter()
                    .filter_map(|thread| {
                        let channel = match channels.find(&thread.channel)? {
                            ListEntry::Text(channel) => channel.name.as_str(),
                            ListEntry::Voice(channel) => channel.name.as_str(),
                            ListEntry::Category(_) => return None,
                        };
                        Some(ThreadRow {
                            thread,
                            channel,
                            author: thread.preview.as_ref().and_then(|root| self.name_of(root.from.as_ref()?)),
                            standing: signed.talk.standing(&thread.timeline),
                            preview: thread.preview.as_ref().map(|root| PreviewLine {
                                mine: root.from.is_some() && root.from.as_ref() == me,
                                text: &root.text,
                                at: thread.last_reply_at.unwrap_or(root.at),
                            }),
                        })
                    })
                    .collect();
                rows.sort_by_key(|row| {
                    (row.standing.unread == 0, std::cmp::Reverse(row.preview.map_or(i64::MIN, |l| l.at)))
                });
                Rows::Ready(rows)
            }
        }
    }

    /// The unread messages across the threads this account follows, for the side pane's "Threads" row.
    pub fn threads_unread(&self) -> u32 {
        let Status::Signed(signed) = &self.status else { return 0 };
        match &signed.talk.followed {
            Loaded::Ready(listed) => listed.iter().map(|thread| signed.talk.standing(&thread.timeline).unread).sum(),
            Loaded::Loading | Loaded::Failed(_) => 0,
        }
    }

    /// Whether this account follows the thread `timeline`.
    pub fn follows(&self, timeline: &TimelineId) -> bool {
        match &self.status {
            Status::Signed(signed) => signed.talk.standing(timeline).followed,
            Status::Reconnecting(_) | Status::Door(_) => false,
        }
    }

    /// The thread open beside the channel in the content area.
    pub fn thread(&self) -> Option<&Discussion> {
        match &self.status {
            Status::Signed(signed) => signed.content.thread(),
            Status::Reconnecting(_) | Status::Door(_) => None,
        }
    }

    /// The followed thread `timeline` open in the content area, reached from the Threads list.
    pub(super) fn open_followed(&mut self, timeline: &TimelineId) -> Vec<Ask> {
        let Status::Signed(signed) = &self.status else { return Vec::new() };
        let Some(thread) = signed.talk.thread(timeline).cloned() else { return Vec::new() };
        let owner = Owner::Thread { channel: thread.channel.clone(), root: thread.root.clone() };
        let root = Root::Brief { message: thread.root, preview: thread.preview };
        self.seat(owner, thread.timeline, Some(root), Some(Roll::Threads))
    }

    /// The thread on the message `message` of the open channel opened beside it.
    pub(super) fn open_thread(&mut self, message: &MessageId) -> Vec<Ask> {
        let Status::Signed(signed) = &self.status else { return Vec::new() };
        let Some(content) = signed.content.discussion() else { return Vec::new() };
        let Owner::Channel(channel) = &content.owner else { return Vec::new() };
        let held = content.timeline().and_then(|timeline| timeline.get(message)).filter(|held| !held.deleted());
        let Some((held, summary)) = held.and_then(|held| Some((held, held.thread.as_ref()?))) else {
            return Vec::new();
        };
        let (channel, timeline) = (channel.clone(), summary.timeline.clone());
        let root = Root::Whole(Box::new(held.clone()));
        self.seat_thread(channel, root, timeline)
    }

    /// Opens the thread `timeline` of `channel` beside it, with `root` on top.
    fn seat_thread(&mut self, channel: ChannelId, root: Root, timeline: TimelineId) -> Vec<Ask> {
        let zone = self.zone.clone();
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let owner = Owner::Thread { channel, root: root.message().clone() };
        if signed.content.thread().is_some_and(|open| open.owner == owner) {
            return Vec::new();
        }
        let tally = signed.tally(&owner, &timeline);
        let Some(beside) = signed.content.beside() else { return Vec::new() };
        if let Some(left) = beside.take() {
            leave(&mut self.held, left);
        }
        let scroll = self.held.get(&Compose::Timeline(timeline.clone())).map(|held| held.scroll.clone());
        let scroll = scroll.unwrap_or_default();
        let open = Discussion::new(owner, timeline, zone, scroll).tally(tally).reached(None, Some(root));
        let first = open.first_page();
        *beside = Some(open);
        first.into_iter().collect()
    }

    /// Opens beside the channel the thread `root` will have once a first reply is sent, its composer taking the
    /// keyboard.
    fn seat_new_thread(&mut self, channel: ChannelId, root: Root) -> Vec<Ask> {
        let zone = self.zone.clone();
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let owner = Owner::Thread { channel, root: root.message().clone() };
        if signed.content.thread().is_some_and(|open| open.owner == owner) {
            return Vec::new();
        }
        let Some(beside) = signed.content.beside() else { return Vec::new() };
        if let Some(left) = beside.take() {
            leave(&mut self.held, left);
        }
        let open = Discussion::new_thread(owner, root, zone);
        self.held.entry(open.compose.clone()).or_default().composing.focus_asked += 1;
        *beside = Some(open);
        Vec::new()
    }

    /// "Reply in thread": the message's thread opens beside the channel, a composer to begin it when it has none.
    pub(super) fn start_thread(&mut self, message: &MessageId) -> Vec<Ask> {
        // Chosen from the message's menu, which, left open, keeps its first item focused under the thread's composer.
        if let Some(open) = self.pane_mut(Pane::Content) {
            open.menu = None;
        }
        if !self.served.on(proto::rules::Switch::Threads) {
            return Vec::new();
        }
        let Some(content) = self.discussion() else { return Vec::new() };
        let Owner::Channel(channel) = &content.owner else { return Vec::new() };
        let Some(held) = content.timeline().and_then(|timeline| timeline.get(message)).filter(|m| !m.deleted()) else {
            return Vec::new();
        };
        if held.thread.is_some() {
            return self.open_thread(message);
        }
        if !self.grants_of(&content.owner).holds(Grant::SendMessage) {
            return Vec::new();
        }
        let (channel, root) = (channel.clone(), Root::Whole(Box::new(held.clone())));
        self.seat_new_thread(channel, root)
    }

    /// Follows or unfollows the thread `timeline`, shown at once; the server's word follows.
    pub(super) fn follow_thread(&mut self, timeline: &TimelineId, follow: bool) -> Vec<Ask> {
        if !self.served.on(proto::rules::Switch::Threads) {
            return Vec::new();
        }
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let is_thread = signed.open().any(|open| open.shows(timeline) && matches!(open.owner, Owner::Thread { .. }));
        if !is_thread {
            return Vec::new();
        }
        let standing = signed.talk.standings.entry(timeline.clone()).or_default();
        if standing.followed == follow {
            return Vec::new();
        }
        standing.followed = follow;
        vec![ops::follow_thread(timeline.clone(), follow)]
    }

    pub(super) fn followed_listed(&mut self, answer: Result<wire::FollowedThreads, LinkError>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match answer {
            Ok(told) => {
                let mut listed = Vec::new();
                for (thread, timeline, standing) in told.threads.iter().filter_map(FollowedThread::read) {
                    let kept = signed.talk.standings.entry(timeline).or_default();
                    *kept = kept.over(TimelineStanding { followed: true, ..standing });
                    listed.push(thread);
                }
                signed.talk.followed = Loaded::Ready(listed);
            }
            Err(failed) => signed.talk.followed = Loaded::Failed(failed),
        }
        Vec::new()
    }

    pub(super) fn close_thread(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(left) = signed.content.beside().and_then(Option::take) {
            leave(&mut self.held, left);
        }
        Vec::new()
    }

    pub(super) fn follow_answered(
        &mut self,
        timeline: &TimelineId,
        follow: bool,
        answer: Result<(), LinkError>,
    ) -> Vec<Ask> {
        match answer {
            Ok(()) => Vec::new(),
            Err(failed) => {
                let Status::Signed(signed) = &mut self.status else { return Vec::new() };
                if let Some(standing) = signed.talk.standings.get_mut(timeline) {
                    standing.followed = !follow;
                }
                if let Some(open) = signed.open_mut().find(|open| open.shows(timeline)) {
                    open.refused = Some(failed);
                }
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, general_timeline, said, tell, timeline_id, told};
    use crate::place;
    use crate::place::talk::tests::{asks, talk, talkative};
    use crate::place::{Owner, Panel, Roll, TalkMessage, TimelineStanding};
    use proto::chatkord_v1 as wire;
    use std::time::Instant;
    use terms::LinkError;

    fn with_a_threaded_message(clock: &Clock) -> (crate::window::Window, place::PlaceId) {
        let (mut window, owls) = talkative(clock);
        let mut root = said(2, 6, "bo", "who is in?");
        root.thread = Some(chat::ThreadSummary { timeline: timeline_id("t-thread"), replies: 2, last_reply_at: None });
        tell(&mut window, clock, owls, told(root));
        (window, owls)
    }

    #[test]
    fn a_thread_chip_opens_the_pane_beside_the_channel_and_follow_toggles_with_the_servers_word() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = with_a_threaded_message(&clock);
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::OpenThread(crate::harness::message_id(2))));
        assert!(
            matches!(&asks(&todo)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == timeline_id("t-thread").as_str())),
            "{todo:?}"
        );
        let place = &window.places()[0];
        assert!(place.discussion().is_some(), "the channel stays");
        let pane = place.thread().expect("the pane");
        assert!(
            matches!(&pane.owner, Owner::Thread { channel, root } if channel.as_str() == "general" && root.as_str() == "m2")
        );
        assert!(place.may_send(place::Pane::Thread), "a thread asks its channel's words");
        let thread = timeline_id("t-thread");
        let day = 24 * 3_600;
        let replies = vec![
            said(1, 1, "bo", "me"),
            chat::Message { created_at: 1_763_035_200 + day, ..said(2, 2, "bo", "still?") },
        ];
        let page = chat::Page { messages: replies, older: false, newer: false, revision: 2 };
        let read = place::DiscussionMessage::Page { timeline: thread.clone(), end: None, page: Ok(page) };
        tell(&mut window, &clock, owls, place::Message::Thread(read));
        let scrolled = place::DiscussionMessage::Scrolled { from_end: 0.0, viewport: 600.0 };
        tell(&mut window, &clock, owls, place::Message::Thread(scrolled));
        let pane = window.places()[0].thread().expect("the pane");
        let days = pane.rows().iter().filter(|row| matches!(row, chat::Row::Day(_))).count();
        assert_eq!((pane.rows().len(), days), (3, 1), "the root's own day is not said again, the next day is");
        assert!(matches!(pane.rows()[0], chat::Row::Message { .. }));
        assert_eq!(pane.room_above(), 0.0, "replies stand under the root, not on the pane's end");
        let todo =
            tell(&mut window, &clock, owls, talk(TalkMessage::Follow { timeline: thread.clone(), follow: true }));
        assert!(
            matches!(&asks(&todo)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::FollowThread(follow)) if follow.follow)),
            "{todo:?}"
        );
        assert!(window.places()[0].follows(&thread), "shown at once");
        let refused = Err(LinkError::Timeout);
        tell(
            &mut window,
            &clock,
            owls,
            talk(TalkMessage::FollowAnswered { timeline: thread.clone(), follow: true, answer: refused }),
        );
        assert!(!window.places()[0].follows(&thread), "put back when the server did not say yes");
        assert!(window.places()[0].thread().is_some_and(|pane| pane.refused.is_some()));
        tell(&mut window, &clock, owls, talk(TalkMessage::Follow { timeline: thread.clone(), follow: true }));
        let told = TimelineStanding { followed: true, ..TimelineStanding::default() };
        tell(&mut window, &clock, owls, talk(TalkMessage::Standing { timeline: thread.clone(), standing: told }));
        let todo =
            tell(&mut window, &clock, owls, talk(TalkMessage::Follow { timeline: thread.clone(), follow: false }));
        assert!(
            matches!(&asks(&todo)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::FollowThread(follow)) if !follow.follow)),
            "{todo:?}"
        );
        assert!(!window.places()[0].follows(&thread));
        tell(&mut window, &clock, owls, talk(TalkMessage::CloseThread));
        assert!(window.places()[0].thread().is_none());
    }

    #[test]
    fn reply_in_thread_opens_a_composer_where_there_is_no_thread_and_its_first_reply_begins_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = with_a_threaded_message(&clock);
        tell(&mut window, &clock, owls, told(said(3, 7, "bo", "plain")));
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::StartThread(crate::harness::message_id(3))));
        assert!(asks(&todo).is_empty(), "a thread is begun by its first reply, nothing is asked yet: {todo:?}");
        let begun = place::Compose::NewThread(crate::harness::message_id(3));
        let pane = window.places()[0].thread().expect("the pane");
        assert!(pane.compose == begun && pane.timeline_id().is_none(), "no timeline to read");
        let thread = |message| place::Message::Thread(message);
        let typed = tell(&mut window, &clock, owls, thread(place::DiscussionMessage::Draft("a first reply".into())));
        assert!(asks(&typed).is_empty(), "a thread yet to begin tells no one it is being written: {typed:?}");
        let todo = tell(&mut window, &clock, owls, thread(place::DiscussionMessage::Send));
        assert!(
            matches!(
                &asks(&todo)[..],
                [place::Ask::SendMessage { timeline, thread_of: Some(root), .. }]
                    if *timeline == general_timeline() && root.as_str() == "m3"
            ),
            "the first reply goes to the channel's timeline, naming the root: {todo:?}"
        );
        tell(&mut window, &clock, owls, thread(place::DiscussionMessage::Draft("and a second".into())));
        let mut root = crate::harness::said(3, 8, "bo", "plain");
        let made = timeline_id("t-new");
        root.thread = Some(chat::ThreadSummary { timeline: made.clone(), replies: 1, last_reply_at: None });
        let todo = tell(&mut window, &clock, owls, told(root));
        assert!(
            matches!(&asks(&todo)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == made.as_str())),
            "the thread's replies are read once it exists: {todo:?}"
        );
        let place = &window.places()[0];
        assert_eq!(place.thread().map(|pane| pane.compose.clone()), Some(place::Compose::Timeline(made.clone())));
        let kept = place.composing(&place::Compose::Timeline(made)).map(|composing| composing.draft.text());
        assert_eq!(kept, Some("and a second"), "what was typed meanwhile is the thread's draft now");
        assert_eq!(place.unsent(&begun).len(), 1, "the send awaiting its answer is found where it was made");
        let again = tell(&mut window, &clock, owls, talk(TalkMessage::StartThread(crate::harness::message_id(2))));
        assert!(
            matches!(&asks(&again)[..], [ask] if matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == timeline_id("t-thread").as_str())),
            "one that has a thread opens it: {again:?}"
        );
    }

    #[test]
    fn reply_in_thread_chosen_from_a_messages_menu_puts_the_menu_away() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = with_a_threaded_message(&clock);
        tell(&mut window, &clock, owls, told(said(3, 7, "bo", "plain")));
        let menu = place::DiscussionMessage::Menu(crate::harness::message_id(3));
        tell(&mut window, &clock, owls, place::Message::Discussion(menu));
        assert!(window.places()[0].discussion().is_some_and(|open| open.menu.is_some()), "the menu is open");
        tell(&mut window, &clock, owls, talk(TalkMessage::StartThread(crate::harness::message_id(3))));
        assert!(window.places()[0].discussion().is_some_and(|open| open.menu.is_none()), "its choice put it away");
    }

    #[test]
    fn the_threads_list_shows_followed_threads_and_opens_one_with_its_root_brief() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = talkative(&clock);
        let followed = wire::FollowedThreads {
            threads: vec![wire::FollowedThread {
                standing: Some(wire::TimelineStanding {
                    timeline_id: "t-thread".into(),
                    read: 3,
                    unread: 4,
                    mentions: 1,
                    followed: true,
                }),
                channel_id: "general".into(),
                root_message_id: "m2".into(),
                root: Some(wire::Preview { user_id: Some("bo".into()), text: "who is in?".into(), created_at: 9 }),
                last_reply_at: Some(20),
                replies: 2,
            }],
        };
        tell(&mut window, &clock, owls, talk(TalkMessage::Followed(Ok(followed))));
        tell(&mut window, &clock, owls, talk(TalkMessage::Show(Roll::Threads)));
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.threads_unread()), (Panel::Threads, 4));
        let place::Rows::Ready(rows) = place.thread_rows() else { panic!("listed") };
        assert_eq!((rows[0].channel, rows[0].author.as_deref()), ("general", Some("Bo")));
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::OpenFollowed(timeline_id("t-thread"))));
        assert!(matches!(&asks(&todo)[..], [ask] if crate::harness::is(ask, crate::harness::Op::History)), "{todo:?}");
        let open = window.places()[0].discussion().expect("open in the content area");
        assert_eq!(open.from, Some(Roll::Threads));
        assert!(
            matches!(&open.root, Some(place::Root::Brief { message, preview: Some(_) }) if message.as_str() == "m2")
        );
    }
}
