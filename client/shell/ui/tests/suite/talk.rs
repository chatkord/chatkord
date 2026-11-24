//! Direct messages, threads and desktop notifications as the canvas draws them: the side pane's two rows, the lists
//! in the content area with their empty states, a conversation (also one whose other member is gone), a message's
//! thread chip and menu, the thread beside the channel (wide and narrow), the person popover's Message, the
//! Notifications page and the rail's badge; each a reference in both schemes where it is a picture, each control
//! sending what it is for. The references are recorded with `UPDATE_REFS=1` and reviewed as a PNG diff.

use crate::common;

use common::{boxes, semantics, settle, sim_sized};
use design::{Accent, Scheme, SemanticRole};
use iced_core::Size;
use proto::chatkord_v1 as wire;
use proto::status::Choice;
use screens::harness::{self, Clock, every_switch_on, said};
use screens::{Ask, PersonFrom, PlaceId, PlaceMessage, Roll, StatusMessage, TalkMessage, Window, WindowMessage, chat};
use std::time::Instant;
use terms::{ConversationId, TimelineId};
use ui::testing::{self, Image, Renderer};
use ui::{Theme, app};

/// The canvas's window.
const WINDOW: Size = Size::new(1280.0, 768.0);
/// The canvas's narrow window, which a thread covers the channel in.
const NARROW: Size = Size::new(669.0, 568.0);

fn clock() -> Clock {
    Clock(Instant::now())
}

fn talk(message: TalkMessage) -> PlaceMessage {
    PlaceMessage::Talk(message)
}

fn tell(window: &mut Window, id: PlaceId, message: PlaceMessage) -> Vec<Ask> {
    harness::tell(window, &clock(), id, message)
}

/// Ada in #general at a server that offers it all, its people listed; Bo's message there starts a thread.
fn talkative() -> (Window, PlaceId) {
    let (mut window, owls) = harness::chatting(&clock());
    every_switch_on(&mut window, &clock(), owls);
    harness::people_listed(&mut window, &clock(), owls);
    harness::roles_listed(&mut window, &clock(), owls);
    let mut root = said(2, 6, "bo", "anyone up for a game later?");
    root.thread = Some(chat::ThreadSummary {
        timeline: harness::timeline_id("t-thread"),
        replies: 3,
        last_reply_at: Some(root.created_at + 600),
    });
    tell(&mut window, owls, harness::told(root));
    (window, owls)
}

fn conversation(id: &str, with: Option<&str>, sendable: bool, unread: u32, text: &str) -> wire::Conversation {
    let mut told = harness::conversation_wire(id, with, sendable);
    told.last = Some(wire::Preview { user_id: with.map(str::to_owned), text: text.into(), created_at: 1_763_035_200 });
    told.standing = Some(wire::TimelineStanding {
        timeline_id: told.timeline_id.clone(),
        read: 1,
        unread,
        mentions: unread,
        followed: false,
    });
    told
}

/// Bo's conversation unread, and Cy's, whose other member is gone.
fn with_conversations() -> (Window, PlaceId) {
    let (mut window, owls) = talkative();
    harness::conversation_told(
        &mut window,
        &clock(),
        owls,
        conversation("c-bo", Some("bo"), true, 2, "see you at the table"),
    );
    harness::conversation_told(&mut window, &clock(), owls, conversation("c-cy", None, false, 0, "any time"));
    (window, owls)
}

/// The thread on bo's message open beside #general.
fn with_thread() -> (Window, PlaceId) {
    let (mut window, owls) = talkative();
    start_thread(&mut window, owls);
    (window, owls)
}

/// Opens the thread on bo's message, as "Reply in thread" does where the message has one.
fn start_thread(window: &mut Window, owls: PlaceId) {
    tell(window, owls, talk(TalkMessage::StartThread(harness::message_id(2))));
}

/// The window's frames over one kept widget tree, as its loop runs them: each said message is handled before the
/// next frame, which the pointer, resting at `cursor`, sees.
struct Frames {
    kept: Option<testing::Cache>,
    cursor: iced_core::Point,
}

impl Frames {
    fn new() -> Self {
        Frames { kept: None, cursor: iced_core::Point::new(640.0, 300.0) }
    }

