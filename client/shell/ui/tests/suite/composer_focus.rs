//! The composer keeps the keyboard while a chooser opens over it and closes again: the window builds its view anew
//! every frame, and a field moved in the tree is a new field, its focus gone.

use iced_core::{Point, Size};
use screens::harness::{self, Clock};
use screens::{DiscussionMessage, PlaceMessage, WindowMessage};
use std::time::Instant;
use ui::app;
use ui::testing::{self, Cache};

const WINDOW: Size = Size::new(1280.0, 800.0);
/// Inside the composer, at the foot of the channel.
const COMPOSER: Point = Point::new(640.0, 765.0);

#[test]
fn letters_typed_after_a_chooser_opens_still_reach_the_field() {
    let clock = Clock(Instant::now());
    let (mut window, _) = harness::chatting(&clock);
    let mut kept: Option<Cache> = None;
    let mut frame = |window: &mut screens::Window, events: &[iced_core::Event]| {
        let said = testing::interact_kept(&mut kept, app::view(window, WINDOW), WINDOW, COMPOSER, events);
        for message in &said {
            window.update(message.clone(), clock.at());
        }
        said
    };
    frame(&mut window, &testing::click());
    let drafts = |said: &[WindowMessage]| -> Vec<String> {
        said.iter()
            .filter_map(|message| match message {
                WindowMessage::At(_, PlaceMessage::Discussion(DiscussionMessage::Draft(text))) => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(drafts(&frame(&mut window, &testing::typing("#"))), ["#"], "the composer has the keyboard");
    let open = window.places()[0].choosing(&harness::general_compose()).is_some();
    assert!(open, "the # opened the chooser");
    assert_eq!(drafts(&frame(&mut window, &testing::typing("g"))), ["#g"], "and keeps it once the chooser is open");
}

#[test]
fn a_reply_bar_or_a_strip_appearing_above_the_field_leaves_it_the_keyboard() {
    let clock = Clock(Instant::now());
    let (mut window, owls) = harness::chatting(&clock);
    let mut kept: Option<Cache> = None;
    let mut frame = |window: &mut screens::Window, events: &[iced_core::Event]| {
        let said = testing::interact_kept(&mut kept, app::view(window, WINDOW), WINDOW, COMPOSER, events);
        for message in &said {
            window.update(message.clone(), clock.at());
        }
        said
    };
    frame(&mut window, &testing::click());
    frame(&mut window, &testing::typing("a"));
    let hello = window.places()[0]
        .discussion()
        .and_then(|open| open.timeline())
        .and_then(|held| held.last())
        .expect("a message")
        .id
        .clone();
    window.update(WindowMessage::At(owls, PlaceMessage::Discussion(DiscussionMessage::Reply(hello))), clock.at());
    assert!(
        window.places()[0].composing(&harness::general_compose()).is_some_and(|composing| composing.reply.is_some())
    );
    let said = frame(&mut window, &testing::typing("b"));
    let typed = said.iter().any(|message| {
        matches!(message, WindowMessage::At(_, PlaceMessage::Discussion(DiscussionMessage::Draft(text))) if text == "ab")
    });
    assert!(typed, "the field kept the keyboard once the reply bar showed: {said:?}");
}

#[test]
fn reply_gives_the_composer_the_keyboard() {
    let clock = Clock(Instant::now());
    let (mut window, owls) = harness::chatting(&clock);
    let mut kept: Option<Cache> = None;
    let away = Point::new(640.0, 300.0);
    let mut frame = |window: &mut screens::Window, events: &[iced_core::Event]| {
        let said = testing::interact_kept(&mut kept, app::view(window, WINDOW), WINDOW, away, events);
        for message in &said {
            window.update(message.clone(), clock.at());
        }
        said
    };
    frame(&mut window, &[]);
    let hello = window.places()[0]
        .discussion()
        .and_then(|open| open.timeline())
        .and_then(|held| held.last())
        .expect("a message")
        .id
        .clone();
    window.update(WindowMessage::At(owls, PlaceMessage::Discussion(DiscussionMessage::Reply(hello))), clock.at());
    frame(&mut window, &[]);
    let said = frame(&mut window, &testing::typing("b"));
    let typed = said.iter().any(|message| {
        matches!(message, WindowMessage::At(_, PlaceMessage::Discussion(DiscussionMessage::Draft(text))) if text == "b")
    });
    assert!(typed, "typing after Reply reaches the composer: {said:?}");
}

#[test]
fn a_file_dropped_on_the_window_gives_the_composer_the_keyboard_so_enter_sends() {
    let clock = Clock(Instant::now());
    let (mut window, owls) = harness::chatting(&clock);
    harness::standing(
        &mut window,
        &clock,
        owls,
        &[proto::rules::Grant::ChannelRead, proto::rules::Grant::SendMessage, proto::rules::Grant::AttachFiles],
    );
    let mut kept: Option<Cache> = None;
    let away = Point::new(640.0, 300.0);
    let mut frame = |window: &mut screens::Window, events: &[iced_core::Event]| {
        let said = testing::interact_kept(&mut kept, app::view(window, WINDOW), WINDOW, away, events);
        for message in &said {
            window.update(message.clone(), clock.at());
        }
        said
    };
    let greeted = screens::ServerMessage::Greeted {
        identity: terms::ServerIdentity::default(),
        limits: terms::FileLimits { attachment: Some(25_000_000), account: None },
    };
    window.update(WindowMessage::At(owls, PlaceMessage::Server(greeted)), clock.at());
    frame(&mut window, &[]);
    // The card shown while files hover is part of the very tree the drop then leaves.
    window.update(WindowMessage::FilesHovered(true), clock.at());
    assert!(window.drop_target().is_some(), "the channel takes the files");
    frame(&mut window, &[]);
    window.update(WindowMessage::FilesDropped(vec![std::path::PathBuf::from("match-point.png")]), clock.at());
    frame(&mut window, &[]);
    let enter = testing::key_press(iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Enter), false);
    let said = frame(&mut window, &[enter]);
    let sent = said
        .iter()
        .any(|message| matches!(message, WindowMessage::At(_, PlaceMessage::Discussion(DiscussionMessage::Send))));
    assert!(sent, "Enter after a drop sends, with no click: {said:?}");
}
