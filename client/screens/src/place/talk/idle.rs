//! What the server offers, whether this device is the one that alerts, and its idleness told.

use super::*;

/// Whether the person has used the machine lately, as the host's watch says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idleness {
    /// Input since the last transition.
    Active,
    /// No input for the host's limit.
    Idle,
}

/// Which of the server's switches are on, as its greeting said and each `ServerChanged` changes it; none before the
/// greeting. Made by [`Served::read`] from the switches the edge read as off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served(std::collections::BTreeSet<Switch>);

impl Served {
    /// Every switch on but those `off` names.
    pub fn read(off: &[Switch]) -> Served {
        Served(Switch::ALL.into_iter().filter(|switch| !off.contains(switch)).collect())
    }

    /// No switch known to be on: a server not yet greeted.
    pub fn nothing() -> Served {
        Served(std::collections::BTreeSet::new())
    }

    /// Whether `switch` is on.
    pub fn on(&self, switch: Switch) -> bool {
        self.0.contains(&switch)
    }

    /// Whether `switch` is on now and was not while `was` held.
    fn newly(&self, was: &Served, switch: Switch) -> bool {
        self.on(switch) && !was.on(switch)
    }
}

/// Whether this device alerts for the account, as the server last said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceUse {
    /// This device had the account's most recent input, or every device is idle, or it is alone.
    pub alerts_here: bool,
}

impl Place {
    /// Whether this server has conversations: the side pane's "Direct messages" row shows only then.
    pub fn offers_conversations(&self) -> bool {
        self.served.on(proto::rules::Switch::Conversations)
    }

    /// Whether this server has threads: the side pane's "Threads" row shows only then.
    pub fn offers_threads(&self) -> bool {
        self.served.on(proto::rules::Switch::Threads)
    }

    /// What this server offers now.
    pub fn served(&self) -> &Served {
        &self.served
    }

    /// Whether this device alerts, as the server told it on this connection.
    pub fn device_use(&self) -> Option<DeviceUse> {
        self.device
    }

    /// The reads this server's lists take after a sign-in: the conversations and the followed threads, each only
    /// where the server offers it.
    pub(in crate::place) fn talk_reads(&self) -> Vec<Ask> {
        self.list_reads(&Served::nothing())
    }

    /// The lists a server offers now that it did not while `was` held, read as it says so while signed in.
    pub(in crate::place) fn reads_newly_offered(&self, was: &Served) -> Vec<Ask> {
        if !self.signed_in() {
            return Vec::new();
        }
        self.list_reads(was)
    }

    /// The reads of the lists the server offers that `was` did not.
    fn list_reads(&self, was: &Served) -> Vec<Ask> {
        let conversations =
            self.served.newly(was, proto::rules::Switch::Conversations).then_some(ops::list_conversations());
        let threads = self.served.newly(was, proto::rules::Switch::Threads).then_some(ops::list_followed());
        conversations.into_iter().chain(threads).collect()
    }

    /// Tells the server this device went idle or came back, once each change, while signed in; nothing otherwise.
    pub(crate) fn tell_activity(&mut self, idleness: Idleness) -> Option<Ask> {
        if !self.wants_idleness() {
            self.activity_told = None;
            return None;
        }
        // A device the server has not been told of is taken as active.
        if self.activity_told.unwrap_or(Idleness::Active) == idleness {
            return None;
        }
        self.activity_told = Some(idleness);
        Some(Ask::Activity { idle: idleness == Idleness::Idle })
    }

    /// Whether the idle watch is of use to this server: the account is signed in, so its status and which device
    /// alerts follow this device's idleness.
    pub(crate) fn wants_idleness(&self) -> bool {
        self.signed_in()
    }

    /// This device's popups switch, as the window keeps it.
    pub(crate) fn set_alerting(&mut self, popups: choices::Popups) {
        self.popups = popups;
    }