    fn run(&mut self, window: &mut Window, events: &[iced_core::Event]) -> Vec<WindowMessage> {
        let view = app::view::<Renderer>(window, WINDOW);
        let said = testing::interact_kept(&mut self.kept, view, WINDOW, self.cursor, events);
        for message in &said {
            window.update(message.clone(), clock().at());
        }
        said
    }
}

/// The drafts the channel's composer, and the thread's, reported.
fn drafts(said: &[WindowMessage]) -> (Vec<String>, Vec<String>) {
    let (mut channel, mut thread) = (Vec::new(), Vec::new());
    for message in said {
        match message {
            WindowMessage::At(_, PlaceMessage::Discussion(screens::DiscussionMessage::Draft(text))) => {
                channel.push(text.clone());
            }
            WindowMessage::At(_, PlaceMessage::Thread(screens::DiscussionMessage::Draft(text))) => {
                thread.push(text.clone());
            }
            _ => {}
        }
    }
    (channel, thread)
}

fn render(theme: &Theme, window: &Window, size: Size) -> Image {
    testing::render::<WindowMessage>(theme, app::view::<Renderer>(window, size), size, 1.0)
}

fn every_scheme(name: &str, window: &Window, size: Size) {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let image = render(&Theme::new(scheme, Accent::DEFAULT), window, size);
        ui::refs!().assert(&format!("{name}-{scheme:?}").to_lowercase(), &image);
    }
}

/// What a press on the control a screen reader names `name` sends.
fn pressing(window: &Window, size: Size, role: SemanticRole, name: &str) -> Vec<WindowMessage> {
    let mut sim = sim_sized(app::view::<Renderer>(window, size), size);
    settle(&mut sim);
    let at = boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| each.role == role && each.label.as_str() == name)
        .map(|(_, bounds)| bounds.center())
        .unwrap_or_else(|| panic!("no {name}"));
    sim.point_at(at);
    sim.simulate(testing::click());
    sim.drain().collect()
}

fn names(window: &Window, size: Size) -> Vec<(SemanticRole, String)> {
    let mut sim = sim_sized(app::view::<Renderer>(window, size), size);
    semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect()
}

fn has(window: &Window, size: Size, role: SemanticRole, name: &str) -> bool {
    names(window, size).iter().any(|(each, said)| *each == role && said == name)
}

fn sent(messages: &[WindowMessage], wanted: &TalkMessage) -> bool {
    messages.iter().any(|message| {
        matches!(message, WindowMessage::At(_, PlaceMessage::Talk(done)) if format!("{done:?}") == format!("{wanted:?}"))
    })
}

#[test]
fn the_side_pane_offers_threads_and_direct_messages_and_each_opens_its_list() {
    let (window, _) = talkative();
    assert!(has(&window, WINDOW, SemanticRole::ListItem, "Threads"));
    let rows = names(&window, WINDOW);
    let at = |name: &str| rows.iter().position(|(role, said)| *role == SemanticRole::ListItem && said == name);
    assert!(at("Direct messages") < at("Threads"), "direct messages first: {rows:?}");
    let pressed = pressing(&window, WINDOW, SemanticRole::ListItem, "Direct messages");
    assert!(sent(&pressed, &TalkMessage::Show(Roll::Conversations)), "{pressed:?}");
    let pressed = pressing(&window, WINDOW, SemanticRole::ListItem, "Threads");
    assert!(sent(&pressed, &TalkMessage::Show(Roll::Threads)), "{pressed:?}");
}

#[test]
fn a_server_without_conversations_has_no_row_for_them() {
    let (window, _) = harness::chatting(&clock());
    assert!(!has(&window, WINDOW, SemanticRole::ListItem, "Direct messages"));
    assert!(!has(&window, WINDOW, SemanticRole::ListItem, "Threads"));
}

#[test]
fn the_conversations_list_is_rows_that_open_their_conversation() {
    let (mut window, owls) = with_conversations();
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Conversations)));
    let rows = names(&window, WINDOW);
    assert!(rows.iter().any(|(role, said)| *role == SemanticRole::ListItem && said.starts_with("Bo")), "{rows:?}");
    let row = rows.iter().find(|(_, said)| said.starts_with("Bo")).map(|(_, said)| said.clone()).unwrap();
    let pressed = pressing(&window, WINDOW, SemanticRole::ListItem, &row);
    let id = ConversationId::parse("c-bo").unwrap();
    assert!(sent(&pressed, &TalkMessage::OpenConversation(id)), "{pressed:?}");
    every_scheme("talk-dm-list", &window, WINDOW);
}