    /// The time zone days and times are said in.
    pub(crate) fn set_zone(&mut self, zone: chat::TimeZone) {
        self.zone = zone;
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{
        ADA, Clock, bo_arrives, channel_id, chatting, conversation_told, conversation_wire, discussion,
        general_timeline, member, said, tell, timeline_id, told,
    };
    use crate::place::talk::tests::{asks, talk, talkative};
    use crate::place::{Owner, TalkMessage, TimelineStanding};
    use crate::window::{Ask, Message, PersonFrom};
    use crate::{WindowMessage, place};
    use proto::chatkord_v1 as wire;
    use proto::rules::Switch;
    use std::time::Instant;

    #[test]
    fn a_switch_is_on_unless_the_greeting_says_it_is_off_and_nothing_is_on_before_it() {
        for switch in Switch::ALL {
            let read = place::Served::read(&[switch]);
            assert!(!read.on(switch), "{switch:?}");
            assert!(Switch::ALL.into_iter().filter(|other| *other != switch).all(|other| read.on(other)), "only it");
            assert!(place::Served::read(&[]).on(switch) && !place::Served::nothing().on(switch));
        }
    }

    #[test]
    fn a_server_without_conversations_is_asked_for_none() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        bo_arrives(&mut window, &clock, owls);
        window.update(Message::Person(owls, member("bo"), PersonFrom::Members), clock.at());
        assert!(window.person().is_some_and(|card| !card.may_message));
        let todo = tell(&mut window, &clock, owls, talk(TalkMessage::Message(member("bo"))));
        assert!(asks(&todo).is_empty(), "{todo:?}");
        let asked = tell(&mut window, &clock, owls, place::Message::Lobby(place::LobbyMessage::Retry));
        assert!(asks(&asked).iter().all(|ask| !(crate::harness::is(ask, crate::harness::Op::ListConversations)
            || crate::harness::is(ask, crate::harness::Op::ListFollowed))));
    }

    mod alerting {
        use super::*;
        use crate::place::{DeviceUse, NotifyMessage, Notifying};
        use proto::notify::{Level, Mute, ServerSettings};
        use std::time::Duration;

        fn level(window: &mut crate::window::Window, clock: &Clock, owls: place::PlaceId, level: Level) {
            let server = ServerSettings { level, ..ServerSettings::default() };
            let notifying = Notifying { server, ..Notifying::default() };
            tell(window, clock, owls, place::Message::Notify(NotifyMessage::Changed(notifying)));
        }

        fn popups(todo: &[Ask]) -> Vec<(String, String, String)> {
            todo.iter()
                .filter_map(|done| match done {
                    Ask::Popup { id, title, body } => Some((id.wire(), title.clone(), body.clone())),
                    _ => None,
                })
                .collect()
        }

        fn withdrawn(todo: &[Ask]) -> Vec<String> {
            todo.iter()
                .filter_map(|done| if let Ask::Withdraw { id } = done { Some(id.wire()) } else { None })
                .collect()
        }

        /// Ada away from the window (it has no focus), everything alerting.
        fn away(clock: &Clock) -> (crate::window::Window, place::PlaceId) {
            let (mut window, owls) = talkative(clock);
            level(&mut window, clock, owls, Level::All);
            window.update(WindowMessage::Shown(true), clock.at());
            (window, owls)
        }

        #[test]
        fn a_live_message_that_alerts_pops_up_once_with_the_writer_the_server_and_the_channel() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            let todo = tell(&mut window, &clock, owls, told(said(6, 6, "bo", "anyone  here?\nsecond line")));
            assert_eq!(
                popups(&todo),
                [("1".to_owned(), "Bo in Night Owls".to_owned(), "#general · anyone here? second line".to_owned())],
                "{todo:?}"
            );
            let again = tell(&mut window, &clock, owls, told(said(7, 7, "bo", "hello?")));
            let [(id, ..)] = &popups(&again)[..] else { panic!("{again:?}") };
            assert_eq!(id, "1", "a newer one replaces the older: the id is the server's and the timeline's");
            let edited = tell(&mut window, &clock, owls, told(said(6, 8, "bo", "anyone here? (edited)")));
            assert!(popups(&edited).is_empty(), "an edit is no new message");
            let own = tell(&mut window, &clock, owls, told(said(9, 9, ADA, "mine")));
            assert!(popups(&own).is_empty(), "not for what this account wrote");
        }