#[test]
fn a_list_with_no_rows_says_so() {
    let (mut window, owls) = talkative();
    tell(&mut window, owls, talk(TalkMessage::Conversations(Ok(wire::Conversations::default()))));
    tell(&mut window, owls, talk(TalkMessage::Followed(Ok(wire::FollowedThreads::default()))));
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Conversations)));
    every_scheme("talk-dm-list-empty", &window, WINDOW);
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Threads)));
    every_scheme("talk-thread-list-empty", &window, WINDOW);
}

#[test]
fn a_conversation_has_a_back_arrow_and_no_pins_and_the_bars_members_toggle() {
    let (mut window, owls) = with_conversations();
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Conversations)));
    tell(&mut window, owls, talk(TalkMessage::OpenConversation(ConversationId::parse("c-bo").unwrap())));
    assert!(!has(&window, WINDOW, SemanticRole::Button, "Pinned messages"));
    assert!(has(&window, WINDOW, SemanticRole::Button, "Hide members"), "the top bar's, the column shown");
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, "Back");
    assert!(sent(&pressed, &TalkMessage::Back), "{pressed:?}");
    every_scheme("talk-dm-open", &window, WINDOW);
}

/// A `#` typed in a conversation offers the channels the writer sees, by their categories.
#[test]
fn a_conversation_offers_the_channels_the_writer_sees() {
    let (mut window, owls) = with_conversations();
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Conversations)));
    tell(&mut window, owls, talk(TalkMessage::OpenConversation(ConversationId::parse("c-bo").unwrap())));
    let draft = |window: &mut Window| {
        let typed = screens::DiscussionMessage::Draft("the rules are in #".into());
        tell(window, owls, PlaceMessage::Discussion(typed));
    };
    let generals = |window: &Window| {
        names(window, WINDOW).iter().filter(|(role, said)| *role == SemanticRole::ListItem && said == "general").count()
    };
    let before = generals(&window);
    draft(&mut window);
    assert_eq!(generals(&window), before + 1, "the chooser offers it: {:?}", names(&window, WINDOW));
    every_scheme("talk-dm-channels", &window, WINDOW);
}

#[test]
fn a_conversation_whose_other_member_is_gone_has_no_composer_and_says_so() {
    let (mut window, owls) = with_conversations();
    tell(&mut window, owls, talk(TalkMessage::Show(Roll::Conversations)));
    tell(&mut window, owls, talk(TalkMessage::OpenConversation(ConversationId::parse("c-cy").unwrap())));
    every_scheme("talk-dm-gone", &window, WINDOW);
}

#[test]
fn a_message_with_a_thread_shows_its_chip_which_opens_it_and_its_menu_offers_a_reply_in_thread() {
    let (mut window, owls) = talkative();
    let chip = names(&window, WINDOW)
        .into_iter()
        .find(|(role, said)| *role == SemanticRole::Button && said.starts_with("3 replies"))
        .map(|(_, said)| said)
        .expect("the thread's chip");
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, &chip);
    assert!(sent(&pressed, &TalkMessage::OpenThread(harness::message_id(2))), "{pressed:?}");
    tell(&mut window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Menu(harness::message_id(2))));
    assert!(names(&window, WINDOW).iter().any(|(_, said)| said == "Reply in thread"));
    let pressed = pressing(&window, WINDOW, SemanticRole::MenuItem, "Reply in thread");
    assert!(sent(&pressed, &TalkMessage::StartThread(harness::message_id(2))), "{pressed:?}");
}

#[test]
fn a_message_whose_thread_has_no_replies_shows_no_chip() {
    let (mut window, owls) = talkative();
    let mut root = said(2, 9, "bo", "anyone up for a game later?");
    root.thread =
        Some(chat::ThreadSummary { timeline: harness::timeline_id("t-thread"), replies: 0, last_reply_at: None });
    tell(&mut window, owls, harness::told(root));
    let chips: Vec<String> = names(&window, WINDOW)
        .into_iter()
        .filter(|(role, said)| *role == SemanticRole::Button && said.contains(" repl"))
        .map(|(_, said)| said)
        .collect();
    assert!(chips.is_empty(), "nothing to open, so nothing to press: {chips:?}");
}

#[test]
fn one_composer_holds_the_keyboard_as_a_thread_opens_beside_the_channel_and_a_reply_begins_there() {
    let (mut window, owls) = talkative();
    let mut frames = Frames::new();
    frames.run(&mut window, &[]);
    assert_eq!(drafts(&frames.run(&mut window, &testing::typing("a"))), (vec!["a".into()], vec![]));
    start_thread(&mut window, owls);
    frames.run(&mut window, &[]);
    let typed = drafts(&frames.run(&mut window, &testing::typing("b")));
    assert_eq!(typed, (vec![], vec!["b".into()]), "the thread opened took the keyboard, and the channel's let it go");
    tell(&mut window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Reply(harness::message_id(2))));
    frames.run(&mut window, &[]);
    let typed = drafts(&frames.run(&mut window, &testing::typing("c")));
    assert_eq!(typed, (vec!["ac".into()], vec![]), "the reply begun took it back, and the thread's let it go");
}

#[test]
fn a_space_typed_in_a_thread_begun_from_the_menu_reaches_the_thread_and_not_the_menu_left_behind() {
    let (mut window, owls) = talkative();
    let mut frames = Frames::new();
    while !frames.run(&mut window, &[]).is_empty() {}
    tell(&mut window, owls, PlaceMessage::Discussion(screens::DiscussionMessage::Menu(harness::message_id(2))));
    frames.run(&mut window, &[]);
    let item = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        settle(&mut sim);
        boxes(&mut sim)
            .into_iter()
            .find(|(each, _)| each.role == SemanticRole::MenuItem && each.label.as_str() == "Reply in thread")
            .map(|(_, bounds)| bounds.center())
            .expect("the menu's Reply in thread")
    };
    frames.cursor = item;
    let pressed = frames.run(&mut window, &testing::click());
    assert!(sent(&pressed, &TalkMessage::StartThread(harness::message_id(2))), "{pressed:?}");
    frames.cursor = iced_core::Point::new(640.0, 300.0);
    frames.run(&mut window, &[]);
    let said = frames.run(&mut window, &testing::typing("why x"));
    let replied = said.iter().any(|message| {
        matches!(message, WindowMessage::At(_, PlaceMessage::Discussion(screens::DiscussionMessage::Reply(_))))
    });
    assert!(!replied, "the space pressed the menu's Reply: {said:?}");
    let thread = ["w", "wh", "why", "why ", "why x"].map(String::from).to_vec();
    assert_eq!(drafts(&said), (vec![], thread));
}

#[test]
fn a_thread_open_beside_the_channel_has_its_bell_and_its_close_and_a_composer_of_its_own() {
    let (window, _) = with_thread();
    assert!(has(&window, WINDOW, SemanticRole::Button, "Close"));
    let timeline = TimelineId::parse("t-thread").unwrap();
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, "Follow thread");
    assert!(
        sent(&pressed, &TalkMessage::Follow { timeline, follow: true }),
        "one who opens a thread follows it only once they reply or ask: {pressed:?}"
    );
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, "Close");
    assert!(sent(&pressed, &TalkMessage::CloseThread), "{pressed:?}");
    every_scheme("talk-thread-open", &window, WINDOW);
}

#[test]
fn a_thread_covers_the_channel_where_there_is_no_room_for_both_and_its_back_arrow_closes_it() {
    let (window, _) = with_thread();
    assert!(!has(&window, NARROW, SemanticRole::Button, "Close"), "a back arrow stands where the close does");
    let pressed = pressing(&window, NARROW, SemanticRole::Button, "Back");
    assert!(sent(&pressed, &TalkMessage::CloseThread), "{pressed:?}");
    every_scheme("talk-thread-narrow", &window, NARROW);
}

#[test]
fn the_members_column_goes_while_a_thread_leaves_the_channel_less_than_its_least() {
    let (window, _) = with_thread();
    let wide = Size::new(1480.0, 768.0);
    assert!(has(&window, wide, SemanticRole::List, "Members"), "room for the channel, the thread and the members");
    assert!(!has(&window, WINDOW, SemanticRole::List, "Members"), "{:?}", names(&window, WINDOW));
}