        #[test]
        fn an_apps_post_that_alerts_pops_up_under_the_name_it_posted_as() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            let mut posted = said(6, 6, "bo", "the projector is warming up");
            posted.author = chat::Author::App { name: "Home Assistant".into(), webhook_name: "Home Assistant".into() };
            let todo = tell(&mut window, &clock, owls, told(posted));
            assert_eq!(
                popups(&todo),
                [(
                    "1".to_owned(),
                    "Home Assistant in Night Owls".to_owned(),
                    "#general · the projector is warming up".to_owned()
                )],
                "{todo:?}"
            );
        }

        #[test]
        fn it_does_not_pop_up_in_view_from_catch_up_or_under_do_not_disturb_mute_or_popups_off() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            // Caught up: the changes since a revision are never told as live.
            let caught = chat::Message { ..said(6, 6, "bo", "missed") };
            let changes = place::Caught { changed: vec![caught], reload: false, revision: 6 };
            let missed = place::DiscussionMessage::CaughtUp { timeline: general_timeline(), changes: Ok(changes) };
            let todo = tell(&mut window, &clock, owls, discussion(missed));
            assert!(popups(&todo).is_empty(), "{todo:?}");

            let chosen =
                |choice| place::Message::Talk(TalkMessage::StatusChosen(proto::status::Chosen { choice, until: None }));
            tell(&mut window, &clock, owls, chosen(proto::status::Choice::DoNotDisturb));
            assert!(popups(&tell(&mut window, &clock, owls, told(said(7, 7, "bo", "quiet")))).is_empty());
            tell(&mut window, &clock, owls, chosen(proto::status::Choice::Online));
            window.update(WindowMessage::TogglePopups, clock.at());
            assert!(popups(&tell(&mut window, &clock, owls, told(said(8, 8, "bo", "switched off")))).is_empty());
            window.update(WindowMessage::TogglePopups, clock.at());
            assert_eq!(
                popups(&tell(&mut window, &clock, owls, told(said(9, 9, "bo", "back")))).len(),
                1,
                "and back on"
            );

            let muted = Notifying {
                server: ServerSettings { level: Level::All, mute: Some(Mute::Forever), ..ServerSettings::default() },
                ..Notifying::default()
            };
            tell(&mut window, &clock, owls, place::Message::Notify(NotifyMessage::Changed(muted)));
            assert!(popups(&tell(&mut window, &clock, owls, told(said(10, 10, "bo", "muted")))).is_empty());
            level(&mut window, &clock, owls, Level::Mentions);
            assert!(popups(&tell(&mut window, &clock, owls, told(said(11, 11, "bo", "no mention")))).is_empty());
            level(&mut window, &clock, owls, Level::All);

            window.update(WindowMessage::Focused, clock.at());
            let in_view = tell(&mut window, &clock, owls, told(said(12, 12, "bo", "read as it comes")));
            assert!(popups(&in_view).is_empty(), "the channel is shown at its newest in a focused window");
        }

        fn sounds(todo: &[Ask]) -> usize {
            todo.iter().filter(|done| matches!(done, Ask::Chime(crate::Chime::Message))).count()
        }

        #[test]
        fn a_message_that_would_alert_sounds_even_with_popups_off() {
            let mut clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(6, 6, "bo", "hello")))), 1);
            window.update(WindowMessage::TogglePopups, clock.at());
            clock.pass(Duration::from_secs(2));
            let todo = tell(&mut window, &clock, owls, told(said(7, 7, "bo", "again")));
            assert_eq!(
                (sounds(&todo), popups(&todo).len()),
                (1, 0),
                "the popups switch gates only the popup: {todo:?}"
            );
        }

        #[test]
        fn a_message_does_not_sound_in_view_for_ones_own_a_muted_server_the_switch_or_do_not_disturb() {
            let mut clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(5, 5, ADA, "mine")))), 0, "one's own");

            let chosen =
                |choice| place::Message::Talk(TalkMessage::StatusChosen(proto::status::Chosen { choice, until: None }));
            tell(&mut window, &clock, owls, chosen(proto::status::Choice::DoNotDisturb));
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(6, 6, "bo", "quiet")))), 0, "do not disturb");
            tell(&mut window, &clock, owls, chosen(proto::status::Choice::Online));

            clock.pass(Duration::from_secs(2));
            window.update(WindowMessage::ToggleMessageSounds, clock.at());
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(7, 7, "bo", "switched off")))), 0);
            window.update(WindowMessage::ToggleMessageSounds, clock.at());

            let muted = Notifying {
                server: ServerSettings { level: Level::All, mute: Some(Mute::Forever), ..ServerSettings::default() },
                ..Notifying::default()
            };
            tell(&mut window, &clock, owls, place::Message::Notify(NotifyMessage::Changed(muted)));
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(8, 8, "bo", "muted")))), 0, "muted");
            level(&mut window, &clock, owls, Level::All);

            window.update(WindowMessage::Focused, clock.at());
            let in_view = tell(&mut window, &clock, owls, told(said(9, 9, "bo", "read as it comes")));
            assert_eq!(sounds(&in_view), 0, "the channel is shown at its newest in a focused window");
        }

        #[test]
        fn under_mentions_only_a_conversation_message_sounds_and_a_plain_channel_one_does_not() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            level(&mut window, &clock, owls, Level::Mentions);
            assert_eq!(sounds(&tell(&mut window, &clock, owls, told(said(6, 6, "bo", "plain")))), 0);
            conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
            let direct = place::DiscussionMessage::Told {
                timeline: timeline_id("t-c-bo"),
                message: Box::new(said(2, 2, "bo", "psst")),
            };
            assert_eq!(
                sounds(&tell(&mut window, &clock, owls, discussion(direct))),
                1,
                "a conversation calls its reader"
            );
        }

        #[test]
        fn a_burst_of_three_messages_within_a_second_and_a_half_sounds_once() {
            let mut clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            let mut heard = 0;
            for revision in 6..9 {
                heard += sounds(&tell(&mut window, &clock, owls, told(said(revision, revision, "bo", "burst"))));
                clock.pass(Duration::from_millis(500));
            }
            assert_eq!(heard, 1, "three within 1.5 s");
            clock.pass(Duration::from_millis(500));
            let later = tell(&mut window, &clock, owls, told(said(9, 9, "bo", "later")));
            assert_eq!(sounds(&later), 1, "past the gap it sounds again");
        }

        #[test]
        fn only_the_device_in_use_pops_up() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            let elsewhere = DeviceUse { alerts_here: false };
            tell(&mut window, &clock, owls, talk(TalkMessage::DeviceInUse(elsewhere)));
            assert!(popups(&tell(&mut window, &clock, owls, told(said(6, 6, "bo", "not here")))).is_empty());
            let here = DeviceUse { alerts_here: true };
            tell(&mut window, &clock, owls, talk(TalkMessage::DeviceInUse(here)));
            assert_eq!(popups(&tell(&mut window, &clock, owls, told(said(7, 7, "bo", "here")))).len(), 1);
        }

        #[test]
        fn a_conversation_message_alerts_as_a_mention_and_a_followed_threads_reply_counts_on_its_channel() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            level(&mut window, &clock, owls, Level::Mentions);
            conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
            let direct = place::DiscussionMessage::Told {
                timeline: timeline_id("t-c-bo"),
                message: Box::new(said(2, 2, "bo", "psst")),
            };
            let todo = tell(&mut window, &clock, owls, discussion(direct));
            assert_eq!(
                popups(&todo),
                [("1".to_owned(), "Bo".to_owned(), "Night Owls · direct message: psst".to_owned())],
                "{todo:?}"
            );
            let place = &window.places()[0];
            assert_eq!((place.conversations_unread(), place.unread(0).map(|held| held.unread.alerting)), (1, Some(1)));

            let thread = timeline_id("t-thread");
            let followed = TimelineStanding { read: 1, followed: true, ..TimelineStanding::default() };
            tell(
                &mut window,
                &clock,
                owls,
                talk(TalkMessage::Standing { timeline: thread.clone(), standing: followed }),
            );
            let found = wire::FollowedThreads {
                threads: vec![wire::FollowedThread {
                    standing: Some(wire::TimelineStanding {
                        timeline_id: "t-thread".into(),
                        read: 1,
                        unread: 0,
                        mentions: 0,
                        followed: true,
                    }),
                    channel_id: "general".into(),
                    root_message_id: "m2".into(),
                    root: None,
                    last_reply_at: None,
                    replies: 0,
                }],
            };
            tell(&mut window, &clock, owls, talk(TalkMessage::Followed(Ok(found))));
            let reply =
                place::DiscussionMessage::Told { timeline: thread, message: Box::new(said(2, 2, "bo", "in thread")) };
            let todo = tell(&mut window, &clock, owls, discussion(reply));
            assert_eq!(
                popups(&todo).first().map(|(_, title, body)| (title.as_str(), body.as_str())),
                Some(("Bo in Night Owls", "Thread in #general · in thread")),
                "{todo:?}"
            );
            assert_eq!(
                window.places()[0].channel_standing(&channel_id("general")).map(|standing| standing.unread),
                Some(1),
                "a followed thread's reply marks its channel"
            );
            let crate::Rows::Ready(rows) = window.places()[0].thread_rows() else { panic!("listed") };
            let (replies, last) = (rows[0].thread.replies, rows[0].thread.last_reply_at);
            assert_eq!((replies, last), (1, Some(1_763_035_200)), "and its row in the list counts it, with its time");
        }

        #[test]
        fn a_popup_is_withdrawn_when_what_it_shows_is_read_here_or_on_another_device() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            tell(&mut window, &clock, owls, told(said(6, 6, "bo", "ping")));
            let todo = window.update(WindowMessage::Focused, clock.at());
            assert_eq!(withdrawn(&todo), ["1"], "read here: {todo:?}");

            window.update(WindowMessage::FocusLost, clock.at());
            tell(&mut window, &clock, owls, told(said(7, 7, "bo", "again")));
            let read = place::ChannelStanding { read: 7, ..place::ChannelStanding::default() };
            let todo = tell(
                &mut window,
                &clock,
                owls,
                place::Message::Lobby(place::LobbyMessage::Standing {
                    channel_id: channel_id("general"),
                    standing: read,
                }),
            );
            assert_eq!(withdrawn(&todo), ["2"], "read on another device: {todo:?}");
            let again = tell(
                &mut window,
                &clock,
                owls,
                place::Message::Lobby(place::LobbyMessage::Standing {
                    channel_id: channel_id("general"),
                    standing: read,
                }),
            );
            assert!(withdrawn(&again).is_empty(), "taken away once");

            conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
            let direct = place::DiscussionMessage::Told {
                timeline: timeline_id("t-c-bo"),
                message: Box::new(said(2, 2, "bo", "psst")),
            };
            tell(&mut window, &clock, owls, discussion(direct));
            let standing = TimelineStanding { read: 2, ..TimelineStanding::default() };
            let todo = tell(
                &mut window,
                &clock,
                owls,
                talk(TalkMessage::Standing { timeline: timeline_id("t-c-bo"), standing }),
            );
            assert_eq!(withdrawn(&todo).len(), 1, "a conversation too: {todo:?}");
        }

        #[test]
        fn a_click_shows_the_window_and_opens_the_place_it_is_for() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = away(&clock);
            conversation_told(&mut window, &clock, owls, conversation_wire("c-bo", Some("bo"), true));
            let direct = place::DiscussionMessage::Told {
                timeline: timeline_id("t-c-bo"),
                message: Box::new(said(2, 2, "bo", "psst")),
            };
            let todo = tell(&mut window, &clock, owls, discussion(direct));
            let [Ask::Popup { id, .. }] =
                todo.iter().filter(|done| matches!(done, Ask::Popup { .. })).collect::<Vec<_>>()[..]
            else {
                panic!("{todo:?}")
            };
            assert_eq!(crate::window::PopupId::from_wire(&id.wire()), Some(*id));
            let todo = window.update(WindowMessage::PopupClicked(*id), clock.at());
            assert!(todo.iter().any(|done| matches!(done, Ask::Show)), "{todo:?}");
            assert!(
                asks(&todo).iter().any(
                    |ask| matches!(ask.op(), Some(crate::harness::Body::History(read)) if read.timeline_id == timeline_id("t-c-bo").as_str())
                )
            );
            let open = window.places()[0].discussion().expect("opened");
            assert!(matches!(&open.owner, Owner::Conversation(_)));
            assert!(window.update(WindowMessage::PopupClicked(*id), clock.at()).is_empty(), "a popup is clicked once");
        }
    }

    mod badge {
        use super::*;
        use crate::window::{Badge, BadgeShown};
        use proto::notify::{Level, Mute, ServerSettings};

        fn standing(unread: u32, mentions: u32) -> place::ChannelStanding {
            place::ChannelStanding { grants: proto::rules::Grants::default(), read: 0, unread, mentions }
        }

        fn badge_with(level: Level, mute: Option<Mute>, unread: u32, mentions: u32) -> Badge {
            let clock = Clock(Instant::now());
            let (mut window, owls) = talkative(&clock);
            let server = ServerSettings { level, mute, ..ServerSettings::default() };
            let notifying = place::Notifying { server, ..place::Notifying::default() };
            tell(&mut window, &clock, owls, place::Message::Notify(place::NotifyMessage::Changed(notifying)));
            let channel = place::LobbyMessage::Standing {
                channel_id: channel_id("general"),
                standing: standing(unread, mentions),
            };
            tell(&mut window, &clock, owls, place::Message::Lobby(channel));
            window.badge()
        }

        #[test]
        fn what_alerts_is_a_number_what_does_not_is_a_dot_and_what_is_muted_is_nothing() {
            assert_eq!(badge_with(Level::All, None, 3, 1).shown, BadgeShown::Count(3));
            assert_eq!(badge_with(Level::Mentions, None, 3, 1).shown, BadgeShown::Count(1));
            assert_eq!(badge_with(Level::Mentions, None, 3, 0).shown, BadgeShown::Dot);
            assert_eq!(badge_with(Level::Nothing, None, 3, 1).shown, BadgeShown::Dot);
            assert_eq!(badge_with(Level::All, Some(Mute::Forever), 3, 1).shown, BadgeShown::Nothing);
            assert_eq!(badge_with(Level::All, None, 0, 0).shown, BadgeShown::Nothing);
        }

        #[test]
        fn the_launcher_counts_what_the_rail_counts_not_the_popups_up() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = talkative(&clock);
            let channel = place::LobbyMessage::Standing { channel_id: channel_id("general"), standing: standing(3, 3) };
            tell(&mut window, &clock, owls, place::Message::Lobby(channel));
            assert_eq!(window.place_badge(owls).shown, BadgeShown::Count(3));
            assert_eq!(window.badge().count(), Some(3), "one popup per channel is up, three mentions are unread");
            assert_eq!(badge_with(Level::Mentions, None, 3, 0).count(), None, "a dot is no number");
        }

        #[test]
        fn servers_sum_and_an_unconnected_one_keeps_its_last_count_marked() {
            let some = |alerting, quiet, stale| place::PlaceUnread { unread: place::Unread { alerting, quiet }, stale };
            assert_eq!(Badge::of([some(2, false, false), some(3, true, false)]).shown, BadgeShown::Count(5));
            assert_eq!(Badge::of([some(0, true, false), some(0, false, false)]).shown, BadgeShown::Dot);
            let stale = Badge::of([some(2, false, false), some(3, false, true)]);
            assert_eq!((stale.shown, stale.stale), (BadgeShown::Count(5), true));
            assert!(!Badge::of([some(2, false, false), some(0, false, true)]).stale, "nothing held, nothing stale");

            let clock = Clock(Instant::now());
            let (mut window, owls) = talkative(&clock);
            let notifying = place::Notifying {
                server: ServerSettings { level: Level::All, ..ServerSettings::default() },
                ..place::Notifying::default()
            };
            tell(&mut window, &clock, owls, place::Message::Notify(place::NotifyMessage::Changed(notifying)));
            let channel = place::LobbyMessage::Standing { channel_id: channel_id("general"), standing: standing(4, 0) };
            tell(&mut window, &clock, owls, place::Message::Lobby(channel));
            tell(&mut window, &clock, owls, crate::harness::ended(proto::chatkord_v1::CloseReason::ClientShutdown));
            let held = window.badge();
            assert_eq!(
                (held.shown, held.stale),
                (BadgeShown::Count(4), true),
                "the connection is made again: the last count"
            );
        }
    }

    /// Ada signing in to Night Owls alone: the window, the place and the run of the idle watch her sign-in started.
    fn watching(clock: &Clock) -> (crate::window::Window, place::PlaceId, crate::window::IdleRun) {
        use crate::harness::{accepted_as, flow, token};
        let opening = crate::door::Opening::Account(crate::harness::owls(), ADA.into());
        let (mut window, _) = crate::window::Window::launch(vec![opening], None, clock.0);
        let owls = crate::harness::id(&window, 0);
        tell(&mut window, clock, owls, flow(token()));
        let todo = tell(&mut window, clock, owls, flow(accepted_as(ADA)));
        let run = started(&todo).expect("the watch starts as the account signs in");
        (window, owls, run)
    }

    /// The connection lost and made again: what coming back asked.
    fn lost_and_back(window: &mut crate::window::Window, clock: &Clock, owls: place::PlaceId) -> (Vec<Ask>, Vec<Ask>) {
        let lost = terms::Ended { silent: true, ..terms::Ended::default() };
        let gone = tell(window, clock, owls, place::Message::Session(place::SessionMessage::Disconnected(lost)));
        tell(window, clock, owls, place::Message::Session(place::SessionMessage::Reconnect(1)));
        let back = tell(window, clock, owls, crate::harness::flow(crate::harness::accepted_as(ADA)));
        (gone, back)
    }

    #[test]
    fn a_signed_in_device_tells_every_change_of_its_idleness_once_and_the_watch_runs_only_while_signed_in() {
        use crate::window::IdleWatch;
        let clock = Clock(Instant::now());
        let (mut window, owls, run) = watching(&clock);
        let todo = window.update(WindowMessage::IdleChanged(run, place::Idleness::Idle), clock.at());
        assert!(
            todo.iter().any(|done| matches!(done, Ask::At(at, place::Ask::Activity { idle: true }) if *at == owls)),
            "alone too: {todo:?}"
        );
        let todo = window.update(WindowMessage::Tick, clock.at());
        assert!(todo.iter().all(|done| !matches!(done, Ask::At(_, place::Ask::Activity { .. }))), "told once");
        let todo = window.update(WindowMessage::IdleChanged(run, place::Idleness::Active), clock.at());
        assert_eq!(idles(&todo), Some(false), "{todo:?}");
        let (gone, back) = lost_and_back(&mut window, &clock, owls);
        assert!(gone.iter().any(|done| matches!(done, Ask::Idle(IdleWatch::Stop))), "{gone:?}");
        assert!(started(&back).is_some_and(|again| again != run), "a new watch once signed in again: {back:?}");
    }

    fn idles(todo: &[Ask]) -> Option<bool> {
        todo.iter().find_map(|done| match done {
            Ask::At(_, place::Ask::Activity { idle }) => Some(*idle),
            _ => None,
        })
    }

    fn started(todo: &[Ask]) -> Option<crate::window::IdleRun> {
        todo.iter().find_map(|done| match done {
            Ask::Idle(crate::window::IdleWatch::Start(run)) => Some(*run),
            _ => None,
        })
    }

    #[test]
    fn where_the_system_tells_no_idle_time_sixty_seconds_without_input_to_the_window_are_idle() {
        use crate::window::IDLE_AFTER;
        use std::time::Duration;
        let mut clock = Clock(Instant::now());
        let (mut window, _, run) = watching(&clock);
        window.update(WindowMessage::IdleSourceLost(run), clock.at());
        let step = Duration::from_secs(1);
        clock.pass(IDLE_AFTER - step);
        let todo = window.update(WindowMessage::Tick, clock.at());
        assert_eq!(idles(&todo), None, "a second short of the bound");
        clock.pass(step);
        let todo = window.update(WindowMessage::Tick, clock.at());
        assert_eq!(idles(&todo), Some(true));
        let todo = window.update(WindowMessage::Input, clock.at());
        assert_eq!(idles(&todo), Some(false), "the next input is active again");
    }

    #[test]
    fn input_within_the_bound_keeps_the_person_active_and_a_wake_is_asked_for_the_bound() {
        use crate::window::IDLE_AFTER;
        use std::time::Duration;
        let mut clock = Clock(Instant::now());
        let (mut window, _, run) = watching(&clock);
        let todo = window.update(WindowMessage::IdleSourceLost(run), clock.at());
        let bound = clock.at().now + IDLE_AFTER;
        assert!(todo.iter().any(|done| matches!(done, Ask::WakeAt(at) if *at == bound)), "{todo:?}");
        for _ in 0..3 {
            clock.pass(Duration::from_secs(59));
            window.update(WindowMessage::Input, clock.at());
            let todo = window.update(WindowMessage::Tick, clock.at());
            assert_eq!(idles(&todo), None, "{todo:?}");
        }
    }

    #[test]
    fn a_restarted_watch_is_the_system_s_again_and_tells_nothing_by_the_window() {
        use crate::window::IDLE_AFTER;
        let mut clock = Clock(Instant::now());
        let (mut window, owls, first) = watching(&clock);
        window.update(WindowMessage::IdleSourceLost(first), clock.at());
        let (_, back) = lost_and_back(&mut window, &clock, owls);
        let again = started(&back).expect("the watch starts again");
        assert_ne!(first, again);
        clock.pass(IDLE_AFTER * 2);
        let todo = window.update(WindowMessage::Tick, clock.at());
        assert_eq!(idles(&todo), None, "the new watch is the system's, and has told nothing");
    }

    #[test]
    fn what_a_stopped_watch_still_tells_is_not_taken_for_the_new_one_s() {
        use crate::window::IDLE_AFTER;
        let mut clock = Clock(Instant::now());
        let (mut window, owls, first) = watching(&clock);
        lost_and_back(&mut window, &clock, owls);
        window.update(WindowMessage::IdleSourceLost(first), clock.at());
        let todo = window.update(WindowMessage::IdleChanged(first, place::Idleness::Idle), clock.at());
        assert_eq!(idles(&todo), None, "{todo:?}");
        clock.pass(IDLE_AFTER * 2);
        let todo = window.update(WindowMessage::Tick, clock.at());
        assert_eq!(idles(&todo), None, "the old run's loss left the new watch the system's");
    }

    #[test]
    fn the_popups_switch_is_kept() {
        use crate::call::sound::Keep;
        use choices::Popups;
        let clock = Clock(Instant::now());
        let (mut window, _owls) = chatting(&clock);
        let todo = window.update(WindowMessage::TogglePopups, clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Popups(Popups::Off))]), "{todo:?}");
    }
}