#[test]
fn the_popover_of_a_member_offers_message_and_no_volume() {
    let (mut window, owls) = talkative();
    window.update(WindowMessage::Person(owls, harness::member("bo"), PersonFrom::Members), clock().at());
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, "Message");
    assert!(sent(&pressed, &TalkMessage::Message(harness::member("bo"))), "{pressed:?}");
    assert!(!names(&window, WINDOW).iter().any(|(_, said)| said == "Volume"));
    every_scheme("talk-dm-person", &window, WINDOW);
}

/// A server of thousands: the members column builds the rows round its viewport, and so asks the host for those rows'
/// pictures alone.
#[test]
fn the_members_column_of_thousands_asks_only_for_the_pictures_of_the_rows_it_builds() {
    use proto::files::FileId;
    const MEMBERS: u64 = 5_000;
    let picture = |at: u64| {
        let mut bytes = [0u8; FileId::LEN];
        bytes[..8].copy_from_slice(&(at + 1).to_le_bytes());
        FileId::from_bytes(bytes)
    };
    let (mut window, owls) = harness::chatting(&clock());
    every_switch_on(&mut window, &clock(), owls);
    window.set_room(screens::frame::Room { width: WINDOW.width, height: WINDOW.height });
    let entries = (0..MEMBERS)
        .map(|at| {
            let id = format!("m{at:04}");
            let named =
                screens::Named { name: id.clone(), colour: None, roles: Vec::new(), picture: Some(picture(at)) };
            (harness::member(&id), named, proto::status::Shown::Online)
        })
        .collect();
    let page = screens::PeoplePage { entries, next: None, presence: 1 };
    tell(&mut window, owls, PlaceMessage::People(screens::PeopleMessage::Page(Ok(page))));
    let asked = std::cell::RefCell::new(Vec::new());
    let handles = |file| {
        asked.borrow_mut().push(file);
        None
    };
    drop(app::view_with::<Renderer>(&window, WINDOW, &handles, &|_| {
        iced_core::Widget::_boxed(iced_widget::Space::new())
    }));
    let asked = asked.into_inner();
    let rows: Vec<u64> = (0..MEMBERS).filter(|at| asked.contains(&picture(*at))).collect();
    assert!(rows.contains(&0), "the first row's: {rows:?}");
    assert!(rows.len() < 100 && !rows.contains(&(MEMBERS - 1)), "{} of {MEMBERS} asked: {rows:?}", rows.len());
}

/// Opened from the members column with no control focused, a member's popover stands beside their row under the top
/// bar, not where the row would be without it.
#[test]
fn the_popover_of_a_member_stands_beside_their_row_under_the_bar() {
    let (mut window, owls) = talkative();
    window.set_room(screens::frame::Room { width: WINDOW.width, height: WINDOW.height });
    let found = |window: &Window, said: &str| {
        let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
        let all = boxes(&mut sim);
        all.into_iter().filter(|(each, _)| each.label.as_str() == said).map(|(_, bounds)| bounds).collect::<Vec<_>>()
    };
    let row = found(&window, "Bo").into_iter().max_by(|a, b| a.x.total_cmp(&b.x)).expect("Bo's row in the column");
    window.update(WindowMessage::Person(owls, harness::member("bo"), PersonFrom::Members), clock().at());
    let message = found(&window, "Message").pop().expect("the popover's Message");
    let named = found(&window, "Bo").into_iter().filter(|each| (each.x - message.x).abs() < message.width);
    let heading = named.min_by(|a, b| a.y.total_cmp(&b.y)).expect("the popover's name");
    assert!((heading.y - row.y).abs() < row.height, "the popover's name {heading:?} by the row {row:?}");
}

/// The account bar's name and picture open one's own popover, a margin above the bar and level with its button, which
/// offers no Message and no volume.
#[test]
fn the_account_bar_opens_ones_own_popover_above_it_with_no_message_and_no_volume() {
    let (mut window, owls) = talkative();
    window.set_room(screens::frame::Room { width: WINDOW.width, height: WINDOW.height });
    let (me, name) = {
        let place = window.open().expect("a server open");
        (place.me().cloned().expect("signed in"), place.shown().expect("a name").to_string())
    };
    let mut pressed =
        pressing(&window, WINDOW, SemanticRole::Opens { layer: design::Popup::Dialog, open: false }, &name);
    // The discussion measures itself on its first layout; what the press asked is the rest.
    pressed.retain(|said| matches!(said, WindowMessage::Person(..)));
    assert!(
        matches!(&pressed[..], [WindowMessage::Person(_, who, PersonFrom::AccountBar)] if *who == me),
        "the bar opens one's own: {pressed:?}"
    );
    window.update(WindowMessage::Person(owls, me, PersonFrom::AccountBar), clock().at());
    assert!(!names(&window, WINDOW).iter().any(|(_, said)| said == "Message" || said == "Volume"));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let all = boxes(&mut sim);
    let bar = all
        .iter()
        .find(|(each, _)| matches!(each.role, SemanticRole::Opens { .. }) && each.label.as_str() == name)
        .map(|(_, bounds)| *bounds)
        .expect("the account bar's button");
    let popover = all
        .iter()
        .find(|(each, _)| each.role == SemanticRole::Dialog && each.label.as_str() == name)
        .map(|(_, bounds)| *bounds)
        .expect("the popover");
    let margin = design::Space::Xs.px() + design::Space::Xxs.px();
    // The button is a control's height in the middle of the strip; the margin is the strip's.
    let strip = bar.y - (design::ACCOUNT_STRIP_HEIGHT - design::CONTROL_HEIGHT) / 2.0;
    assert!(
        (strip - (popover.y + popover.height) - margin).abs() < 1.0,
        "a margin above the strip: {popover:?} {bar:?}"
    );
    assert!((popover.x - bar.x).abs() < 1.0, "level with the bar's button: {popover:?} {bar:?}");
    every_scheme("talk-account-person", &window, WINDOW);
}

#[test]
fn the_volume_popup_of_a_call_card_is_the_name_and_the_volume_alone() {
    let (mut window, owls) =
        harness::moderator_in_call(&clock(), harness::ADMIN_PLACE, proto::rules::Grants::default());
    every_switch_on(&mut window, &clock(), owls);
    window.update(WindowMessage::Person(owls, harness::member("bo"), PersonFrom::Card), clock().at());
    let found = names(&window, WINDOW);
    assert!(found.iter().any(|(_, said)| said == "Volume"), "{found:?}");
    assert!(!found.iter().any(|(_, said)| said == "Message"), "{found:?}");
}

#[test]
fn notifications_page_switches_popups_points_to_the_status_and_keeps_the_sounds() {
    let (mut window, owls) = with_status();
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Notifications), clock().at());
    let pressed = pressing(&window, WINDOW, SemanticRole::Switch { on: true }, "Show desktop notifications");
    assert!(matches!(&pressed[..], [WindowMessage::TogglePopups]), "{pressed:?}");
    let pressed = pressing(&window, WINDOW, SemanticRole::Button, "Choose a status");
    assert!(matches!(&pressed[..], [WindowMessage::Status(StatusMessage::Choose)]), "{pressed:?}");
    every_scheme("talk-notifications", &window, WINDOW);
    chosen(&mut window, owls, Choice::DoNotDisturb, Some(harness::WALL + 3_600));
    let note = "On until 13:00: no popups or sounds on any of your devices.";
    let said = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW).find(note).is_ok();
    assert!(said, "the status's end is said");
    window.update(WindowMessage::Status(StatusMessage::Choose), clock().at());
    let card = window.person().expect("the account's popover opens");
    assert_eq!(card.open.from, PersonFrom::AccountBar);
}

/// Ada signed in, the day known.
fn with_status() -> (Window, PlaceId) {
    let (mut window, owls) = talkative();
    let today = chat::local(harness::WALL, &chat::TimeZone::UTC).map(|at| at.date()).expect("a day");
    window.update(WindowMessage::Today(today), clock().at());
    window.set_room(screens::frame::Room { width: WINDOW.width, height: WINDOW.height });
    (window, owls)
}

/// The server telling the account it chose `choice` until `until`.
fn chosen(window: &mut Window, owls: PlaceId, choice: Choice, until: Option<i64>) {
    tell(window, owls, talk(TalkMessage::StatusChosen(proto::status::Chosen { choice, until })));
}

/// The account's own popover, opened from its bar.
fn own_popover(window: &mut Window, owls: PlaceId) {
    let me = window.open().and_then(|place| place.me().cloned()).expect("signed in");
    window.update(WindowMessage::Person(owls, me, PersonFrom::AccountBar), clock().at());
}

#[test]
fn the_account_popover_lists_the_statuses_and_a_pick_opens_its_durations() {
    let (mut window, owls) = with_status();
    own_popover(&mut window, owls);
    let found = names(&window, WINDOW);
    let radio = |chosen: bool, name: &str| (SemanticRole::MenuItemRadio { chosen }, name.to_string());
    for wanted in
        [radio(true, "Online"), radio(false, "Away"), radio(false, "Do not disturb"), radio(false, "Invisible")]
    {
        assert!(found.contains(&wanted), "{wanted:?} in {found:?}");
    }
    every_scheme("talk-status-menu", &window, WINDOW);
    let pressed = pressing(&window, WINDOW, SemanticRole::MenuItemRadio { chosen: false }, "Do not disturb");
    let picked =
        |message: &WindowMessage| matches!(message, WindowMessage::Status(StatusMessage::Pick(Choice::DoNotDisturb)));
    assert!(pressed.iter().any(picked), "{pressed:?}");
    window.update(WindowMessage::Status(StatusMessage::Pick(Choice::DoNotDisturb)), clock().at());
    every_scheme("talk-status-pick", &window, WINDOW);
    let pressed = pressing(&window, WINDOW, SemanticRole::MenuItem, "Until tomorrow");
    let tomorrow = |message: &WindowMessage| {
        matches!(message, WindowMessage::Status(StatusMessage::For(screens::StatusFor::Tomorrow)))
    };
    assert!(pressed.iter().any(tomorrow), "{pressed:?}");
}

#[test]
fn the_account_bar_says_the_status_with_its_dot_and_the_menu_marks_its_end() {
    let (mut window, owls) = with_status();
    chosen(&mut window, owls, Choice::DoNotDisturb, Some(harness::WALL + 86_400));
    let says = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW).find("Do not disturb").is_ok();
    assert!(says, "the account bar says it");
    every_scheme("talk-status-dnd", &window, WINDOW);
    own_popover(&mut window, owls);
    let found = names(&window, WINDOW);
    let marked = (SemanticRole::MenuItemRadio { chosen: true }, "Do not disturb Until tomorrow 12:00".to_string());
    assert!(found.contains(&marked), "the chosen one with its end: {found:?}");
    chosen(&mut window, owls, Choice::Invisible, None);
    // The first press closes the popover open, the second opens it on the new status.
    own_popover(&mut window, owls);
    own_popover(&mut window, owls);
    let found = names(&window, WINDOW);
    assert!(found.contains(&(SemanticRole::MenuItemRadio { chosen: true }, "Invisible".to_string())), "{found:?}");
    every_scheme("talk-status-invisible", &window, WINDOW);
}

#[test]
fn the_members_column_marks_each_status_by_its_dot() {
    let (mut window, owls) = with_status();
    let shown = vec![
        (harness::member("grace"), proto::status::Shown::Away),
        (harness::member("bo"), proto::status::Shown::DoNotDisturb),
    ];
    let told = screens::PeopleMessage::Presence { generation: 2, shown };
    tell(&mut window, owls, PlaceMessage::People(told));
    let grace = window.open().map(|place| place.directory().presence(&harness::member("grace")));
    assert_eq!(grace, Some(proto::status::Shown::Away), "the status reached the directory");
    every_scheme("talk-members-status", &window, WINDOW);
}

#[test]
fn the_rail_marks_a_server_with_what_is_unread_on_it() {
    let (mut window, owls) = talkative();
    window.update(WindowMessage::Open(window.places()[0].id()), clock().at());
    harness::conversation_told(&mut window, &clock(), owls, conversation("c-bo", Some("bo"), true, 3, "hi"));
    tell(&mut window, owls, harness::told(said(5, 12, "bo", "hello")));
    every_scheme("talk-rail-badge", &window, WINDOW);
}
