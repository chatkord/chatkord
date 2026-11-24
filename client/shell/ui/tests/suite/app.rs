//! The window after getting in: its controls and their order, what the keyboard and the pointer do to the model, its
//! states as pictures, and names that do not fit.

use crate::common;

use common::{Plain, focusable_total, semantics, sim_sized, tab};
use design::{Accent, Role, Scheme, SemanticRole, Surface};
use iced_core::Size;
use iced_core::keyboard::key::Named;
use proto::chatkord_v1::CloseReason;
use proto::rules::Grants;
use screens::colour::Message as Picking;
use screens::harness::{self, ADA, ADMIN_ROLE, Clock, MEMBER_ROLE, accepted, books, flow, owls, server, token};
use screens::{
    Ask, Channel, ChannelAction, DialogMessage, End, Heard, InviteMessage, MemberAction, MenuFrom, Message, Moment,
    Opening, Page, Person, Pick, PlaceAsk, PlaceId, PlaceMessage, Report, RoomEvent, Server, Standing, Track,
    UpdateFound, UpdateMessage, Window, WindowMessage,
};
use std::time::Instant;
use terms::{Ended, Secret};
use ui::testing::{self, Image, Renderer};
use ui::{Theme, app};
use words::Phrase;

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const WINDOW: Size = Size::new(960.0, 600.0);

/// A window tall enough for all of About Chatkord above its notices.
const TALL: Size = Size::new(960.0, 900.0);

/// The moment it is now, as the harness's clock shows it.
fn at() -> Moment {
    Clock(Instant::now()).at()
}

fn channel(id: &str, name: &str) -> Channel {
    Channel {
        id: screens::harness::channel_id(id),
        name: name.into(),
        people: Vec::new(),
        timeline: screens::harness::timeline_id(id),
    }
}

fn voices() -> Vec<Channel> {
    vec![channel("a", "Booth"), channel("b", "Lounge"), channel("c", &"Ω".repeat(80))]
}

/// `message` for the server `id`, now.
fn tell(window: &mut Window, id: PlaceId, message: PlaceMessage) -> Vec<Ask> {
    harness::tell(window, &Clock(Instant::now()), id, message)
}

/// Night Owls and Book club remembered with Ada's account; `signed` of them signed in on their tokens, with their
/// people listed.
fn launched(signed: usize) -> Window {
    let rows = vec![Opening::Account(owls(), ADA.into()), Opening::Account(books(), ADA.into())];
    let (mut window, _) = Window::launch(rows, None, Instant::now());
    for at in 0..signed {
        let id = window.places()[at].id();
        tell(&mut window, id, flow(token()));
        tell(&mut window, id, flow(accepted(Grants::default())));
        harness::people_listed(&mut window, &Clock(Instant::now()), id);
    }
    window
}

fn first(window: &Window) -> PlaceId {
    window.places()[0].id()
}

fn listed(channels: Vec<Channel>) -> Window {
    let mut window = launched(2);
    let id = first(&window);
    let listed = screens::harness::in_chat(screens::ChannelList { voice: channels, ..screens::ChannelList::default() });
    tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(listed))));
    window
}

/// Both of one's lanes up, as a call under way tells them after the room.
fn lanes_up() -> RoomEvent {
    RoomEvent::Lanes { send: screens::Lane::Up, receive: screens::Lane::Up }
}

/// A call with `names` in it, the first heard.
fn call(names: &[&str]) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Join(screens::harness::channel_id("a"))));
    let ids = |name: &&str| Person::new(screens::harness::member(&name.to_lowercase()), *name);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Joined {
            members: names.iter().map(ids).collect(),
            streams: names
                .iter()
                .map(|name| (screens::harness::member(&name.to_lowercase()), Track::Microphone))
                .collect(),
        })),
    );
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(lanes_up())));
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Levels(vec![(
            screens::harness::member(&names[0].to_lowercase()),
            90,
        )]))),
    );
    window
}

/// What a call's device menu lists: a default that follows the built-in device, a headset in use, and the built-in
/// one itself.
fn sound() -> screens::Sound {
    let listed =
        |asked: Option<&str>, label: &str| screens::Listed { asked: asked.map(str::to_string), label: label.into() };
    let both = || vec![listed(None, "Laptop"), listed(Some("Headset"), "Headset"), listed(Some("Laptop"), "Laptop")];
    screens::Sound {
        microphones: both(),
        speakers: both(),
        microphone: Some("Headset".into()),
        speaker: Some("Headset".into()),
    }
}

/// The call with the menu of microphones and speakers open over the call bar.
fn devices_open() -> Window {
    let mut window = call(&["Ada Lovelace", "Grace Hopper", "Μαργαρίτα"]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Sound(sound())));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Devices));
    window
}

/// The call with the popover of how its connection is open over the call panel, as it looks mid-call on a fair link.
fn connection_open() -> Window {
    let mut window = call(&["Ada Lovelace", "Grace Hopper"]);
    let id = first(&window);
    let flow = |bits_per_second, packets| screens::Flow { bits_per_second, packets };
    let health = screens::Health {
        round_trip: Some(screens::RoundTrip {
            took: std::time::Duration::from_millis(172),
            on: screens::MeasuredOn::Route,
        }),
        quality: Some(screens::Quality::Fair),
        sending: flow(48_000, 12_408),
        receiving: flow(96_000, 24_117),
        lost: 9,
        up_bytes: 12_400_000,
        down_bytes: 48_100_000,
    };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Health(health)));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Stats));
    window
}

fn render(theme: &Theme, window: &Window) -> Image {
    testing::render::<WindowMessage>(theme, app::view::<Renderer>(window, WINDOW), WINDOW, 1.0)
}

fn names<'a, M: 'a>(sim: &mut common::Sim<'a, M>) -> Vec<(SemanticRole, String)> {
    semantics(sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect()
}

fn says(found: &[(SemanticRole, String)], role: SemanticRole, said: &str) -> bool {
    found.iter().any(|(each, label)| *each == role && label == said)
}

#[test]
fn the_rail_names_each_server_and_what_its_marks_say_and_a_press_opens_one() {
    let window = launched(1);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::ListItem, "Night Owls"), "{found:?}");
    assert!(says(&found, SemanticRole::ListItem, "Book club, connecting"), "{found:?}");
    tab(&mut sim, false);
    tab(&mut sim, false);
    common::press(&mut sim, Named::Enter);
    let pressed: Vec<_> = sim.drain().collect();
    assert!(matches!(&pressed[..], [WindowMessage::Rail(id)] if *id == window.places()[1].id()), "{pressed:?}");
}

#[test]
fn the_pane_has_a_row_per_channel_and_the_keyboard_joins_one() {
    let mut window = listed(voices());
    let messages = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let found = names(&mut sim);
        assert!(says(&found, SemanticRole::ListItem, "Booth"), "{found:?}");
        assert_eq!(
            focusable_total(&mut sim),
            2 + 1 + 1 + 1 + 1 + 1 + 3 + 3 + 4 + 1 + 1,
            "the rail's two servers, + and gear; the members toggle; the menu; the category; three channels; the account bar's name, mute and deafen; four people; two splitters"
        );
        // Past the rail, the top bar, the menu and the category's header to the second channel.
        for _ in 0..9 {
            tab(&mut sim, false);
        }
        common::press(&mut sim, Named::Enter);
        sim.drain().collect::<Vec<_>>()
    };
    let todo: Vec<_> = messages.into_iter().flat_map(|message| window.update(message, at())).collect();
    assert!(
        matches!(&todo[..], [Ask::At(_, PlaceAsk::Join { channel_id, .. })] if channel_id.as_str() == "b"),
        "{todo:?}"
    );
}

#[test]
fn a_failed_list_offers_to_try_again() {
    let mut window = launched(1);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Err(terms::LinkError::Connect))));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(says(&names(&mut sim), SemanticRole::Button, &WORDS.phrase(Phrase::TryAgain)));
}

#[test]
fn a_call_has_one_tile_per_person_and_its_controls_mute_deafen_and_leave() {
    let window = call(&["Ada", "Grace"]);
    // Every channel is in a category, whose header folds it.
    let (rail_servers_plus_and_gear, pane_menu, category, channels) = (4, 1, 1, 3);
    // Each person's row under the channel opens their popover, Ada's own too; each other person's card does.
    let (people_rows, cards) = (2, 1);
    let (panel_bars, panel_leave) = (1, 1);
    // The account bar's name and picture, its mute and its deafen.
    let account_mute_deafen = 3;
    // A volume on each other person's card and a pin on each, shown or not; View, Audio controls, Chat and the
    // members toggle in the top bar.
    let (volumes, pins, header) = (1, 2, 4);
    let call_bar_mute_arrow_deafen_leave = 4;
    // Ada, grace, Bo and mira in the members column, each opening their popover.
    let members = 4;
    let splitter = 2;
    let panel_leave_at =
        rail_servers_plus_and_gear + header + pane_menu + category + channels + people_rows + panel_bars + panel_leave;
    let every = panel_leave_at
        + account_mute_deafen
        + cards
        + volumes
        + pins
        + call_bar_mute_arrow_deafen_leave
        + members
        + splitter;
    let press_at = |tabs: usize, window: &Window| {
        let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
        for _ in 0..tabs {
            tab(&mut sim, false);
        }
        common::press(&mut sim, Named::Enter);
        sim.drain().collect::<Vec<_>>()
    };
    {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let found = names(&mut sim);
        assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::LeaveTheCall)), "{found:?}");
        assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::MuteMic)), "{found:?}");
        assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::DeafenSelf)), "{found:?}");
        assert_eq!(focusable_total(&mut sim), every, "{found:?}");
    }
    let asked = |messages: Vec<WindowMessage>, window: &mut Window| -> Vec<Ask> {
        messages.into_iter().flat_map(|message| window.update(message, at())).collect()
    };
    let mut window = window;
    let leave = press_at(panel_leave_at, &window);
    let todo = asked(leave, &mut window);
    let left = |todo: &[Ask]| matches!(todo, [Ask::At(_, PlaceAsk::Leave), Ask::Chime(screens::Chime::Left)]);
    assert!(left(&todo), "the call panel leaves: {todo:?}");
    let mut window = call(&["Ada", "Grace"]);
    // After the panel's Leave, the account bar's name and picture, then its mute.
    let mute = press_at(panel_leave_at + 2, &window);
    let todo = asked(mute, &mut window);
    assert!(
        matches!(
            &todo[..],
            [Ask::At(_, PlaceAsk::Call(screens::Control::Mute(true))), Ask::Chime(screens::Chime::Muted)]
        ),
        "the account strip mutes: {todo:?}"
    );
    let mut window = call(&["Ada", "Grace"]);
    let leave_from_bar = press_at(every - splitter - members, &window);
    let todo = asked(leave_from_bar, &mut window);
    assert!(left(&todo), "the call bar leaves too: {todo:?}");
}

/// A call of Ada, Grace and Bo with Grace's card pinned.
fn pinned_call() -> Window {
    let mut window = call(&["Ada", "Grace", "Bo"]);
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Person(screens::harness::member("grace")))),
    );
    window
}

/// The tallest box named for `who`: their card, not their row in the side pane.
fn card_of(window: &Window, who: &str, pointer: Option<iced_core::Point>) -> Option<iced_core::Rectangle> {
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    if let Some(at) = pointer {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
        common::settle(&mut sim);
    }
    common::boxes(&mut sim)
        .into_iter()
        .filter(|(said, _)| said.label.as_str().starts_with(who))
        .map(|(_, bounds)| bounds)
        .max_by(|a, b| a.height.total_cmp(&b.height))
}

#[test]
fn a_hidden_strip_gives_its_height_to_the_stage() {
    let mut window = pinned_call();
    let shown = card_of(&window, "Grace", None).expect("the pinned card");
    assert_eq!(card_of(&window, "Bo", None).map(|bo| bo.height), Some(design::STRIP_TILE.1), "Bo is in the strip");
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleStrip(screens::Stage::Tall)));
    let hidden = card_of(&window, "Grace", None).expect("the pinned card");
    assert_eq!(hidden.height - shown.height, design::STRIP_TILE.1 + design::Space::M.px(), "the strip's and its gap");
    assert_eq!(card_of(&window, "Bo", None).map(|bo| bo.height < design::STRIP_TILE.1), Some(true), "only Bo's row");
}

#[test]
fn the_strip_toggle_shows_while_the_pointer_is_over_the_stage() {
    let mut window = pinned_call();
    let hide = WORDS.phrase(Phrase::HideStrip).to_string();
    assert!(card_of(&window, &hide, None).is_none(), "a toggle nobody sees is not read out");
    let stage = card_of(&window, "Grace", None).expect("the pinned card");
    let toggle = card_of(&window, &hide, Some(stage.center())).expect("the pointer over the stage shows its toggle");
    assert!(
        toggle.center_y() > stage.center_y() && (toggle.center_x() - stage.center_x()).abs() < 1.0,
        "bottom centre"
    );

    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    for at in [stage.center(), toggle.center()] {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
        common::settle(&mut sim);
    }
    sim.simulate([
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left)),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left)),
    ]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    drop(sim);
    for message in pressed {
        window.update(message, at());
    }
    let show = WORDS.phrase(Phrase::ShowStrip).to_string();
    assert!(card_of(&window, &show, Some(stage.center())).is_some(), "pressed, it hides the strip and offers it back");
}

/// A call of Ada and Grace, Grace's screen playing.
fn watching_grace() -> Window {
    let mut window = call(&["Ada", "Grace"]);
    let id = first(&window);
    let started = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::ScreenVideo };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(started)));
    let card = screens::ShareCard {
        who: screens::harness::member("grace"),
        width: 1920,
        height: 1080,
        frame_rate: Some(60),
        dropped: 0,
        codec: "H.264".into(),
        decoder: screens::Route::Hardware { via: "VA-API".into() },
        refused: 0,
        freezes: (0, std::time::Duration::ZERO),
        state: screens::ShareState::Live,
    };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
    window
}

/// Grace's screen over the whole display.
fn grace_full_screen() -> Window {
    let mut window = watching_grace();
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::FullScreen(screens::harness::member("grace"))));
    window
}

/// Whether `messages` ask for exactly `wanted` of the first place's call.
fn asks(messages: &[WindowMessage], wanted: &screens::CallMessage) -> bool {
    matches!(messages, [WindowMessage::At(_, PlaceMessage::Call(asked))] if format!("{asked:?}") == format!("{wanted:?}"))
}

/// Grace's screen playing and pinned, with Appearance open over the call: it floats.
fn grace_floating() -> Window {
    let mut window = watching_grace();
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Screen(screens::harness::member("grace")))),
    );
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), at());
    window
}

fn mouse(event: iced_core::mouse::Event) -> iced_core::Event {
    iced_core::Event::Mouse(event)
}

fn moved(position: iced_core::Point) -> iced_core::Event {
    mouse(iced_core::mouse::Event::CursorMoved { position })
}

const LEFT: iced_core::mouse::Button = iced_core::mouse::Button::Left;

#[test]
fn a_pinned_screen_floats_over_a_page_by_the_bottom_right_with_its_three_controls() {
    let window = grace_floating();
    let card = card_of(&window, "Grace’s stream", None).expect("the floating card");
    let (width, height) = design::FLOAT_CARD;
    let inset = design::FLOAT_INSET;
    assert_eq!(
        card,
        iced_core::Rectangle::new(
            iced_core::Point::new(WINDOW.width - width - inset, WINDOW.height - height - inset),
            Size::new(width, height),
        )
    );
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    sim.point_at(card.center());
    sim.simulate([moved(card.center())]);
    common::settle(&mut sim);
    let found = names(&mut sim);
    for said in [Phrase::GoToCall, Phrase::ResetPosition, Phrase::Close] {
        assert!(says(&found, SemanticRole::Button, &WORDS.phrase(said)), "{said:?}: {found:?}");
    }
    let go = card_of(&window, &WORDS.phrase(Phrase::GoToCall), Some(card.center())).expect("Go to the call");
    assert!(card.contains(go.center()), "on the card's own bar: {go:?}");
    let nudged = iced_core::Point::new(go.center().x + 3.0, go.center().y);
    sim.point_at(go.center());
    sim.simulate([moved(go.center()), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    sim.point_at(nudged);
    sim.simulate([moved(nudged), mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(
        matches!(&pressed[..], [WindowMessage::Float(screens::FloatMessage::GoToCall)]),
        "a press on the bar is the control's, never a drag: {pressed:?}"
    );
}

#[test]
fn a_drag_on_the_floating_card_sends_where_its_top_left_goes() {
    let window = grace_floating();
    let card = card_of(&window, "Grace’s stream", None).expect("the floating card");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let from = iced_core::Point::new(card.x + 40.0, card.y + card.height - 60.0);
    let to = iced_core::Point::new(from.x - 300.0, from.y - 200.0);
    sim.point_at(from);
    sim.simulate([moved(from), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    sim.point_at(to);
    sim.simulate([moved(to), mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
    let sent: Vec<WindowMessage> = sim.drain().collect();
    let dropped: Vec<(f32, f32)> = sent
        .iter()
        .filter_map(|message| match message {
            WindowMessage::Float(screens::FloatMessage::To(x, y)) => Some((*x, *y)),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, [(card.x - 300.0, card.y - 200.0)], "{sent:?}");
}

#[test]
fn a_watched_screens_bar_offers_full_screen() {
    let window = watching_grace();
    let screen = card_of(&window, "Grace’s stream", None).expect("Grace's screen card");
    let full = WORDS.phrase(Phrase::FullScreen).to_string();
    let button = card_of(&window, &full, Some(screen.center())).expect("its bar offers full screen");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    for at in [screen.center(), button.center()] {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
        common::settle(&mut sim);
    }
    sim.simulate([
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left)),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left)),
    ]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(asks(&pressed, &screens::CallMessage::FullScreen(screens::harness::member("grace"))), "{pressed:?}");
}

/// The buttons of the bar over the card named `who`, under the pointer, left to right.
fn bar_of(window: &Window, who: &str) -> Vec<String> {
    let card = card_of(window, who, None).expect("the card");
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    sim.point_at(card.center());
    sim.simulate([moved(card.center())]);
    common::settle(&mut sim);
    let mut bar: Vec<_> = common::boxes(&mut sim)
        .into_iter()
        .filter(|(said, bounds)| said.role == SemanticRole::Button && card.contains(bounds.center()))
        .filter(|(_, bounds)| bounds.center_y() < card.y + card.height / 4.0)
        .collect();
    bar.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    bar.into_iter().map(|(said, _)| said.label.as_str().to_string()).collect()
}

/// Grace's screen in a window of its own.
fn grace_out() -> Window {
    let mut window = watching_grace();
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::PopOut(
            screens::harness::member("grace"),
            proto::video::Video::Screen,
        )),
    );
    window
}

#[test]
fn a_watched_screens_bar_follows_the_models_order_with_about_at_the_corner() {
    let window = watching_grace();
    let said = |phrase| WORDS.phrase(phrase).to_string();
    let bar = bar_of(&window, "Grace’s stream");
    let order =
        [Phrase::PauseStream, Phrase::PictureInPicture, Phrase::FullScreen, Phrase::PinScreen, Phrase::AboutPicture]
            .map(said);
    assert_eq!(bar, order, "what is received, how it is viewed, then Pin and About at the corner");
    let out = said(Phrase::PictureInPicture);
    let screen = card_of(&window, "Grace’s stream", None).expect("Grace's screen card");
    let button = card_of(&window, &out, Some(screen.center())).expect("its bar offers picture in picture");
    let press = [iced_core::mouse::Event::ButtonPressed(LEFT), iced_core::mouse::Event::ButtonReleased(LEFT)];
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    for at in [screen.center(), button.center()] {
        sim.point_at(at);
        sim.simulate([moved(at)]);
        common::settle(&mut sim);
    }
    sim.simulate(press.map(mouse));
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(
        asks(&pressed, &screens::CallMessage::PopOut(screens::harness::member("grace"), proto::video::Video::Screen)),
        "{pressed:?}"
    );
}

/// Grace's screen playing with sound, paused for this viewer alone.
fn grace_paused() -> Window {
    let mut window = watching_grace();
    let id = first(&window);
    let sound = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::ScreenAudio };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(sound)));
    let card = screens::ShareCard {
        who: screens::harness::member("grace"),
        width: 1920,
        height: 1080,
        frame_rate: Some(60),
        dropped: 0,
        codec: "H.264".into(),
        decoder: screens::Route::Hardware { via: "VA-API".into() },
        refused: 0,
        freezes: (0, std::time::Duration::ZERO),
        state: screens::ShareState::Paused,
    };
    let began =
        screens::Watched::Paused { user_id: screens::harness::member("grace"), video: proto::video::Video::Screen };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Watched(began)));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Watched(screens::Watched::Card(card))));
    window
}

#[test]
fn a_paused_screens_bar_holds_resume_in_pauses_place_and_its_card_resumes_it() {
    let window = grace_paused();
    let said = |phrase| WORDS.phrase(phrase).to_string();
    let bar = bar_of(&window, "Grace’s stream");
    let order = [Phrase::Volume, Phrase::ResumeStream, Phrase::PictureInPicture, Phrase::FullScreen, Phrase::PinScreen]
        .map(said);
    assert_eq!(bar[..order.len()], order, "Resume in the slot of Pause: {bar:?}");
    let card = card_of(&window, "Grace’s stream", None).expect("its card");
    let found = texts(&window, WINDOW);
    let paused = found.iter().find(|(shown, _)| *shown == WORDS.phrase(Phrase::Paused)).map(|found| found.1);
    assert!(paused.is_some_and(|at| card.contains(at.center())), "Paused on its card: {found:?}");
    let resume = WORDS.phrase(Phrase::ResumeStream).to_string();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let middle = common::boxes(&mut sim)
        .into_iter()
        .filter(|(said, bounds)| {
            said.role == SemanticRole::Button
                && said.label.as_str() == resume
                && bounds.center_y() > card.y + card.height / 4.0
        })
        .map(|(_, bounds)| bounds)
        .next()
        .expect("the centre Resume");
    assert!(card.contains(middle.center()), "on the card: {middle:?}");
    sim.point_at(middle.center());
    sim.simulate([moved(middle.center()), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    sim.simulate([mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    let toggle = screens::CallMessage::TogglePause(screens::Pinned::Screen(screens::harness::member("grace")));
    assert!(asks(&pressed, &toggle), "the same as the bar's: {pressed:?}");
}

#[test]
fn a_screen_paused_before_it_was_watched_shows_the_veil_and_resume_before_any_picture() {
    let mut window = call(&["Ada", "Grace"]);
    let id = first(&window);
    let started = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::ScreenVideo };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(started)));
    let began =
        screens::Watched::Paused { user_id: screens::harness::member("grace"), video: proto::video::Video::Screen };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Watched(began)));
    let found = texts(&window, WINDOW);
    assert!(found.iter().any(|(shown, _)| *shown == WORDS.phrase(Phrase::Paused)), "the veil: {found:?}");
    assert!(
        !found.iter().any(|(shown, _)| *shown == WORDS.phrase(Phrase::ShareWaiting)),
        "no spinner waits for what only Resume starts: {found:?}"
    );
    let resume = WORDS.phrase(Phrase::ResumeStream).to_string();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let at = common::boxes(&mut sim)
        .into_iter()
        .filter(|(said, _)| said.role == SemanticRole::Button && said.label.as_str() == resume)
        .map(|(_, bounds)| bounds.center())
        .next()
        .expect("a Resume");
    sim.point_at(at);
    sim.simulate([moved(at), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    sim.simulate([mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    let toggle = screens::CallMessage::TogglePause(screens::Pinned::Screen(screens::harness::member("grace")));
    assert!(asks(&pressed, &toggle), "{pressed:?}");
}

#[test]
fn a_paused_screens_volume_is_dimmed_and_presses_nothing() {
    let window = grace_paused();
    let card = card_of(&window, "Grace’s stream", None).expect("its card");
    let volume = WORDS.phrase(Phrase::Volume).to_string();
    let button = card_of(&window, &volume, Some(card.center())).expect("its Volume");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    for at in [card.center(), button.center()] {
        sim.point_at(at);
        sim.simulate([moved(at)]);
        common::settle(&mut sim);
    }
    sim.simulate([
        mouse(iced_core::mouse::Event::ButtonPressed(LEFT)),
        mouse(iced_core::mouse::Event::ButtonReleased(LEFT)),
    ]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(pressed.is_empty(), "nothing until Resume: {pressed:?}");
}

/// The box of the popover about Grace's screen, which is open.
fn about_box(window: &Window) -> iced_core::Rectangle {
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    common::boxes(&mut sim)
        .into_iter()
        .find(|(said, _)| said.role == SemanticRole::Dialog && said.label.as_str() == "Grace’s stream")
        .map(|(_, bounds)| bounds)
        .expect("the popover about the picture")
}

#[test]
fn the_popover_about_a_picture_hangs_from_its_card_and_lies_over_the_call_whole() {
    let mut window = watching_grace();
    let id = first(&window);
    let grace = screens::harness::member("grace");
    let card = card_of(&window, "Grace’s stream", None).expect("its card");
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleInfo(grace.clone())));
    let popover = about_box(&window);
    assert_eq!(popover.width, design::ABOUT_POPOVER_WIDTH);
    assert!(
        popover.x >= 0.0 && popover.x + popover.width <= WINDOW.width && popover.y >= 0.0,
        "whole inside the window: {popover:?}"
    );
    // The card is narrower than the popover and stands at the area's left edge: the popover keeps its end in from the
    // card's unless that would put its start past the edge, where it moves the least to stay whole.
    let end = (card.x + card.width - design::ABOUT_END_INSET).max(card.x + design::ABOUT_POPOVER_WIDTH);
    assert_eq!(popover.x + popover.width, end, "its end in from the card's, or whole at the area's edge");

    // In the strip a card is 76 tall: the popover is taller than its card and rises to clear the call bar, so a card
    // that clipped it would show only its upper part.
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::CloseOver));
    let ada = screens::Pinned::Person(screens::harness::member("ada"));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Pin(ada)));
    let card = card_of(&window, "Grace’s stream", None).expect("its card in the strip");
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleInfo(grace)));
    let popover = about_box(&window);
    assert!(popover.height > card.height, "not cut to its card: {popover:?} over {card:?}");
    assert!(popover.y + popover.height <= WINDOW.height, "above the foot of the window: {popover:?}");
    assert!(popover.x >= 0.0 && popover.x + popover.width <= WINDOW.width, "{popover:?}");
}

#[test]
fn a_press_outside_the_popover_about_a_picture_closes_it() {
    let mut window = watching_grace();
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleInfo(screens::harness::member("grace"))));
    let popover = about_box(&window);
    let outside = iced_core::Point::new(popover.x - 10.0, popover.y + popover.height / 2.0);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    sim.point_at(outside);
    sim.simulate([moved(outside), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(asks(&pressed, &screens::CallMessage::CloseOver), "{pressed:?}");
}

#[test]
fn a_screen_in_its_own_window_says_so_on_its_card_whose_bar_brings_it_back() {
    let window = grace_out();
    let found = texts(&window, WINDOW);
    let card = card_of(&window, "Grace’s stream", None).expect("its card");
    let sentence = found.iter().find(|(said, _)| said == "Grace’s stream is in its own window").map(|found| found.1);
    assert!(sentence.is_some_and(|at| card.contains(at.center())), "on its card: {found:?}");
    let said = |phrase| WORDS.phrase(phrase).to_string();
    let bar = bar_of(&window, "Grace’s stream");
    assert_eq!(
        bar,
        [Phrase::ClosePictureInPicture, Phrase::PinScreen, Phrase::AboutPicture].map(said),
        "no full screen, zoom or volume for a picture that is elsewhere"
    );
}

#[test]
fn a_screens_own_window_shows_its_name_and_close_and_volume_while_it_has_sound() {
    let size = Size::new(design::FLOAT_CARD.0, design::FLOAT_CARD.1);
    let hovered = |window: &Window| {
        let view = app::out_view::<Renderer>(window, size).expect("Grace's screen is out");
        let mut sim = sim_sized(view, size);
        let middle = iced_core::Point::new(size.width / 2.0, size.height / 2.0);
        sim.point_at(middle);
        sim.simulate([moved(middle)]);
        common::settle(&mut sim);
        names(&mut sim)
    };
    assert!(app::out_view::<Renderer>(&watching_grace(), size).is_none(), "no screen out, no window for one");
    let mut window = grace_out();
    let found = hovered(&window);
    assert!(says(&found, SemanticRole::Dialog, "Grace’s stream"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::Close)), "{found:?}");
    assert!(!says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::Volume)), "no sound, no volume: {found:?}");
    let id = first(&window);
    let sound = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::ScreenAudio };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(sound)));
    assert!(says(&hovered(&window), SemanticRole::Button, &WORDS.phrase(Phrase::Volume)), "with sound, Volume");

    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ShareSound(screens::harness::member("grace"))));
    let view = app::out_view::<Renderer>(&window, size).expect("out");
    let mut sim = sim_sized(view, size);
    let found = names(&mut sim);
    assert!(found.iter().any(|(_, said)| said == "Sound from Grace’s stream"), "its popover opens there: {found:?}");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(!found.iter().any(|(_, said)| said == "Sound from Grace’s stream"), "not over the call: {found:?}");
}

#[test]
fn close_in_a_screens_own_window_brings_it_back() {
    let window = grace_out();
    let size = Size::new(design::FLOAT_CARD.0, design::FLOAT_CARD.1);
    let mut sim = sim_sized(app::out_view::<Renderer>(&window, size).expect("out"), size);
    let middle = iced_core::Point::new(size.width / 2.0, size.height / 2.0);
    sim.point_at(middle);
    sim.simulate([moved(middle)]);
    common::settle(&mut sim);
    let close = common::boxes(&mut sim)
        .into_iter()
        .find(|(said, _)| said.role == SemanticRole::Button && said.label.as_str() == WORDS.phrase(Phrase::Close))
        .map(|(_, bounds)| bounds)
        .expect("Close");
    assert!(close.x > size.width / 2.0 && close.y < size.height / 2.0, "at the top right: {close:?}");
    sim.point_at(close.center());
    sim.simulate([moved(close.center()), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
    sim.simulate([mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(asks(&pressed, &screens::CallMessage::PopIn), "{pressed:?}");
}

#[test]
fn full_screen_draws_only_the_picture_its_name_and_the_way_out() {
    let window = grace_full_screen();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, "Grace’s stream, full screen"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::LeaveFullScreen)), "{found:?}");
    assert!(
        !says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::LeaveTheCall)),
        "nothing of the window: {found:?}"
    );
    let leave = card_of(&window, &WORDS.phrase(Phrase::LeaveFullScreen), None).expect("the way out");
    assert!(leave.x > WINDOW.width / 2.0 && leave.y < WINDOW.height / 4.0, "at the top right: {leave:?}");
}

#[test]
fn escape_in_full_screen_leaves_it() {
    let window = grace_full_screen();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::press(&mut sim, Named::Escape);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(asks(&pressed, &screens::CallMessage::LeaveFullScreen), "{pressed:?}");
}

#[test]
fn tab_reaches_leave_full_screen_while_the_controls_are_hidden() {
    let mut window = grace_full_screen();
    let later = Moment { now: Instant::now() + design::FULL_SCREEN_CALM * 2, ..at() };
    window.update(WindowMessage::Tick, later);
    assert!(window.full_screen().is_some_and(|full| !full.controls), "the pointer rested");
    let leave = WORDS.phrase(Phrase::LeaveFullScreen).to_string();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(!says(&names(&mut sim), SemanticRole::Button, &leave), "a control nobody sees is not read out");
    tab(&mut sim, false);
    common::settle(&mut sim);
    assert!(says(&names(&mut sim), SemanticRole::Button, &leave), "the keyboard brings it back");
    // Zoom comes first in the bar, then Leave.
    tab(&mut sim, false);
    common::press(&mut sim, Named::Enter);
    let pressed: Vec<WindowMessage> = sim.drain().collect();
    assert!(asks(&pressed, &screens::CallMessage::LeaveFullScreen), "{pressed:?}");
}

/// Grace's screen pinned on the stage, zoom on when `zoomed`.
fn grace_pinned(zoomed: bool) -> Window {
    let mut window = watching_grace();
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Screen(screens::harness::member("grace")))),
    );
    if zoomed {
        tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleZoom(screens::harness::member("grace"))));
    }
    window
}

/// What the window's widgets publish for `events` with the pointer at `at`.
fn after(window: &Window, at: iced_core::Point, events: Vec<iced_core::Event>) -> Vec<WindowMessage> {
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    sim.point_at(at);
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
    common::settle(&mut sim);
    sim.drain().for_each(drop);
    sim.simulate(events);
    sim.drain().collect()
}

fn wheel(y: f32) -> iced_core::Event {
    iced_core::Event::Mouse(iced_core::mouse::Event::WheelScrolled {
        delta: iced_core::mouse::ScrollDelta::Lines { x: 0.0, y },
    })
}

#[test]
fn a_watched_screens_bar_offers_zoom_only_on_the_stage() {
    let zoom = WORDS.phrase(Phrase::ZoomPicture).to_string();
    let window = watching_grace();
    let screen = card_of(&window, "Grace’s stream", None).expect("Grace's screen card");
    assert!(card_of(&window, &zoom, Some(screen.center())).is_none(), "a screen in the grid has no zoom");
    let window = grace_pinned(false);
    let stage = card_of(&window, "Grace’s stream", None).expect("the pinned card");
    assert!(card_of(&window, &zoom, Some(stage.center())).is_some(), "pinned, it has");
    let window = grace_pinned(true);
    let stop = WORDS.phrase(Phrase::StopZooming).to_string();
    assert!(card_of(&window, &stop, Some(stage.center())).is_some(), "on, its button says how to stop");
}

#[test]
fn while_zoom_is_on_the_wheel_and_a_drag_say_where_the_pointer_is() {
    let window = grace_pinned(true);
    let stage = card_of(&window, "Grace’s stream", None).expect("the pinned card");
    let at = iced_core::Point::new(stage.x + 100.0, stage.y + 50.0);
    let wheeled = after(&window, at, vec![wheel(1.0)]);
    let [WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::ZoomWheel { notches, at: from, area }))] =
        &wheeled[..]
    else {
        panic!("one wheel step: {wheeled:?}");
    };
    assert_eq!(*notches, 1, "up zooms in");
    assert!((from.0 - 100.0).abs() < 1.0 && (from.1 - 50.0).abs() < 1.0, "from the picture's area: {from:?}");
    assert!((area.0 - stage.width).abs() < 1.0, "{area:?} against {stage:?}");
    let left = iced_core::mouse::Button::Left;
    let dragged = after(
        &window,
        at,
        vec![
            iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(left)),
            iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved {
                position: iced_core::Point::new(at.x + 30.0, at.y - 10.0),
            }),
            // Released past the window's edge: the drag still ends.
            iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved {
                position: iced_core::Point::new(-5.0, -5.0),
            }),
            iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(left)),
            iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at }),
        ],
    );
    let pans: Vec<(f32, f32)> = dragged
        .iter()
        .filter_map(|message| match message {
            WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::Pan { by, .. })) => Some(*by),
            _ => None,
        })
        .collect();
    assert_eq!(pans.len(), 2, "two moves while held, none after: {dragged:?}");
    assert!((pans[0].0 - 30.0).abs() < 1.0 && (pans[0].1 + 10.0).abs() < 1.0, "{pans:?}");
}

#[test]
fn without_zoom_the_wheel_is_left_to_what_holds_the_card() {
    let window = grace_pinned(false);
    let stage = card_of(&window, "Grace’s stream", None).expect("the pinned card");
    let wheeled = after(&window, stage.center(), vec![wheel(1.0)]);
    assert!(
        !wheeled.iter().any(|message| matches!(
            message,
            WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::ZoomWheel { .. }))
        )),
        "{wheeled:?}"
    );
}

#[test]
fn the_name_pill_says_the_zoom() {
    let mut window = grace_pinned(true);
    let id = first(&window);
    let shows = |window: &Window, said: &str| {
        let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
        sim.find(said).is_ok()
    };
    assert!(shows(&window, "100 %"), "on, even at the whole picture");
    let wheel = screens::CallMessage::ZoomWheel { notches: 15, at: (0.0, 0.0), area: (800.0, 450.0) };
    tell(&mut window, id, PlaceMessage::Call(wheel));
    assert!(shows(&window, "250 %"));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleZoom(screens::harness::member("grace"))));
    assert!(!shows(&window, "250 %") && !shows(&window, "100 %"), "off, nothing");
}

#[test]
fn a_strip_cards_bar_fits_its_card() {
    let mut window = watching_grace();
    let id = first(&window);
    let sound = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::ScreenAudio };
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(sound)));
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Pin(screens::Pinned::Person(screens::harness::member("ada")))),
    );
    let card = card_of(&window, "Grace’s stream", None).expect("Grace's screen in the strip");
    assert_eq!(card.height, design::STRIP_TILE.1, "in the strip");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    sim.point_at(card.center());
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: card.center() })]);
    common::settle(&mut sim);
    let full = WORDS.phrase(Phrase::FullScreen).to_string();
    let found = common::boxes(&mut sim);
    let bar: Vec<_> = found
        .iter()
        // The bar's band: the card's upper half, wherever across; a person's tile is the strip's full height.
        .filter(|(_, bounds)| {
            bounds.height < card.height / 2.0 && (card.y..card.y + card.height / 2.0).contains(&bounds.center_y())
        })
        .filter(|(said, _)| said.role == SemanticRole::Button)
        .collect();
    assert!(bar.iter().any(|(said, _)| said.label.as_str() == full), "its Full screen: {bar:?}");
    let about = WORDS.phrase(Phrase::AboutPicture).to_string();
    assert!(bar.iter().any(|(said, _)| said.label.as_str() == about), "every control, as Sharkord's: {bar:?}");
    assert_eq!(bar.len(), 5, "Volume, Picture in picture, Full screen, Pin, About: {bar:?}");
    // A row with no room squeezes its last buttons to nothing rather than past the card: each keeps its square.
    for (said, bounds) in &bar {
        assert!(
            bounds.width == bounds.height && bounds.x >= card.x && bounds.x + bounds.width <= card.x + card.width,
            "{} at {bounds:?}, squeezed or out of {card:?}",
            said.label.as_str()
        );
    }
}

#[test]
fn a_call_that_ended_says_why_and_its_card_goes_back() {
    let mut window = call(&["Ada"]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(End::Closed(None)))));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::BackToChannels)), "{found:?}");
    assert!(
        !says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::SwitchHere)),
        "only a move offers the seat: {found:?}"
    );
    let card = window.places()[0].call().unwrap().end_card(Grants::NONE).unwrap();
    assert_eq!(WORDS.say(&card.body), "The voice channel was closed.");
}

/// The channels after the server stopped answering: kept on screen while the connection is made again.
fn lost() -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Session(screens::SessionMessage::Disconnected(Ended { silent: true, ..Ended::default() })),
    );
    window
}

#[test]
fn a_lost_connection_says_so_and_joins_nothing() {
    let window = lost();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "Connection to Night Owls lost. Reconnecting…"), "{found:?}");
    assert!(found.iter().any(|(_, said)| said == "Booth"), "the list stays: {found:?}");
    assert_eq!(
        focusable_total(&mut sim),
        2 + 1 + 1 + 1 + 1 + 3 + 1,
        "the rail and its gear, the server's menu, the category's fold, the account bar's name and the next call's mute and deafen, and the splitter: no channel can be joined until the connection is back"
    );
}

/// A server that did not answer its first try.
fn down() -> Window {
    let mut window = launched(0);
    let id = first(&window);
    tell(&mut window, id, flow(Report::TokenLoaded(Some(Secret::new("t".into())))));
    tell(&mut window, id, flow(Report::Unreached(terms::LinkError::Timeout)));
    window
}

#[test]
fn a_server_not_answering_counts_down_and_try_now_tries() {
    let mut window = down();
    let messages = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let found = names(&mut sim);
        assert!(says(&found, SemanticRole::ListItem, "Night Owls, not answering"), "{found:?}");
        assert!(sim.find(Plain("Night Owls isn’t answering")).is_ok());
        assert!(
            sim.find(Plain("You’re still signed in. Trying again in 1 second. Your other servers keep working."))
                .is_ok()
        );
        sim.click(&*WORDS.phrase(Phrase::TryNow)).unwrap();
        sim.drain().collect::<Vec<_>>()
    };
    assert!(
        matches!(&messages[..], [WindowMessage::At(_, PlaceMessage::Session(screens::SessionMessage::TryNow))]),
        "{messages:?}"
    );
    let todo: Vec<_> = messages.into_iter().flat_map(|message| window.update(message, at())).collect();
    assert!(!todo.is_empty(), "a try at once");
}

/// A server whose session an admin ended by resetting the password.
fn signed_out(reason: CloseReason) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Session(screens::SessionMessage::Disconnected(Ended {
            reason: Some(reason),
            ..Ended::default()
        })),
    );
    window
}

#[test]
fn a_signed_out_server_says_why_and_another_account_clears_the_name() {
    let window = signed_out(CloseReason::PasswordReset);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.find(Plain("Sign in to Night Owls")).is_ok());
    assert!(
        sim.find(Plain("An admin reset your password. Sign in with the temporary password they gave you.")).is_ok()
    );
    sim.click(&*WORDS.phrase(Phrase::NotYou)).unwrap();
    let pressed: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &pressed[..],
            [WindowMessage::At(_, PlaceMessage::Session(screens::SessionMessage::Door(Message::OtherAccount)))]
        ),
        "{pressed:?}"
    );
}

#[test]
fn a_deleted_account_leaves_only_its_reason() {
    let window = signed_out(CloseReason::AccountDeleted);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.find(Plain("You’re no longer on Night Owls")).is_ok());
    assert!(sim.find(&*WORDS.phrase(Phrase::SignInAction)).is_err(), "nothing to sign in to");
}

#[test]
fn a_toast_is_said_as_a_status() {
    let mut window = listed(voices());
    let id = first(&window);
    let admin = proto::rules::Grant::ALL.into_iter().collect::<proto::rules::Grants>();
    tell(
        &mut window,
        id,
        PlaceMessage::Session(screens::SessionMessage::Standing {
            place: screens::harness::ADMIN_PLACE,
            grants: admin,
        }),
    );
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(says(&names(&mut sim), SemanticRole::Status, "You have a higher role on Night Owls now"));
}

/// The pane's header opens the server's menu under it, and an item is the model's to carry out.
#[test]
fn the_header_opens_the_servers_menu_and_its_items_say_what_they_do() {
    let mut window = listed(voices());
    let id = first(&window);
    let opened = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let closed = SemanticRole::Opens { layer: design::Popup::Menu, open: false };
        assert!(says(&names(&mut sim), closed, "Night Owls"), "the header opens a menu, closed");
        // The rail's four, then the top bar's members toggle, then the header.
        for _ in 0..6 {
            tab(&mut sim, false);
        }
        common::press(&mut sim, Named::Enter);
        sim.drain().collect::<Vec<_>>()
    };
    assert!(matches!(&opened[..], [WindowMessage::Menu(at, MenuFrom::Header)] if *at == id), "{opened:?}");
    window.update(opened[0].clone(), at());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Menu, "Night Owls"), "{found:?}");
    let items = [
        "Notification settings",
        "Copy server address",
        "Sign out of Night Owls",
        "Remove from this device",
        "Leave Night Owls",
    ];
    for item in items {
        assert!(says(&found, SemanticRole::MenuItem, item), "{item}: {found:?}");
    }
    common::press(&mut sim, Named::Enter);
    let picked: Vec<_> = sim.drain().collect();
    assert!(matches!(&picked[..], [WindowMessage::Pick(at, Pick::Notify)] if *at == id), "{picked:?}");
}

#[test]
fn a_right_click_on_a_server_in_the_rail_asks_for_its_menu() {
    let window = listed(voices());
    let middle = iced_core::Point::new(design::RAIL_WIDTH / 2.0, ui::RailItem::<WindowMessage>::HEIGHT / 2.0 + 8.0);
    let seen = testing::interact(app::view::<Renderer>(&window, WINDOW), WINDOW, middle, &testing::right_click());
    assert!(
        matches!(&seen.messages[..], [WindowMessage::Menu(at, MenuFrom::Rail)] if *at == first(&window)),
        "{:?}",
        seen.messages
    );
}

fn asked(pick: Pick) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    window.update(WindowMessage::Pick(id, pick), at());
    window
}

#[test]
fn a_confirmation_is_an_alert_that_escape_cancels_and_a_press_outside_asks_the_model() {
    let window = asked(Pick::SignOut);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::AlertDialog, "Sign out of Night Owls?"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::SignOutAction)), "{found:?}");
    common::press(&mut sim, Named::Escape);
    sim.point_at(iced_core::Point::new(10.0, 590.0));
    sim.simulate(testing::click());
    let said: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &said[..],
            [WindowMessage::Dialog(DialogMessage::Cancel), WindowMessage::Dialog(DialogMessage::Outside)]
        ),
        "{said:?}"
    );
}

/// Voice options and Audio controls hang under their buttons at the bar's end, each panel's right edge on its button's
/// while no column stands beside the call.
#[test]
fn the_calls_panels_hang_with_their_right_edges_on_their_buttons() {
    let opened =
        [(screens::CallMessage::ViewMenu, "Voice options"), (screens::CallMessage::AudioControls, "Audio controls")];
    for (message, name) in opened {
        let mut window = sized(call(&["Ada", "Grace"]), WINDOW, false);
        window.update(WindowMessage::ToggleMembers, at());
        let id = first(&window);
        tell(&mut window, id, PlaceMessage::Call(message));
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let boxes = common::boxes(&mut sim);
        let of = |wanted: fn(&SemanticRole) -> bool| {
            boxes
                .iter()
                .find(|(each, _)| wanted(&each.role) && each.label.as_str() == name)
                .map(|(_, at)| *at)
                .expect(name)
        };
        let button = of(|role| matches!(role, SemanticRole::Opens { .. }));
        let panel = of(|role| *role == SemanticRole::Dialog);
        assert!((panel.x + panel.width - (button.x + button.width)).abs() < 0.5, "{name}: {button:?} {panel:?}");
        assert!(panel.y >= button.y + button.height, "{name} under its button: {button:?} {panel:?}");
    }
}

/// A call's Voice options open.
fn view_menu() -> Window {
    let mut window = call(&["Ada", "Grace"]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ViewMenu));
    window
}

#[test]
fn voice_options_open_from_the_call_header_and_send_each_switch() {
    let window = call(&["Ada", "Grace"]);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let button = common::boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| {
            each.role == SemanticRole::Opens { layer: design::Popup::Dialog, open: false }
                && each.label.as_str() == "Voice options"
        })
        .expect("the header's Voice options")
        .1;
    sim.point_at(button.center());
    sim.simulate(testing::click());
    let said: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&said[..], [WindowMessage::At(_, PlaceMessage::Call(screens::CallMessage::ViewMenu))]),
        "{said:?}"
    );

    let window = view_menu();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, "Voice options"), "{found:?}");
    let switches: Vec<_> = common::boxes(&mut sim)
        .into_iter()
        .filter_map(|(each, at)| match each.role {
            SemanticRole::Switch { on } => Some((each.label.as_str().to_string(), on, at)),
            _ => None,
        })
        .collect();
    let named: Vec<(&str, bool)> = switches.iter().map(|(name, on, _)| (name.as_str(), *on)).collect();
    assert_eq!(
        named,
        [("Hide people with no picture", false), ("Always show voice controls", true), ("Hide my own share", false)],
        "Sharkord's order, the bar shown by default"
    );
    for (_, _, at) in &switches {
        sim.point_at(at.center());
        sim.simulate(testing::click());
    }
    let picked: Vec<_> = sim
        .drain()
        .filter_map(
            |each| if let WindowMessage::Look(choices::LookPick::View(pick)) = each { Some(pick) } else { None },
        )
        .collect();
    assert_eq!(picked, [choices::ViewPick::People, choices::ViewPick::Controls, choices::ViewPick::OwnShare]);
}

/// Whether the call bar's Leave, on the stage past the rail and the pane, is offered with the pointer at `at`.
fn leave_offered(window: &Window, at: iced_core::Point) -> bool {
    let leave = WORDS.phrase(Phrase::LeaveTheCall).to_string();
    let past_the_pane = design::RAIL_WIDTH + design::PANE_LEAST + 2.0 * design::BORDER_WIDTH;
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    common::settle(&mut sim);
    common::boxes(&mut sim).iter().any(|(said, bounds)| {
        said.role == SemanticRole::Button && said.label.as_str() == leave && bounds.x > past_the_pane
    })
}

#[test]
fn a_call_bar_shown_only_while_wanted_comes_with_the_pointer_over_the_call() {
    let on_the_rail = iced_core::Point::new(20.0, WINDOW.height / 2.0);
    // The middle of the stage, between the pane and the members column.
    let stage = (design::RAIL_WIDTH + design::PANE_LEAST + WINDOW.width - design::MEMBERS_WIDTH) / 2.0;
    let on_the_call = iced_core::Point::new(stage, WINDOW.height / 2.0);
    let mut window = call(&["Ada", "Grace"]);
    assert!(leave_offered(&window, on_the_rail), "always shown by default");
    window.update(WindowMessage::Look(choices::LookPick::View(choices::ViewPick::Controls)), at());
    assert!(!leave_offered(&window, on_the_rail), "gone with the pointer off the call");
    assert!(leave_offered(&window, on_the_call), "back with the pointer over it");
}

/// The window's close asked during a call.
fn asked_quit() -> Window {
    let mut window = call(&["Ada", "Grace"]);
    window.update(WindowMessage::CloseAsked, at());
    window
}

#[test]
fn closing_during_a_call_asks_to_quit_naming_the_call() {
    let window = asked_quit();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::AlertDialog, "Quit Chatkord?"), "{found:?}");
    let channel = window.dialog().and_then(screens::Dialog::call).expect("the call's channel").to_string();
    assert!(sim.find(format!("Closing Chatkord leaves the call in {channel}.").as_str()).is_ok());
    sim.click(Plain("Quit")).unwrap();
    let said: Vec<_> = sim.drain().collect();
    assert!(matches!(&said[..], [WindowMessage::Dialog(DialogMessage::Confirm)]), "{said:?}");
}

#[test]
fn leaving_asks_the_password_and_says_a_wrong_one() {
    let mut window = asked(Pick::Leave);
    let id = first(&window);
    window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("not it".into()))), at());
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), at());
    let wrong = terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::Unauthenticated);
    tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::LeaveAnswered(Err(wrong))));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::SecretInput, &WORDS.phrase(Phrase::YourPassword)), "{found:?}");
    assert!(sim.find(&*WORDS.phrase(Phrase::PasswordDoesntMatch)).is_ok());
    assert!(
        sim.find(Plain("This deletes your account on Night Owls. To come back, you’ll need a new invite.")).is_ok()
    );
}

#[test]
fn the_add_dialog_shows_the_welcome_steps_with_cancel_and_escape_goes_to_the_model() {
    let mut window = listed(voices());
    window.update(WindowMessage::Add, at());
    let id = window.add_dialog().unwrap().id();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, &WORDS.phrase(Phrase::AddTitle)), "{found:?}");
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::Cancel)), "{found:?}");
    assert!(says(&found, SemanticRole::Button, &WORDS.phrase(Phrase::Close)), "{found:?}");
    common::press(&mut sim, Named::Escape);
    let said: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&said[..], [WindowMessage::At(at, PlaceMessage::Session(screens::SessionMessage::Door(Message::Back)))] if *at == id),
        "{said:?}"
    );
}

#[test]
fn copying_writes_the_servers_address_to_the_clipboard() {
    let mut window = listed(voices());
    let id = first(&window);
    window.update(WindowMessage::Pick(id, Pick::Copy), at());
    let redraw = iced_core::Event::Window(iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()));
    let seen = testing::interact(app::view::<Renderer>(&window, WINDOW), WINDOW, iced_core::Point::ORIGIN, &[redraw]);
    assert_eq!(seen.clipboard, [iced_core::clipboard::Content::Text("chat.example.invalid".into())]);
}

#[test]
fn a_deleted_accounts_panel_removes_the_server_and_a_new_password_can_be_signed_out_of() {
    let window = signed_out(CloseReason::AccountDeleted);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.click(Plain("Remove Night Owls from this device")).is_ok());
    let said: Vec<_> = sim.drain().collect();
    assert!(matches!(&said[..], [WindowMessage::Remove(at)] if *at == first(&window)), "{said:?}");
}

/// How many controls hold the focus: never more than one.
fn focused_count<'a, M: 'a>(sim: &mut common::Sim<'a, M>) -> usize {
    struct Counting(usize);
    impl iced_core::widget::Operation for Counting {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
            operate(self);
        }
        fn focusable(
            &mut self,
            _id: Option<&iced_core::widget::Id>,
            _bounds: iced_core::Rectangle,
            state: &mut dyn iced_core::widget::operation::Focusable,
        ) {
            self.0 += usize::from(state.is_focused());
        }
    }
    let mut counting = Counting(0);
    sim.operate(&mut counting);
    counting.0
}

/// A menu opened from one server in the rail while another control has the focus takes it, and gives it back there:
/// one control holds the focus throughout.
#[test]
fn a_rail_menu_takes_the_focus_and_gives_it_back_to_the_control_that_had_it() {
    let before = listed(voices());
    let owls = first(&before);
    let mut open = listed(voices());
    open.update(WindowMessage::Menu(owls, MenuFrom::Rail), at());
    let after = listed(voices());
    let mut sim = sim_sized(app::view::<Renderer>(&before, WINDOW), WINDOW);
    tab(&mut sim, false);
    tab(&mut sim, false);
    assert_eq!(common::focused(&mut sim), Some(1), "Book club in the rail");
    let mut sim = sim.rebuild(app::view::<Renderer>(&open, WINDOW));
    common::settle(&mut sim);
    assert_eq!(focused_count(&mut sim), 1, "only the menu's first item");
    let mut sim = sim.rebuild(app::view::<Renderer>(&after, WINDOW));
    common::settle(&mut sim);
    assert_eq!((focused_count(&mut sim), common::focused(&mut sim)), (1, Some(1)), "back on Book club alone");
}

/// A rail of more servers than fit scrolls, says how far, and a server's menu opens beside its picture where the
/// scroll has put it.
#[test]
fn a_rail_menu_follows_the_rail_s_scroll() {
    let crowded = || {
        let rows = (0..20)
            .map(|n| Opening::Account(server(&format!("s{n}.example.invalid"), &format!("Server {n}")), ADA.into()))
            .collect();
        Window::launch(rows, None, Instant::now()).0
    };
    // A touchpad's pixels move the rail at once; a wheel's lines are eased over real time.
    let swipe = iced_core::Event::Mouse(iced_core::mouse::Event::WheelScrolled {
        delta: iced_core::mouse::ScrollDelta::Pixels { x: 0.0, y: -60.0 },
    });
    let told = after(&crowded(), iced_core::Point::new(20.0, 300.0), vec![swipe]);
    let scrolled = told
        .iter()
        .find_map(|message| match message {
            WindowMessage::RailScrolled(by) => Some(*by),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the rail did not say its scroll: {told:?}"));
    assert!(scrolled > 0.0, "{scrolled}");
    let menu_top = |scrolled: Option<f32>| {
        let mut window = crowded();
        if let Some(by) = scrolled {
            window.update(WindowMessage::RailScrolled(by), at());
        }
        let sixth = window.places()[5].id();
        window.update(WindowMessage::Menu(sixth, MenuFrom::Rail), at());
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        common::settle(&mut sim);
        let boxes = common::boxes(&mut sim);
        boxes.iter().find(|(each, _)| each.role == SemanticRole::MenuItem).expect("a menu").1.y
    };
    assert_eq!(menu_top(Some(scrolled)), menu_top(None) - scrolled);
}

/// The states a person meets, each built the way it is reached: by messages.
fn states() -> Vec<Window> {
    let mut joining = listed(voices());
    let id = first(&joining);
    tell(&mut joining, id, PlaceMessage::Lobby(screens::LobbyMessage::Join(screens::harness::channel_id("a"))));
    vec![
        launched(0),
        listed(voices()),
        listed(Vec::new()),
        joining,
        call(&["Ada Lovelace", "Grace Hopper", "Μαργαρίτα"]),
        lost(),
        down(),
        signed_out(CloseReason::PasswordReset),
        signed_out(CloseReason::AccountDeleted),
        menu_open(),
        asked(Pick::SignOut),
        asked(Pick::Remove),
        left_wrong(),
        owner_leaving(),
        adding(None),
        adding(Some("chat.example.invalid")),
        owner_menu(),
        inviting(),
        invite_made(),
        invites_page(),
        delete_asked(),
        members_page(true),
        members_page(false),
        reset_asked(),
        reset_done(),
        transfer_asked(),
        owner().0,
        channels_page(),
        channel_asked(None),
        channel_asked(Some(ChannelAction::Rename)),
        channel_asked(Some(ChannelAction::Delete)),
        account_page(),
        password_form(false),
        password_form(true),
        password_wrong(),
        devices_open(),
        connection_open(),
        voice_settings(false),
        voice_settings(true),
        stage_notice(),
        toast_in_call(),
        members_refused(),
        voice_settings_noticed(),
        person_menu_open(),
        held_in_call(),
        taken_out(),
        account_named(None),
        account_named(Some("Countess Ada of the long evenings")),
        profile_refused(),
        appearance_page(),
        overview_page(false, None),
        overview_page(true, None),
        overview_page(false, Some("Night Owls, the late-night gaming and film club for everyone who stays up")),
        in_call_elsewhere(false),
        in_call_elsewhere(true),
        overview_picture(&Stage::Held),
        overview_picture(&Stage::Uploading),
        overview_picture(&Stage::Refused),
        notifications_page(),
        asked_quit(),
        view_menu(),
        startup_page(),
        roles_page(true),
        roles_page(false),
        member_menu_open(screens::MemberMenu::Roles),
        member_menu_open(screens::MemberMenu::More),
        invite_custom("12"),
        invite_custom("101"),
        invite_role_menu(),
        held_own(true),
        held_own(false),
    ]
}

/// An administrator muted this account for everyone, its own mute `muted`: the microphone on the danger ground, its
/// icon the own mute, and the stage saying what the microphone is once the hold is lifted.
fn held_own(muted: bool) -> Window {
    let mut window = call(&["Ada Lovelace", "Bo"]);
    let id = first(&window);
    let ada = screens::harness::member("ada lovelace");
    let state = choices::State { mic_muted: muted, deafened: false };
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::State { user_id: ada.clone(), state })),
    );
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Held { user_id: ada, held: true })));
    window
}

/// How far a picture on Overview has gone.
enum Stage {
    Held,
    Uploading,
    Refused,
}

/// Overview at a server whose picture the host has ready (the rail shows it too): held, a new one going up, or one
/// refused as too big.
fn overview_picture(stage: &Stage) -> Window {
    use screens::picture_row::{Message as Row, Picked};
    let owl = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
    let (mut window, id) = owner();
    let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: Some(owl) };
    tell(
        &mut window,
        id,
        PlaceMessage::Server(screens::ServerMessage::Greeted { identity, limits: terms::FileLimits::default() }),
    );
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::PictureReady(owl)));
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    if matches!(stage, Stage::Held) {
        return window;
    }
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Picture(Row::Choose)));
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Picture(Row::Picked(Picked::File(vec![0; 16])))));
    if matches!(stage, Stage::Refused) {
        let refusal = proto::files::FileRefusal::TooLarge;
        let big = terms::FileError::Refused { kind: proto::files::FileKind::ServerPicture, refusal };
        tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Picture(Row::Uploaded(Err(big)))));
    }
    window
}

/// Server settings › Overview at a server that offers its name and colour, as its owner: the picker open, or a name
/// typed.
fn overview_page(picking: bool, typed: Option<&str>) -> Window {
    let (mut window, id) = owner();
    let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: None };
    tell(
        &mut window,
        id,
        PlaceMessage::Server(screens::ServerMessage::Greeted { identity, limits: terms::FileLimits::default() }),
    );
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    if let Some(typed) = typed {
        let typed = screens::NameFieldMessage::Typed(typed.into());
        tell(
            &mut window,
            id,
            PlaceMessage::Page(screens::PageMessage::Overview(screens::OverviewMessage::Name(typed))),
        );
    }
    if picking {
        tell(
            &mut window,
            id,
            PlaceMessage::Page(screens::PageMessage::Overview(screens::OverviewMessage::Colour(Picking::Toggle))),
        );
    }
    window
}

/// A press on the area and a drag along the strip pick a colour, each step sent as the point under the pointer; the
/// Hex field and Done are named for a screen reader.
#[test]
fn the_colour_picker_sends_the_point_a_drag_is_at_and_names_its_parts() {
    let window = overview_page(true, None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in
        [(SemanticRole::Image, "Colour and brightness"), (SemanticRole::Image, "Hue"), (SemanticRole::TextInput, "Hex")]
    {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    let boxes = common::boxes(&mut sim);
    let bounds = |said: &str| boxes.iter().find(|(each, _)| each.label.as_str() == said).map(|(_, at)| *at).unwrap();
    let (area, hue) = (bounds("Colour and brightness"), bounds("Hue"));
    let press = |sim: &mut common::Sim<'_, WindowMessage>, at: iced_core::Point| {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left))]);
    };
    let moved = |sim: &mut common::Sim<'_, WindowMessage>, at: iced_core::Point| {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: at })]);
    };
    let most_colour_and_light = iced_core::Point::new(area.x + area.width - 1.0, area.y + 1.0);
    press(&mut sim, most_colour_and_light);
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left))]);
    press(&mut sim, iced_core::Point::new(hue.x, hue.center_y()));
    moved(&mut sim, iced_core::Point::new(hue.x + hue.width / 3.0, hue.center_y()));
    let past_the_strip = iced_core::Point::new(hue.x + hue.width * 2.0, hue.center_y());
    moved(&mut sim, past_the_strip);
    let picked: Vec<Picking> = sim
        .drain()
        .filter_map(|each| match each {
            WindowMessage::At(
                _,
                PlaceMessage::Page(screens::PageMessage::Overview(screens::OverviewMessage::Colour(picking))),
            ) => Some(picking),
            _ => None,
        })
        .collect();
    let near = |got: f64, wanted: f64| (got - wanted).abs() < 0.02;
    assert!(matches!(picked[0], Picking::Area { x, y } if near(x, 1.0) && near(y, 0.0)), "{picked:?}");
    assert!(matches!(picked[1], Picking::Hue(x) if near(x, 0.0)), "{picked:?}");
    assert!(matches!(picked[2], Picking::Hue(x) if near(x, 1.0 / 3.0)), "{picked:?}");
    assert!(matches!(picked[3], Picking::Hue(x) if near(x, 1.0)), "{picked:?}");
}

fn appearance_page() -> Window {
    let mut window = listed(voices());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), at());
    window
}

fn notifications_page() -> Window {
    let mut window = listed(voices());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Notifications), at());
    window
}

/// Settings › Startup and window on a system that offers it, as it comes: not at sign-in, so Start in the system tray waits.
fn startup_page() -> Window {
    let remembered =
        choices::Remembered { startup: Some(choices::Startup::default()), ..choices::Remembered::default() };
    let mut window = listed(voices()).remembering(remembered, std::collections::BTreeMap::new());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Startup), at());
    window
}

/// Settings › Startup and window is the last page of this device where the system offers it; Start in the system
/// tray cannot be taken until Chatkord opens at sign-in, and each switch sends its flip.
#[test]
fn the_startup_page_flips_its_switches_and_start_minimized_waits_on_the_first() {
    let window = startup_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Radio { chosen: true }, "Startup and window"), "{found:?}");
    let boxes = common::boxes(&mut sim);
    let at = |said: &str| {
        boxes
            .iter()
            .find(|(each, _)| matches!(each.role, SemanticRole::Switch { .. }) && each.label.as_str() == said)
            .unwrap_or_else(|| panic!("no switch {said}: {found:?}"))
            .1
    };
    for said in [
        "Open Chatkord when you sign in to this device",
        "Start in the system tray",
        "Closing the window keeps Chatkord running",
    ] {
        sim.point_at(at(said).center());
        sim.simulate(testing::click());
    }
    let sent: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&sent[..], [WindowMessage::ToggleAtSignIn, WindowMessage::ToggleCloseTo]),
        "Start in the system tray waits on the first: {sent:?}"
    );
}

/// A tray choice made before the start at sign-in was turned off is still shown, on a switch that cannot be taken.
#[test]
fn start_in_the_tray_holds_its_value_while_the_start_at_sign_in_is_off() {
    let held = choices::Startup { starts_in: choices::StartsIn::Tray, ..choices::Startup::default() };
    let remembered = choices::Remembered { startup: Some(held), ..choices::Remembered::default() };
    let mut window = listed(voices()).remembering(remembered, std::collections::BTreeMap::new());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Startup), at());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Switch { on: true }, "Start in the system tray"), "{found:?}");
    let boxes = common::boxes(&mut sim);
    let tray = boxes
        .iter()
        .find(|(each, _)| each.label.as_str() == "Start in the system tray")
        .unwrap_or_else(|| panic!("no switch: {found:?}"))
        .1;
    sim.point_at(tray.center());
    sim.simulate(testing::click());
    assert_eq!(sim.drain().count(), 0, "it waits on the first switch");
}

/// Settings › Notifications is a page of this device, after Voice and video, and its switch turns the window's own
/// sounds off.
#[test]
fn the_notifications_page_switches_the_window_s_own_sounds() {
    let window = notifications_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Radio { chosen: true }, "Notifications"), "{found:?}");
    let pages: Vec<String> = found
        .iter()
        .filter(|(role, _)| matches!(role, SemanticRole::Radio { .. }))
        .map(|(_, said)| said.clone())
        .collect();
    let at = |said: &str| pages.iter().position(|page| page == said);
    assert_eq!(at("Notifications"), at("Voice and video").map(|before| before + 1), "{pages:?}");
    let sounds = common::boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| {
            matches!(each.role, SemanticRole::Switch { on: true }) && each.label.as_str() == "Event sounds"
        })
        .expect("the window's own sounds, on")
        .1;
    sim.point_at(sounds.center());
    sim.simulate(testing::click());
    let sent: Vec<_> = sim.drain().collect();
    assert!(matches!(&sent[..], [WindowMessage::ToggleEventSounds]), "{sent:?}");
}

/// The Appearance page names its choices for a screen reader, holds no colour, and a theme and the
/// switch each send their pick.
#[test]
fn the_appearance_page_sends_each_pick() {
    use choices::LookPick;
    let window = appearance_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Appearance"),
        (SemanticRole::RadioGroup, "Theme"),
        (SemanticRole::Radio { chosen: true }, "Match system"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    assert!(!says(&found, SemanticRole::RadioGroup, "Colour"), "{found:?}");
    let boxes = common::boxes(&mut sim);
    let at =
        |said: &str| boxes.iter().find(|(each, _)| each.label.as_str() == said).map(|(_, at)| at.center()).unwrap();
    let switch = boxes
        .iter()
        .find(|(each, _)| matches!(each.role, SemanticRole::Switch { .. }) && each.label.as_str() == "More contrast")
        .map(|(_, at)| at.center())
        .expect("the switch");
    for point in [at("Dark"), switch] {
        sim.point_at(point);
        sim.simulate(testing::click());
    }
    let picked: Vec<_> = sim.drain().collect();
    let wanted = [LookPick::Theme(design::Preference::Dark), LookPick::MoreContrast];
    let got: Vec<LookPick> = picked
        .iter()
        .filter_map(|each| if let WindowMessage::Look(pick) = each { Some(*pick) } else { None })
        .collect();
    assert_eq!(got, wanted, "{picked:?}");
}

/// Appearance on a system whose languages ask for English, with the Language field's menu open or not.
fn language_field(open: bool) -> Window {
    let mut window = appearance_page();
    window.update(WindowMessage::SystemLanguages(vec!["en-GB".parse().unwrap()]), at());
    if open {
        window.update(WindowMessage::LanguageMenu, at());
    }
    window
}

/// The Language field shows the choice, Match system naming the system's language, says it opens a menu that is shut,
/// and a press on it asks for the menu.
#[test]
fn the_language_field_shows_the_choice_and_a_press_asks_for_its_menu() {
    let window = language_field(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let boxes = common::boxes(&mut sim);
    let shut = SemanticRole::Opens { layer: design::Popup::Menu, open: false };
    let field = boxes
        .iter()
        .find(|(each, _)| each.role == shut && each.label.as_str() == "Language Match system (English)")
        .map(|(_, at)| at.center())
        .unwrap_or_else(|| panic!("the field, shut: {boxes:?}"));
    assert!(!boxes.iter().any(|(each, _)| each.role == SemanticRole::Menu), "no menu yet");
    sim.point_at(field);
    sim.simulate(testing::click());
    assert!(matches!(sim.drain().collect::<Vec<_>>()[..], [WindowMessage::LanguageMenu]));
}

/// Open, the menu under the field lists Match system with the system's language beside it, then each language
/// carried by its own name, the one taken marked; a reader is told the field is open and is in the menu; a pick sends
/// the language, and Escape closes the menu alone.
#[test]
fn the_language_menu_lists_the_choices_sends_a_pick_and_escape_closes_it() {
    let window = language_field(true);
    let tree = window_tree(&window);
    let (_, field) = tree_node(&tree, accesskit::Role::Button, "Language Match system (English)");
    assert_eq!((field.has_popup(), field.is_expanded()), (Some(accesskit::HasPopup::Menu), Some(true)));
    let (menu, _) = tree_node(&tree, accesskit::Role::Menu, "Language");
    let (_, focused) = tree.nodes.iter().find(|(id, _)| *id == tree.focus).unwrap();
    assert_eq!(focused.role(), accesskit::Role::MenuItemRadio);
    assert!(under(&tree, tree.focus, *menu), "the focus is on a choice of the menu");

    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let boxes = common::boxes(&mut sim);
    let field = boxes
        .iter()
        .find(|(each, _)| matches!(each.role, SemanticRole::Opens { open: true, .. }))
        .map(|(_, at)| *at)
        .expect("the field, open");
    let choices: Vec<_> = boxes
        .iter()
        .filter(|(each, _)| matches!(each.role, SemanticRole::MenuItemRadio { .. }))
        .map(|(each, at)| (each.role, each.label.as_str().to_string(), *at))
        .collect();
    let said: Vec<(SemanticRole, &str)> = choices.iter().map(|(role, said, _)| (*role, said.as_str())).collect();
    let carried: Vec<(SemanticRole, &str)> = words::Language::all()
        .map(|language| (SemanticRole::MenuItemRadio { chosen: false }, language.name()))
        .collect();
    assert_eq!(said[0], (SemanticRole::MenuItemRadio { chosen: true }, "Match system English"), "{said:?}");
    assert_eq!(said[1..], carried[..], "{said:?}");
    // Under the field where the menu fits below it, else over it: all its choices on one side, as wide as the field.
    let under = choices.iter().all(|(_, _, at)| at.y >= field.y + field.height);
    let over = choices.iter().all(|(_, _, at)| at.y + at.height <= field.y);
    assert!(under || over, "beside it: {choices:?} {field:?}");
    for (_, _, at) in &choices {
        assert!((at.width - field.width).abs() < field.width / 4.0, "as wide: {at:?} {field:?}");
    }
    let (_, _, english) = choices.iter().skip(1).find(|(_, said, _)| said == "English").expect("English");
    sim.point_at(english.center());
    sim.simulate(testing::click());
    let picked: Vec<_> = sim.drain().collect();
    assert!(
        matches!(picked[..], [WindowMessage::ChooseLanguage(Some(language))] if language == words::Language::ENGLISH),
        "{picked:?}"
    );

    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    common::press(&mut sim, Named::Escape);
    assert!(matches!(sim.drain().collect::<Vec<_>>()[..], [WindowMessage::CloseMenu]), "the menu, not the page");
}

fn zoom_picks(messages: &[WindowMessage]) -> Vec<choices::ZoomPick> {
    let zoom = |each: &WindowMessage| match each {
        WindowMessage::Look(choices::LookPick::Zoom(step)) => Some(*step),
        _ => None,
    };
    messages.iter().filter_map(zoom).collect()
}

/// The page's − and + and the keys Ctrl+=, Ctrl++, Ctrl+- and Ctrl+0 send the same steps, as Discord's and Teams'.
#[test]
fn the_zoom_keys_send_the_same_pick_as_the_page() {
    use choices::ZoomPick::{In, Out, Reset};
    let window = appearance_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let boxes = common::boxes(&mut sim);
    let at =
        |said: &str| boxes.iter().find(|(each, _)| each.label.as_str() == said).map(|(_, at)| at.center()).unwrap();
    for said in ["Zoom out", "Zoom in"] {
        sim.point_at(at(said));
        sim.simulate(testing::click());
    }
    sim.simulate(['=', '+', '-', '0'].map(testing::command));
    let picked: Vec<_> = sim.drain().collect();
    assert_eq!(zoom_picks(&picked), [Out, In, In, In, Out, Reset], "{picked:?}");
    let found = names(&mut sim);
    assert!(!says(&found, SemanticRole::Button, "Reset"), "no Reset at 100 %: {found:?}");
}

/// The keys zoom while a field holds the focus, and type nothing into it.
#[test]
fn the_zoom_keys_work_in_a_field_and_type_nothing() {
    let window = account_named(None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let boxes = common::boxes(&mut sim);
    let field = boxes.iter().find(|(each, _)| each.role == SemanticRole::TextInput).map(|(_, at)| at.center());
    sim.point_at(field.expect("the name field"));
    sim.simulate(testing::click());
    sim.simulate([testing::command('-')]);
    let picked: Vec<_> = sim.drain().collect();
    assert_eq!(zoom_picks(&picked), [choices::ZoomPick::Out], "{picked:?}");
    let typed = picked.iter().any(|each| {
        matches!(
            each,
            WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Typed(_))))
        )
    });
    assert!(!typed, "nothing is typed: {picked:?}");
}

/// Ctrl+K opens the server's search from any page of it, a field holding the focus or not, and types nothing.
#[test]
fn ctrl_k_opens_the_search_from_a_page_of_the_server() {
    let mut window = account_named(None);
    let served = screens::Served::read(&[]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Serves(served)));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let boxes = common::boxes(&mut sim);
    let field = boxes.iter().find(|(each, _)| each.role == SemanticRole::TextInput).map(|(_, at)| at.center());
    sim.simulate([testing::command('k')]);
    sim.point_at(field.expect("the name field"));
    sim.simulate(testing::click());
    sim.simulate([testing::command('k')]);
    let opened =
        |each: &WindowMessage| matches!(each, WindowMessage::At(_, PlaceMessage::Search(screens::SearchMessage::Open)));
    let picked: Vec<_> = sim.drain().collect();
    assert_eq!(picked.iter().filter(|each| opened(each)).count(), 2, "{picked:?}");
    let typed = picked.iter().any(|each| {
        matches!(
            each,
            WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Typed(_))))
        )
    });
    assert!(!typed, "nothing is typed: {picked:?}");
}

/// The search Ctrl+K opens takes the keyboard: the next letters typed are searched for, with no press first.
#[test]
fn the_search_ctrl_k_opens_takes_what_is_typed_next() {
    let mut window = account_named(None);
    let served = screens::Served::read(&[]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Serves(served)));
    let mut kept: Option<testing::Cache> = None;
    let mut frame = |window: &mut Window, events: &[iced_core::Event]| {
        let view = app::view::<Renderer>(window, WINDOW);
        let said = testing::interact_kept(&mut kept, view, WINDOW, iced_core::Point::ORIGIN, events);
        for message in &said {
            window.update(message.clone(), at());
        }
        said
    };
    frame(&mut window, &[testing::command('k')]);
    frame(&mut window, &[]);
    let said = frame(&mut window, &testing::typing("d"));
    let searched = said.iter().any(|each| {
        matches!(each, WindowMessage::At(_, PlaceMessage::Search(screens::SearchMessage::Type(typed))) if typed == "d")
    });
    assert!(searched, "{said:?}");
}

/// At 200 % + does nothing and Reset is offered; at 150 % the whole window is drawn larger in both schemes, the least
/// of it still on screen.
#[test]
fn a_zoomed_window_offers_reset_and_stops_at_the_top() {
    let mut window = appearance_page();
    for _ in 0..5 {
        window.update(WindowMessage::Look(choices::LookPick::Zoom(choices::ZoomPick::In)), at());
    }
    assert_eq!(window.look().zoom.percent(), 200);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Reset"), "{found:?}");
    let boxes = common::boxes(&mut sim);
    let plus = boxes.iter().find(|(each, _)| each.label.as_str() == "Zoom in").map(|(_, at)| at.center()).unwrap();
    sim.point_at(plus);
    sim.simulate(testing::click());
    assert_eq!(zoom_picks(&sim.drain().collect::<Vec<_>>()), [], "+ is off at the top");

    let mut window = appearance_page();
    for _ in 0..3 {
        window.update(WindowMessage::Look(choices::LookPick::Zoom(choices::ZoomPick::In)), at());
    }
    let zoom = window.look().zoom;
    assert_eq!(zoom.percent(), 150);
    let size = WINDOW * (1.0 / zoom.factor());
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let image = testing::render::<WindowMessage>(&theme, app::view::<Renderer>(&window, size), size, zoom.factor());
        ui::refs!().assert(&format!("app-zoom-150-{scheme:?}").to_lowercase(), &image);
    }
}

/// More contrast draws the window and its Appearance page with stronger text and edges, in both schemes; the
/// palette is the shell's to build from the look, so the test builds it the same way.
#[test]
fn more_contrast_draws_the_window_stronger() {
    let mut window = appearance_page();
    window.update(WindowMessage::Look(choices::LookPick::MoreContrast), at());
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = Theme::with_contrast(scheme, Accent::DEFAULT, window.look().contrast);
        ui::refs!().assert(&format!("app-more-contrast-{scheme:?}").to_lowercase(), &render(&theme, &window));
    }
}

/// An administrator in a call with Bo, a member, who is listed at his rank.
fn moderating() -> Window {
    let mut window = call(&["Ada Lovelace", "Bo"]);
    let id = first(&window);
    let grants = proto::rules::Grant::ALL.into_iter().collect();
    tell(
        &mut window,
        id,
        PlaceMessage::Session(screens::SessionMessage::Standing { place: screens::harness::ADMIN_PLACE, grants }),
    );
    // The words in each channel, as the list read again after the standing carries them.
    for channel in voices() {
        let standing = screens::ChannelStanding { grants, read: 0, unread: 0, mentions: 0 };
        tell(
            &mut window,
            id,
            PlaceMessage::Lobby(screens::LobbyMessage::Standing { channel_id: channel.id, standing }),
        );
    }
    // Becoming an administrator says so in a toast; opening the server puts it away.
    window.update(WindowMessage::Open(id), at());
    let person = |user_id: &str, name: &str, place| screens::Occupant {
        user_id: screens::harness::member(user_id),
        colour: None,
        name: name.into(),
        state: choices::State::default(),
        sharing: false,
        camera: false,
        held: false,
        place,
    };
    let people = vec![
        person("ada lovelace", "Ada Lovelace", screens::harness::ADMIN_PLACE),
        person("bo", "Bo", proto::rules::Place::BOTTOM),
    ];
    tell(
        &mut window,
        id,
        PlaceMessage::Lobby(screens::LobbyMessage::Occupancy { channel_id: screens::harness::channel_id("a"), people }),
    );
    window
}

/// Pressing Bo's tile opens what the administrator may do to him.
fn person_menu_open() -> Window {
    let mut window = moderating();
    let id = first(&window);
    window.update(WindowMessage::Person(id, screens::harness::member("bo"), screens::PersonFrom::Card), at());
    window
}

/// Bo is muted for everyone: Bo's tile says it in words, and the stage says why.
fn held_in_call() -> Window {
    let mut window = moderating();
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Started {
            user_id: screens::harness::member("bo"),
            track: Track::Microphone,
        })),
    );
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Held {
            user_id: screens::harness::member("bo"),
            held: true,
        })),
    );
    window
}

/// An administrator took this account out of the call: it says so and may join again.
fn taken_out() -> Window {
    let mut window = call(&["Ada Lovelace", "Bo"]);
    let id = first(&window);
    let end = screens::End::Closed(Some(proto::chatkord_v1::CloseReason::TakenOut));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(end))));
    window
}

/// Taken out of the call while the Appearance page was open over it: the page says so.
fn taken_out_under_a_page() -> Window {
    let mut window = call(&["Ada Lovelace", "Bo"]);
    let id = first(&window);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), at());
    let end = screens::End::Closed(Some(proto::chatkord_v1::CloseReason::TakenOut));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(end))));
    window
}

/// A call that ended under a settings page says so at the top of the page, with the card's way back, and its ✕ puts
/// the call away.
#[test]
fn a_call_ended_under_a_settings_page_says_so_there() {
    let window = taken_out_under_a_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "An admin took you out of Booth."), "{found:?}");
    assert!(says(&found, SemanticRole::Heading, "Appearance"), "the page stays: {found:?}");
    let boxes = common::boxes(&mut sim);
    let at = |role: SemanticRole, said: &str| {
        boxes.iter().find(|(each, _)| each.role == role && each.label.as_str() == said).map(|(_, at)| at.center())
    };
    let again = at(SemanticRole::Button, "Join again").expect("Join again");
    let away = at(SemanticRole::Button, "Dismiss").expect("its ✕");
    for point in [again, away] {
        sim.point_at(point);
        sim.simulate(testing::click());
    }
    let sent: Vec<_> = sim.drain().filter(|sent| !matches!(sent, WindowMessage::HoldToast(_))).collect();
    assert!(matches!(&sent[..], [WindowMessage::ToastAct, WindowMessage::DismissToast]), "{sent:?}");
    for scheme in [Scheme::Dark, Scheme::Light] {
        let image = render(&Theme::new(scheme, Accent::DEFAULT), &window);
        ui::refs!().assert(&format!("app-call-ended-under-a-page-{scheme:?}").to_lowercase(), &image);
    }
}

/// Book club open while the call is in Booth on Night Owls, then taken out there when `taken`.
fn in_call_elsewhere(taken: bool) -> Window {
    let mut window = call(&["Ada Lovelace", "Bo"]);
    let id = first(&window);
    let books = window.places()[1].id();
    window.update(WindowMessage::Open(books), at());
    if taken {
        let end = screens::End::Closed(Some(proto::chatkord_v1::CloseReason::TakenOut));
        tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(RoomEvent::Ended(end))));
    }
    window
}

/// The server a call is on carries headphones in the rail; a call taken out there is said on the server that is open,
/// naming the call's, and Join again goes back to it.
#[test]
fn a_call_on_another_server_is_marked_in_the_rail_and_its_end_is_said_on_the_open_one() {
    let window = in_call_elsewhere(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::ListItem, "Night Owls, you’re in a call here"), "{found:?}");
    assert!(says(&found, SemanticRole::ListItem, "Book club"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, "Leave the call"), "the pane keeps the call's panel: {found:?}");
    let boxes = common::boxes(&mut sim);
    let mute = boxes
        .iter()
        .find(|(each, _)| each.role == SemanticRole::Button && each.label.as_str() == "Mute")
        .map(|(_, at)| at.center())
        .expect("Mute");
    sim.point_at(mute);
    sim.simulate(testing::click());
    let sent: Vec<_> = sim.drain().collect();
    let owls = first(&window);
    assert!(
        matches!(&sent[..], [WindowMessage::At(a, PlaceMessage::Call(screens::CallMessage::ToggleMute))] if *a == owls),
        "mute is the call's, not the next call's: {sent:?}"
    );
    for scheme in [Scheme::Dark, Scheme::Light] {
        let image = render(&Theme::new(scheme, Accent::DEFAULT), &window);
        ui::refs!().assert(&format!("app-call-elsewhere-mark-{scheme:?}").to_lowercase(), &image);
    }

    let window = in_call_elsewhere(true);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "An admin took you out of Booth on Night Owls."), "{found:?}");
    assert!(says(&found, SemanticRole::ListItem, "Night Owls"), "the mark goes with the call: {found:?}");
    let boxes = common::boxes(&mut sim);
    let at = |role: SemanticRole, said: &str| {
        boxes.iter().find(|(each, _)| each.role == role && each.label.as_str() == said).map(|(_, at)| at.center())
    };
    let again = at(SemanticRole::Button, "Join again").expect("Join again");
    let away = at(SemanticRole::Button, "Dismiss").expect("its ✕");
    for point in [again, away] {
        sim.point_at(point);
        sim.simulate(testing::click());
    }
    let sent: Vec<_> = sim.drain().filter(|sent| !matches!(sent, WindowMessage::HoldToast(_))).collect();
    assert!(matches!(&sent[..], [WindowMessage::ToastAct, WindowMessage::DismissToast]), "{sent:?}");
    for scheme in [Scheme::Dark, Scheme::Light] {
        let image = render(&Theme::new(scheme, Accent::DEFAULT), &window);
        ui::refs!().assert(&format!("app-call-ended-elsewhere-{scheme:?}").to_lowercase(), &image);
    }
}

/// A call whose microphone fell back to another device: the stage says so under its header, and the tiles are what is
/// left.
fn stage_notice() -> Window {
    let mut window = call(&["Ada Lovelace", "Grace Hopper", "Μαργαρίτα"]);
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Call(screens::CallMessage::Notice(
            screens::Subject::Microphone,
            words::Sentence::SoundGone { was: "Headset".into(), now: Some("Desk".into()) },
        )),
    );
    window
}

/// A call, and a confirmation said at the foot of the content area, above the call bar.
fn toast_in_call() -> Window {
    let mut window = call(&["Ada Lovelace", "Grace Hopper"]);
    let id = first(&window);
    window.update(WindowMessage::Pick(id, Pick::Copy), at());
    window.update(WindowMessage::Copied(Ok(())), at());
    window
}

/// The members page, where giving the admin role was refused: the page says so above its list.
fn members_refused() -> Window {
    let mut window = members_page(true);
    let id = first(&window);
    let answer = Err(terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized));
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::RoleSet { name: "Bo".into(), answer }));
    window
}

/// Settings › Voice and video, where the engine said the saved headset was not there.
fn voice_settings_noticed() -> Window {
    let mut window = voice_settings(false);
    let said = words::Sentence::SoundGone { was: "Headset".into(), now: Some("Default".into()) };
    window.update(WindowMessage::Probed(screens::ProbeHeard::Notice(screens::Subject::Microphone, said)), at());
    window
}

/// A device a test picked, known by one word for both its id and its name.
fn picked(name: &str) -> choices::Picked {
    choices::Picked { id: name.into(), label: name.into() }
}

/// Settings › Voice and video, as the headset in use (or, with `away`, the default standing in for a saved microphone
/// that is unplugged), with the microphone loud.
fn voice_settings(away: bool) -> Window {
    let remembered = choices::Remembered {
        startup: None,
        microphone: Some(picked(if away { "Studio microphone" } else { "Headset" })),
        speaker: Some(picked("Headset")),
        next_call: choices::State::default(),
        share: choices::ShareQuality::default(),
        look: choices::Look::default(),
        event_sounds: choices::EventSounds::On,
        message_sounds: choices::MessageSounds::On,
        popups: choices::Popups::On,
        processing: choices::VoiceProcessing::default(),
        camera: choices::CameraSettings::default(),
        language: None,
        auto_update: choices::AutoUpdate::On,
        emoji: choices::EmojiPicks::default(),
    };
    let mut window = listed(voices()).remembering(remembered, std::collections::BTreeMap::new());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::VoiceAndVideo), at());
    let mut heard = sound();
    if away {
        heard.microphone = None;
    }
    window.update(WindowMessage::Probed(screens::ProbeHeard::Sound(heard)), at());
    window.update(WindowMessage::Probed(screens::ProbeHeard::Loudness(-29)), at());
    let cameras = ["Built-in camera", "USB camera"]
        .map(|name| screens::ListedCamera {
            camera: screens::Listed { asked: Some(name.into()), label: name.into() },
            sixty_at: Vec::new(),
        })
        .into();
    window.update(WindowMessage::Probed(screens::ProbeHeard::Cameras(Ok(cameras))), at());
    window
}

/// The account page of a member, opened by the gear on the account strip, with any name `typed` not saved yet.
fn account_named(typed: Option<&str>) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::OpenAccount));
    if let Some(typed) = typed {
        tell(
            &mut window,
            id,
            PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Typed(typed.into()))),
        );
    }
    window
}

/// The profile, its colour picker open when `picking`.
fn profile_colour(picking: bool) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    tell(
        &mut window,
        id,
        PlaceMessage::Server(screens::ServerMessage::Greeted {
            identity: terms::ServerIdentity::default(),
            limits: terms::FileLimits::default(),
        }),
    );
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::OpenAccount));
    if picking {
        tell(
            &mut window,
            id,
            PlaceMessage::Page(screens::PageMessage::ProfileColour(screens::colour::Message::Toggle)),
        );
        tell(
            &mut window,
            id,
            PlaceMessage::Page(screens::PageMessage::ProfileColour(screens::colour::Message::Typed("#5AA9FF".into()))),
        );
    }
    window
}

/// The profile offers the colour others see beside the person's picture in it, says who sees it, and sends what the
/// picker does.
#[test]
fn the_profile_offers_a_colour_beside_the_picture_in_it() {
    let window = profile_colour(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Your colour"), "{found:?}");
    let boxes = common::boxes(&mut sim);
    let swatch = boxes
        .iter()
        .find(|(each, _)| each.role == SemanticRole::Button && each.label.as_str() == "Your colour")
        .map(|(_, at)| at.center())
        .expect("the swatch");
    sim.point_at(swatch);
    sim.simulate(testing::click());
    let sent: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &sent[..],
            [WindowMessage::At(
                _,
                PlaceMessage::Page(screens::PageMessage::ProfileColour(screens::colour::Message::Toggle))
            )]
        ),
        "{sent:?}"
    );
    for (picking, name) in [(false, "closed"), (true, "picking")] {
        let window = profile_colour(picking);
        for scheme in [Scheme::Dark, Scheme::Light] {
            let image = render(&Theme::new(scheme, Accent::DEFAULT), &window);
            ui::refs!().assert(&format!("profile-colour-{name}-{scheme:?}").to_lowercase(), &image);
        }
    }
}

/// The server would not take the name typed.
fn profile_refused() -> Window {
    let mut window = account_named(Some("Countess"));
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Save)));
    let refused = terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::InvalidRequest);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::NameSet(Err(refused))));
    window
}

/// The account page of a member.
fn account_page() -> Window {
    account_named(None)
}

fn typed(window: &mut Window, field: screens::Field, said: &str) {
    let id = first(window);
    let message = screens::AccountMessage::Typed(field, Secret::new(said.into()));
    tell(window, id, PlaceMessage::Page(screens::PageMessage::Account(message)));
}

/// Change password open, empty or filled in ready to save.
fn password_form(filled: bool) -> Window {
    let mut window = account_page();
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Account(screens::AccountMessage::Change)));
    if filled {
        typed(&mut window, screens::Field::Current, "evening games with friends");
        typed(&mut window, screens::Field::New, "another correct phrase entirely");
        typed(&mut window, screens::Field::Again, "another correct phrase entirely");
    }
    window
}

/// The server said the current password is not the account's.
fn password_wrong() -> Window {
    let mut window = password_form(true);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Account(screens::AccountMessage::Save)));
    let wrong = terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::Unauthenticated);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::PasswordChanged(Err(wrong))));
    window
}

/// The account page shows the username and offers the name others see, typed and saved with Enter; Escape closes it.
#[test]
fn the_account_page_types_the_name_others_see_and_saves_it_with_enter() {
    let window = account_named(None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Account"),
        (SemanticRole::Radio { chosen: false }, "Voice and video"),
        (SemanticRole::Button, "Close"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    let field = common::boxes(&mut sim)
        .into_iter()
        .rfind(|(each, _)| each.label.as_str() == "Name others see")
        .expect("the name field")
        .1;
    sim.point_at(field.center());
    sim.simulate(testing::click());
    sim.typewrite("Countess");
    common::press(&mut sim, Named::Enter);
    common::press(&mut sim, Named::Escape);
    let said: Vec<_> = sim.drain().collect();
    assert!(
        said.iter().any(|each| matches!(each,
            WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Typed(typed)))) if typed == "Countess")),
        "{said:?}"
    );
    assert!(
        said.iter().any(|each| matches!(
            each,
            WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Name(screens::NameFieldMessage::Save)))
        )),
        "{said:?}"
    );
    assert!(
        matches!(said.last(), Some(WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::CloseAccount)))),
        "{said:?}"
    );
}

/// Where the window's one gear is, if it draws one: a button named Settings.
fn gear_at<'a, M: 'a>(sim: &mut common::Sim<'a, M>) -> Option<iced_core::Rectangle> {
    let settings = WORDS.phrase(Phrase::SettingsLabel);
    let gears: Vec<_> = common::boxes(sim)
        .into_iter()
        .filter(|(each, _)| each.role == SemanticRole::Button && each.label.as_str() == settings)
        .map(|(_, bounds)| bounds)
        .collect();
    assert!(gears.len() <= 1, "one way into the settings: {gears:?}");
    gears.first().copied()
}

/// The gear at the rail's foot opens the settings: the open server's profile while it is signed in, else this
/// device's Appearance, drawn whatever the server's state, with no account pages for a server not signed in.
#[test]
fn the_gear_opens_this_devices_settings_from_the_rail() {
    let mut window = listed(voices());
    let pressed = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let gear = gear_at(&mut sim).expect("the rail's gear");
        assert!(gear.x < design::RAIL_WIDTH, "in the rail: {gear:?}");
        assert!(gear.y + gear.height > WINDOW.height - design::Space::L.px(), "at its foot: {gear:?}");
        sim.point_at(gear.center());
        sim.simulate(testing::click());
        sim.drain().collect::<Vec<_>>()
    };
    assert!(matches!(&pressed[..], [WindowMessage::OpenSettings]), "{pressed:?}");
    window.update(WindowMessage::OpenSettings, at());
    assert_eq!(
        window.open().map(screens::Place::panel),
        Some(screens::Panel::Account),
        "the signed-in server’s own page first"
    );

    let mut window = launched(1);
    window.update(WindowMessage::Open(window.places()[1].id()), at());
    window.update(WindowMessage::OpenSettings, at());
    assert_eq!(window.device(), Some(screens::DevicePage::Appearance), "Book club is still connecting");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Heading, "Appearance"), "{found:?}");
    assert!(!says(&found, SemanticRole::Radio { chosen: false }, "Profile"), "no account to show: {found:?}");
    assert!(gear_at(&mut sim).is_some(), "the gear stays, marked");
}

#[test]
fn the_gear_shows_with_the_rail_whatever_the_servers_state_and_opens_this_devices_pages() {
    let mut window = launched(0);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(gear_at(&mut sim).is_some_and(|gear| gear.x < design::RAIL_WIDTH), "no server signed in: still there");
    drop(sim);
    window.update(WindowMessage::OpenSettings, at());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Heading, "Appearance"), "{found:?}");
    assert!(says(&found, SemanticRole::Radio { chosen: true }, "Appearance"), "{found:?}");
    for page in ["Voice and video", "Notifications", "About"] {
        assert!(says(&found, SemanticRole::Radio { chosen: false }, page), "{page}: {found:?}");
    }
    for page in ["Profile", "Account"] {
        assert!(!says(&found, SemanticRole::Radio { chosen: false }, page), "no account to show: {found:?}");
    }
    let window = listed(voices());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(gear_at(&mut sim).is_some_and(|gear| gear.x < design::RAIL_WIDTH), "none in the account strip either");
}

/// The account page names what a person can do there; Escape closes it; the form's Save does nothing until it is
/// ready and then asks.
#[test]
fn the_account_page_names_its_actions_and_its_password_form_saves_only_when_ready() {
    let window = account_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Account"),
        (SemanticRole::Radio { chosen: false }, "Voice and video"),
        (SemanticRole::Button, "Change password"),
        (SemanticRole::Button, "Sign out"),
        (SemanticRole::Button, "Leave"),
        (SemanticRole::Button, "Close"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    common::press(&mut sim, Named::Escape);
    let closed: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&closed[..], [WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::CloseAccount))]),
        "{closed:?}"
    );

    let window = password_form(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let _ = sim.click(&*WORDS.phrase(Phrase::SavePasswordAction));
    assert!(sim.drain().next().is_none(), "nothing typed: Save asks nothing");
    let window = password_form(true);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    sim.click(&*WORDS.phrase(Phrase::SavePasswordAction)).unwrap();
    let saved: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &saved[..],
            [WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Account(screens::AccountMessage::Save)))]
        ),
        "{saved:?}"
    );
}

/// What the password dialog says: its title and line, and under its fields a refusal on the current password, the
/// length and the match; Escape sends Cancel.
#[test]
fn the_password_dialog_says_why_and_closes_on_escape() {
    let window = password_wrong();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, "Change password"), "{found:?}");
    assert!(sim.find(&*WORDS.phrase(Phrase::ChangePasswordLine)).is_ok());
    assert!(sim.find(Plain("That password doesn’t match.")).is_ok());
    assert!(sim.find(&*WORDS.phrase(Phrase::LongEnough)).is_ok());
    assert!(sim.find(&*WORDS.phrase(Phrase::Matches)).is_ok());
    common::press(&mut sim, Named::Escape);
    let said: Vec<_> = sim.drain().collect();
    assert!(
        said.iter().any(|each| matches!(
            each,
            WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Account(screens::AccountMessage::Cancel)))
        )),
        "{said:?}"
    );
}

/// Who is in a voice room, by their names: the count a page shows is theirs.
fn occupants(names: &[&str]) -> Vec<screens::Occupant> {
    names
        .iter()
        .map(|name| screens::Occupant {
            user_id: screens::harness::member(&name.to_lowercase()),
            colour: None,
            name: (*name).into(),
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        })
        .collect()
}

/// Server settings › Channels, the first page an owner opens, with two people in the Lounge's call.
fn channels_page() -> Window {
    let (mut window, id) = owner();
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(Page::Channels)));
    tell(
        &mut window,
        id,
        PlaceMessage::Lobby(screens::LobbyMessage::Occupancy {
            channel_id: screens::harness::channel_id("b"),
            people: occupants(&["Bo", "Cy"]),
        }),
    );
    window
}

/// A server with no category has no channel: the pane and the Channels page say so, and offer Create category to
/// whoever manages the channels alone.
#[test]
fn a_server_with_no_category_offers_to_make_one_to_whoever_manages_the_channels() {
    let none = || PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(screens::ChannelList::default())));
    let (mut window, id) = owner();
    tell(&mut window, id, none());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Create category"), "the pane's empty state: {found:?}");
    let pressed = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        sim.click(Plain("CREATE CATEGORY")).expect("the pane's row");
        sim.drain().collect::<Vec<_>>()
    };
    assert!(matches!(&pressed[..], [WindowMessage::CreateCategory(at)] if *at == id), "{pressed:?}");
    let mut window = launched(2);
    let id = first(&window);
    tell(&mut window, id, none());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(!says(&found, SemanticRole::Button, "Create category"), "not offered to a member: {found:?}");
    let mut window = channels_page();
    let id = first(&window);
    tell(&mut window, id, none());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Create category"), "the page's header: {found:?}");
    assert!(!says(&found, SemanticRole::List, "Channels, in the server’s order"), "nothing to order: {found:?}");
}

/// A member of a server with no category reads one line where a manager is offered Create category, and no button.
#[test]
fn a_member_of_a_server_with_no_category_reads_that_there_is_no_channel_yet() {
    let mut window = launched(2);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Channels(Ok(screens::ChannelList::default()))));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.find(Plain(&WORDS.phrase(Phrase::NoChannelsYet))).is_ok(), "the pane's line");
    let found = names(&mut sim);
    assert!(!says(&found, SemanticRole::Button, "Create category"), "no button for a member: {found:?}");
}

/// Create channel with a name typed (`None`), or a button on the Lounge's row.
fn channel_asked(action: Option<ChannelAction>) -> Window {
    let mut window = channels_page();
    let id = first(&window);
    match action {
        Some(action) => window.update(WindowMessage::Channel(id, action, screens::harness::channel_id("b")), at()),
        None => {
            window.update(WindowMessage::CreateChannel(id), at());
            window.update(WindowMessage::Dialog(DialogMessage::Named("Movie night".into())), at())
        }
    };
    window
}

/// The channels page lists the channels in order with Create category and Create channel, and the chosen one's
/// editor beside it offers its name and Delete channel; the pane offers its gear only to whoever manages channels; a
/// delete is an alert that names the channel.
#[test]
fn the_channels_page_and_the_panes_plus_are_offered_to_whoever_manages_channels() {
    let window = channels_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Channels"),
        (SemanticRole::Button, "Create category"),
        (SemanticRole::Button, "Create channel"),
        (SemanticRole::List, "Channels, in the server’s order"),
        (SemanticRole::ListItem, "Lounge"),
        (SemanticRole::Button, "Close"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    let wide = Size::new(1280.0, 800.0);
    let mut window = channels_page();
    let id = first(&window);
    let lounge = screens::channel_settings::Message::Choose(screens::harness::channel_id("b"));
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Channels(lounge)));
    let mut sim = sim_sized(app::view::<Renderer>(&window, wide), wide);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::List, "Channels, in the server’s order"),
        (SemanticRole::TextInput, "Channel name"),
        (SemanticRole::Button, "Delete channel"),
    ] {
        assert!(says(&found, role, said), "the chosen channel's editor beside the list: {role:?} {said} in {found:?}");
    }
    // A row shows no button of its own for its menu, to its owner either: a right press opens it, as everywhere.
    let (window, _) = owner();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let under_the_bar = design::SERVER_BAR_HEIGHT + design::BORDER_WIDTH;
    let row = iced_core::Point::new(
        design::RAIL_WIDTH + design::PANE_LEAST / 2.0,
        under_the_bar + design::PANE_HEADER_HEIGHT + 30.0,
    );
    sim.point_at(row);
    sim.simulate([moved(row)]);
    common::settle(&mut sim);
    let gear = WORDS.phrase(Phrase::EditChannel);
    assert!(!names(&mut sim).iter().any(|(_, said)| *said == *gear), "no gear under the pointer");
    sim.simulate([
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Right)),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Right)),
    ]);
    let said: Vec<WindowMessage> = sim.drain().collect();
    assert!(said.iter().any(|said| matches!(said, WindowMessage::ListMenu(..))), "{said:?}");
    assert!(!said.iter().any(|said| matches!(said, WindowMessage::Menu(_, MenuFrom::Pane))), "the row's alone");
    // Where no row is, the right press is the list's own: what is made there.
    let empty = iced_core::Point::new(design::RAIL_WIDTH + design::PANE_LEAST / 2.0, WINDOW.height - 160.0);
    sim.point_at(empty);
    sim.simulate([
        moved(empty),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Right)),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Right)),
    ]);
    let said: Vec<WindowMessage> = sim.drain().collect();
    assert!(matches!(&said[..], [WindowMessage::Menu(_, MenuFrom::Pane)]), "{said:?}");
    let window = channel_asked(Some(ChannelAction::Delete));
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::AlertDialog, "Delete Lounge?"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, "Delete Lounge"), "{found:?}");
}

/// The server's owner, holding every word, its channels listed and the toast that said so put away.
fn owner() -> (Window, PlaceId) {
    let mut window = listed(voices());
    let id = first(&window);
    let grants = proto::rules::Grant::ALL.into_iter().collect();
    tell(
        &mut window,
        id,
        PlaceMessage::Session(screens::SessionMessage::Standing { place: proto::rules::Place::OWNER, grants }),
    );
    window.update(WindowMessage::Open(id), at());
    (window, id)
}

fn owner_menu() -> Window {
    let (mut window, id) = owner();
    window.update(WindowMessage::Menu(id, MenuFrom::Header), at());
    window
}

fn inviting() -> Window {
    let (mut window, id) = owner();
    window.update(WindowMessage::Pick(id, Pick::Invite), at());
    window.update(WindowMessage::Dialog(DialogMessage::Invite(InviteMessage::Note("for Grace".into()))), at());
    window
}

/// The owner inviting at a server whose roles are read, so the dialog offers the role to give.
fn inviting_with_roles() -> Window {
    let (mut window, id) = owner();
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::RolesListed(Ok(harness::server_roles()))));
    window.update(WindowMessage::Pick(id, Pick::Invite), at());
    window
}

fn invite_said(window: &mut Window, said: InviteMessage) {
    window.update(WindowMessage::Dialog(DialogMessage::Invite(said)), at());
}

/// Custom picked among the people an invite admits, `typed` in its field.
fn invite_custom(typed: &str) -> Window {
    let mut window = inviting_with_roles();
    invite_said(&mut window, InviteMessage::Uses(screens::UsesMessage::Custom));
    invite_said(&mut window, InviteMessage::Uses(screens::UsesMessage::Typed(typed.into())));
    window
}

/// The Role field open, the default role taken.
fn invite_role_menu() -> Window {
    let mut window = inviting_with_roles();
    invite_said(&mut window, InviteMessage::RoleMenu);
    window
}

fn invite_made() -> Window {
    let mut window = inviting();
    let id = first(&window);
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), at());
    let invited = terms::Invited { code: Secret::new("Kx7fQ2".repeat(7) + "a"), expires_at: 1_793_491_200 };
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Invited(Ok(invited))));
    window
}

fn invites_page() -> Window {
    let (mut window, id) = owner();
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(Page::Invites)));
    let row = |id: &str, uses, max_uses, note: Option<&str>| proto::chatkord_v1::InviteSummary {
        invite_id: id.into(),
        uses,
        max_uses,
        expires_at: 1_793_491_200,
        note: note.map(str::to_string),
        ..Default::default()
    };
    tell(
        &mut window,
        id,
        PlaceMessage::Page(screens::PageMessage::Invites(Ok(vec![
            row("1", 2, 5, Some("for Grace")),
            row("2", 0, 1, None),
        ]))),
    );
    window
}

fn delete_asked() -> Window {
    let mut window = invites_page();
    let id = first(&window);
    window.update(WindowMessage::DeleteInvite(id, screens::harness::invite_id("1")), at());
    window
}

/// The invite dialog names its choices and which is taken, and the keyboard reaches Create; the link made is copied
/// from its own button.
#[test]
fn the_invite_dialog_says_its_choices_and_its_link_is_copied_from_it() {
    let window = inviting();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::RadioGroup, "Can be used by"),
        (SemanticRole::Radio { chosen: true }, "5 people"),
        (SemanticRole::Radio { chosen: true }, "7 days"),
        (SemanticRole::Button, "Create invite link"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    common::press(&mut sim, Named::Enter);
    let mut pressed: Vec<_> = sim.drain().collect();
    pressed.retain(|message| matches!(message, WindowMessage::Dialog(DialogMessage::Confirm)));
    assert!(!pressed.is_empty(), "Enter creates the link");
    let window = invite_made();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Copy") && says(&found, SemanticRole::Button, "Manage invites"));
}

/// The role the invite gives is offered to who may give roles and to no one else, and Custom shows a slider and a field
/// that are one count, a number out of bounds refused under them.
#[test]
fn the_invite_dialog_offers_a_role_and_a_custom_count_as_the_account_may() {
    let named = |window: &Window| {
        let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
        names(&mut sim)
    };
    let role_field = |found: &[(SemanticRole, String)]| {
        found.iter().any(|(role, said)| matches!(role, SemanticRole::Opens { .. }) && said.starts_with("Role"))
    };
    let found = named(&inviting_with_roles());
    assert!(role_field(&found), "the Role field: {found:?}");
    assert!(says(&found, SemanticRole::Radio { chosen: false }, "Custom"), "{found:?}");
    assert!(!found.iter().any(|(role, _)| matches!(role, SemanticRole::Slider { .. })), "no slider before Custom");
    let found = named(&inviting());
    assert!(!role_field(&found), "no roles read, none offered: {found:?}");
    let found = named(&invite_custom("12"));
    assert!(
        found.iter().any(|(role, said)| {
            matches!(role, SemanticRole::Slider { value: 12, .. }) && said == "People who can use it"
        }),
        "the slider stands at the count typed: {found:?}"
    );
    assert!(says(&found, SemanticRole::Radio { chosen: true }, "Custom"), "{found:?}");
    let refused = invite_custom("101");
    let mut sim = sim_sized(app::view::<Renderer>(&refused, WINDOW), WINDOW);
    assert!(sim.find(Plain("Pick a number from 1 to 100.")).is_ok(), "a number past the most is refused in a sentence");
}

/// The settings take the pane's place, list the invites, and their Delete and Close reach the model.
#[test]
fn the_invites_page_lists_each_with_delete_and_closes() {
    let window = invites_page();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Invites"),
        (SemanticRole::Table, "Invites"),
        (SemanticRole::Button, "New invite"),
        (SemanticRole::Button, "Close"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    assert!(!says(&found, SemanticRole::ListItem, "Booth"), "the pane is not drawn");
    for _ in 0..focusable_total(&mut sim) {
        tab(&mut sim, false);
        common::press(&mut sim, Named::Enter);
    }
    let pressed: Vec<_> = sim.drain().collect();
    assert!(
        pressed.iter().any(|m| matches!(m, WindowMessage::DeleteInvite(_, invite) if invite.as_str() == "1")),
        "{pressed:?}"
    );
    assert!(
        pressed
            .iter()
            .any(|m| matches!(m, WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::CloseSettings)))),
        "{pressed:?}"
    );
}

/// Close stays where it is however far a settings page is scrolled, and still leaves the settings from there.
#[test]
fn a_settings_page_scrolled_to_its_end_keeps_close_in_place() {
    /// Each scrollable snapped to its end, or its offset read; the most any is scrolled.
    struct Scroll(bool, f32);
    impl iced_core::widget::Operation for Scroll {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _id: Option<&iced_core::widget::Id>,
            bounds: iced_core::Rectangle,
            content: Size,
            translation: iced_core::Vector,
            state: &mut dyn iced_core::widget::operation::Scrollable,
        ) {
            if self.0 {
                let end = iced_core::widget::operation::scrollable::RelativeOffset { x: None, y: Some(1.0) };
                state.snap_to(end, iced_core::widget::operation::Animation::Instant, bounds, content);
            }
            self.1 = self.1.max(translation.y);
        }
    }
    let size = Size::new(1280.0, 400.0);
    let window = sized(voice_settings(true), size, false);
    let close = WORDS.phrase(Phrase::Close).to_string();
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    let (_, rest) = common::boxes(&mut sim).into_iter().find(|(each, _)| each.label.as_str() == close).expect("Close");
    sim.operate(&mut Scroll(true, 0.0));
    common::settle(&mut sim);
    let mut scrolled = Scroll(false, 0.0);
    sim.operate(&mut scrolled);
    assert!(scrolled.1 > 100.0, "the page scrolled, by {}", scrolled.1);
    sim.drain().for_each(drop);
    sim.point_at(rest.center());
    sim.simulate([
        iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: rest.center() }),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left)),
        iced_core::Event::Mouse(iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left)),
    ]);
    let pressed: Vec<_> = sim.drain().collect();
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::CloseDevice)), "{pressed:?}");
}

fn menu_open() -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    window.update(WindowMessage::Menu(id, MenuFrom::Header), at());
    window
}

fn left_wrong() -> Window {
    let mut window = asked(Pick::Leave);
    let id = first(&window);
    window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("not it".into()))), at());
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), at());
    let wrong = terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::Unauthenticated);
    tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::LeaveAnswered(Err(wrong))));
    window
}

fn owner_leaving() -> Window {
    let (mut window, id) = owner();
    window.update(WindowMessage::Pick(id, Pick::Leave), at());
    window
}

fn member(id: &str, name: &str, owner: bool, admin: bool) -> proto::chatkord_v1::MemberEntry {
    let roles = if admin { vec![ADMIN_ROLE.to_string(), MEMBER_ROLE.into()] } else { vec![MEMBER_ROLE.into()] };
    let grants =
        if admin || owner { proto::rules::Grants::from_iter(proto::rules::Grant::ALL).words() } else { Vec::new() };
    proto::chatkord_v1::MemberEntry {
        user_id: id.into(),
        handle: name.into(),
        display_name: name.into(),
        role_ids: roles,
        place: if owner {
            proto::rules::Place::OWNER
        } else if admin {
            screens::harness::ADMIN_PLACE
        } else {
            proto::rules::Place::BOTTOM
        }
        .wire(),
        grants,
        colour: None,
        banned: false,
    }
}

/// In the least window a member's buttons go on a line under the name rather than squeeze it to a letter a line; at
/// the usual width they stay beside it.
#[test]
fn in_a_narrow_window_a_members_buttons_go_under_the_name() {
    let window = members_page(true);
    let below = |size: Size| {
        let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
        let boxes = common::boxes(&mut sim);
        let bo = boxes.iter().find(|(each, _)| each.label.as_str() == "Bo").expect("Bo's picture").1;
        let more =
            boxes.iter().find(|(each, _)| each.label.as_str() == "More for Bo").map(|(_, at)| *at).expect("Bo's More");
        more.y >= bo.y + bo.height
    };
    assert!(!below(WINDOW), "beside the name at the usual width");
    let narrow = Size::new(design::WINDOW_LEAST.0, 600.0);
    assert!(below(narrow), "under the name when narrow");
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let image = testing::render(&theme, app::view::<Renderer>(&window, narrow), narrow, 1.0);
    ui::refs!().assert("app-members-narrow", &image);
}

/// The members page, as the server's owner Ada, or as Ada an admin while mira owns it.
fn members_page(owner: bool) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    let grants = proto::rules::Grant::ALL.into_iter().collect();
    let place = if owner { proto::rules::Place::OWNER } else { screens::harness::ADMIN_PLACE };
    tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::Standing { place, grants }));
    window.update(WindowMessage::Open(id), at());
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(Page::Members)));
    let members = vec![
        member("ada", ADA, owner, !owner),
        member("2", "grace", false, true),
        member("3", "Bo", false, false),
        member("4", "mira", !owner, false),
    ];
    harness::roles_listed(&mut window, &Clock(Instant::now()), id);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::MembersListed(Ok(members))));
    window
}

/// The roles page, as the server's owner Ada with the admin role chosen, or as Ada an admin, whose own role is locked.
fn roles_page(owner: bool) -> Window {
    let mut window = listed(voices());
    let id = first(&window);
    let grants = proto::rules::Grant::ALL.into_iter().collect();
    let place = if owner { proto::rules::Place::OWNER } else { screens::harness::ADMIN_PLACE };
    tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::Standing { place, grants }));
    window.update(WindowMessage::Open(id), at());
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(Page::Roles)));
    harness::roles_listed(&mut window, &Clock(Instant::now()), id);
    let admin = screens::RolesMessage::Choose(screens::harness::role_id(ADMIN_ROLE));
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Roles(admin)));
    window
}

/// The roles page on its list, the role chosen put back: where the list and the role do not fit side by side, the list.
fn roles_list_page() -> Window {
    let mut window = roles_page(true);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Roles(screens::RolesMessage::Back)));
    window
}

/// The members page with a menu open on Bo's row.
fn member_menu_open(menu: screens::MemberMenu) -> Window {
    let mut window = members_page(true);
    let id = first(&window);
    let open = screens::MembersMessage::Menu(screens::harness::member("3"), menu);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Members(open)));
    window
}

fn reset_asked() -> Window {
    let mut window = members_page(true);
    let id = first(&window);
    window.update(WindowMessage::Member(id, MemberAction::ResetPassword, screens::harness::member("2")), at());
    window
}

fn reset_done() -> Window {
    let mut window = reset_asked();
    let id = first(&window);
    window.update(WindowMessage::Dialog(DialogMessage::Confirm), at());
    tell(
        &mut window,
        id,
        PlaceMessage::Page(screens::PageMessage::PasswordReset(Ok(Secret::new("river-copper-lamp-42".into())))),
    );
    window
}

fn transfer_asked() -> Window {
    let mut window = members_page(true);
    let id = first(&window);
    window.update(WindowMessage::Member(id, MemberAction::MakeOwner, screens::harness::member("2")), at());
    window
}

/// Each member's row offers what the server allows this account: a Roles menu ticking each role it may give, a More
/// menu of the rest; their entries reach the model, and an admin sees only what their place allows.
#[test]
fn the_members_page_offers_each_row_its_menus_and_their_entries_reach_the_model() {
    use screens::{MemberMenu, MembersMessage};
    let window = members_page(true);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    let menu = |open| SemanticRole::Opens { layer: design::Popup::Menu, open };
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Members (4)"),
        (SemanticRole::Radio { chosen: false }, "Banned"),
        (SemanticRole::TextInput, "Search members"),
        (SemanticRole::List, "Members"),
        (menu(false), "Roles"),
        (menu(false), "More for Bo"),
        (menu(false), "More for grace"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    let id = first(&window);
    let opened = |menu: MemberMenu, who: &str| {
        let mut window = members_page(true);
        let message = MembersMessage::Menu(screens::harness::member(who), menu);
        tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Members(message)));
        window
    };
    let window = opened(MemberMenu::Roles, "3");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, menu(true), "Roles"), "{found:?}");
    assert!(says(&found, SemanticRole::MenuItemCheckbox { checked: false }, "Admin"), "{found:?}");
    assert!(sim.click(Plain("Admin")).is_ok());
    let pressed: Vec<_> = sim.drain().collect();
    let admin = || (screens::harness::role_id(ADMIN_ROLE), "Admin".to_string());
    assert!(
        pressed.iter().any(|m| matches!(m, WindowMessage::Member(_, MemberAction::GiveRole(role, name), user)
            if (role.clone(), name.clone()) == admin() && user.as_str() == "3")),
        "a press ticks the role: {pressed:?}"
    );
    let window = opened(MemberMenu::More, "2");
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    for said in ["Reset password", "Make owner", "Kick grace", "Ban grace"] {
        assert!(says(&found, SemanticRole::MenuItem, said), "{said} in {found:?}");
    }
    let window = members_page(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    let rows: Vec<&str> = found
        .iter()
        .filter(|(role, _)| matches!(role, SemanticRole::Opens { .. }))
        .map(|(_, said)| said.as_str())
        .collect();
    assert_eq!(rows, ["More for Bo"], "an admin acts only on Bo, below them, and gives no role at their own place");
}

/// The roles page lists the order with its notes and draws the chosen role's editor, whose name, words and Delete reach
/// the model; an admin's own role is locked, with nothing to change.
#[test]
fn the_roles_page_lists_the_order_and_the_chosen_role_s_editor_reaches_the_model() {
    let wide = Size::new(1280.0, 800.0);
    let window = roles_page(true);
    let mut sim = sim_sized(app::view::<Renderer>(&window, wide), wide);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::List, "Roles, highest first"),
        (SemanticRole::ListItem, "Owner Server owner"),
        (SemanticRole::ListItem, "Admin"),
        (SemanticRole::ListItem, "Member Everyone"),
        (SemanticRole::Button, "Create role"),
        (SemanticRole::TextInput, "Role name"),
        (SemanticRole::Switch { on: true }, "Manage roles"),
        (SemanticRole::Button, "Delete role"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    for _ in 0..focusable_total(&mut sim) {
        tab(&mut sim, false);
        common::press(&mut sim, Named::Enter);
    }
    let pressed: Vec<_> = sim.drain().collect();
    let role = |message: &WindowMessage, wanted: fn(&screens::RolesMessage) -> bool| matches!(message, WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::Roles(said))) if wanted(said));
    assert!(pressed.iter().any(|m| role(m, |said| matches!(said, screens::RolesMessage::Choose(_)))), "{pressed:?}");
    assert!(
        pressed.iter().any(|m| role(m, |said| matches!(said, screens::RolesMessage::Word(_, false)))),
        "{pressed:?}"
    );
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::CreateRole(_))), "{pressed:?}");
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::DeleteRole(_))), "{pressed:?}");
    let window = roles_page(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, wide), wide);
    let found = names(&mut sim);
    assert!(!found.iter().any(|(role, said)| *role == SemanticRole::TextInput && said == "Role name"), "{found:?}");
    assert!(!says(&found, SemanticRole::Button, "Delete role"), "{found:?}");
    let mut window = roles_page(true);
    let id = first(&window);
    let member = screens::RolesMessage::Choose(screens::harness::role_id(MEMBER_ROLE));
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Roles(member)));
    let mut sim = sim_sized(app::view::<Renderer>(&window, wide), wide);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::TextInput, "Role name"), "the default role is renamed: {found:?}");
    assert!(!says(&found, SemanticRole::Button, "Delete role"), "and never deleted: {found:?}");
    let narrow = roles_page(true);
    let mut sim = sim_sized(app::view::<Renderer>(&narrow, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Roles"), "a role opened alone goes back to the list: {found:?}");
    assert!(!says(&found, SemanticRole::List, "Roles, highest first"), "{found:?}");
}

/// A temporary password is shown with Copy and Done, and no Cancel; Copy reaches the model.
#[test]
fn a_reset_shows_the_temporary_password_once_with_copy_and_done() {
    let window = reset_done();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Button, "Copy") && says(&found, SemanticRole::Button, "Done"));
    assert!(!says(&found, SemanticRole::Button, "Cancel"), "{found:?}");
    for _ in 0..focusable_total(&mut sim) {
        tab(&mut sim, false);
        common::press(&mut sim, Named::Enter);
    }
    let pressed: Vec<_> = sim.drain().collect();
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::Dialog(DialogMessage::CopyTemporary))), "{pressed:?}");
}

/// The server owner leaving is offered "Choose a new owner", which asks the model.
#[test]
fn an_owner_leaving_is_offered_to_choose_a_new_owner() {
    let window = owner_leaving();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(says(&names(&mut sim), SemanticRole::Button, "Choose a new owner"));
    common::press(&mut sim, Named::Enter);
    let pressed: Vec<_> = sim.drain().collect();
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::Dialog(DialogMessage::Confirm))), "{pressed:?}");
}

/// The add dialog, with `typed` looked for (a server in the list, so it is found there).
fn adding(typed: Option<&str>) -> Window {
    let mut window = listed(voices());
    window.update(WindowMessage::Add, at());
    if let Some(typed) = typed {
        let id = window.add_dialog().unwrap().id();
        tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::Door(Message::Where(typed.into()))));
        tell(&mut window, id, PlaceMessage::Session(screens::SessionMessage::Door(Message::Submit)));
    }
    window
}

#[test]
fn each_screen_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (at, window) in states().iter().enumerate() {
            ui::refs!().assert(&format!("app-{scheme:?}-{at}").to_lowercase(), &render(&theme, window));
        }
    }
}

/// A name that does not fit is the pane's to hold, not to spill: nothing is painted in the content area beside it.
#[test]
fn a_name_that_does_not_fit_stays_inside_the_pane() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let base = theme.palette().color(Role::Surface(Surface::Base));
    let from = (design::RAIL_WIDTH + design::PANE_LEAST + 2.0 * design::BORDER_WIDTH) as u32;
    for name in ["x".repeat(300), "Α".repeat(120), "e\u{301}".repeat(100)] {
        let image = render(&theme, &listed(vec![channel("a", &name)]));
        for y in design::SERVER_BAR_HEIGHT as u32 + 1..image.size.height / 4 {
            for x in from..from + 16 {
                assert_eq!(image.pixel(x, y), [base.r, base.g, base.b, 255], "{name:?} painted at {x},{y}");
            }
        }
    }
}

/// The page names its lists, ticks what is in use, lists a saved microphone that is away without making it a control,
/// and sends what a person does: a pick, the switch, the test sound, Escape; the nav leads back to the account.
#[test]
fn the_voice_settings_say_their_devices_and_send_what_is_done() {
    let window = voice_settings(true);
    // Tall enough for the whole page: the microphone, its processing and the speaker.
    let tall = Size::new(WINDOW.width, 1400.0);
    let mut sim = sim_sized(app::view::<Renderer>(&window, tall), tall);
    let found = names(&mut sim);
    for (role, said) in [
        (SemanticRole::Radio { chosen: true }, "Voice and video"),
        (SemanticRole::Radio { chosen: false }, "Account"),
        (SemanticRole::Button, "Play a test sound"),
        (SemanticRole::Radio { chosen: true }, "Default Laptop"),
        (SemanticRole::Radio { chosen: false }, "Studio microphone Not connected"),
        (SemanticRole::Radio { chosen: true }, "Headset"),
    ] {
        assert!(says(&found, role, said), "{role:?} {said} in {found:?}");
    }
    let away = common::boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| each.label.as_str() == "Studio microphone Not connected")
        .expect("the saved microphone is listed")
        .1;
    sim.point_at(away.center());
    sim.simulate(testing::click());
    assert!(sim.drain().next().is_none(), "a microphone that is not connected is not picked");

    sim.click(Plain("Play a test sound")).unwrap();
    let sent: Vec<_> = sim.drain().collect();
    assert!(sent.iter().any(|each| matches!(each, WindowMessage::PlayTestSound)), "{sent:?}");
    let mut press = |label: &str| {
        let control =
            common::boxes(&mut sim).into_iter().find(|(each, _)| each.label.as_str() == label).expect("the control").1;
        sim.point_at(control.center());
        sim.simulate(testing::click());
        sim.drain().collect::<Vec<_>>()
    };
    let sent = press("Test");
    assert!(sent.iter().any(|each| matches!(each, WindowMessage::TestMicrophone)), "{sent:?}");
    let sent = press("Automatic gain");
    let off = choices::VoiceProcessing { automatic_gain: choices::AutomaticGain::Off, ..Default::default() };
    assert!(sent.iter().any(|each| matches!(each, WindowMessage::SetProcessing(asked) if *asked == off)), "{sent:?}");
    let sent = press("Standard");
    let standard = choices::VoiceProcessing { suppression: choices::Suppression::Standard, ..Default::default() };
    assert!(
        sent.iter().any(|each| matches!(each, WindowMessage::SetProcessing(asked) if *asked == standard)),
        "{sent:?}"
    );
    assert!(
        !common::boxes(&mut sim).iter().any(|(each, _)| each.label.as_str().contains("Join calls muted")),
        "no Join calls muted switch"
    );

    let window = voice_settings(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, tall), tall);
    let rows: Vec<_> = common::boxes(&mut sim)
        .into_iter()
        .filter(|(each, _)| each.label.as_str() == "Headset" && matches!(each.role, SemanticRole::Radio { .. }))
        .collect();
    assert_eq!(rows.len(), 2, "one in the microphones and one in the speakers");
    sim.point_at(rows[0].1.center());
    sim.simulate(testing::click());
    sim.point_at(rows[1].1.center());
    sim.simulate(testing::click());
    let picked: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&picked[..], [WindowMessage::PickMicrophone(Some(mic)), WindowMessage::PickSpeaker(Some(speaker))]
            if mic.id == "Headset" && speaker.id == "Headset"),
        "{picked:?}"
    );
    sim.click(Plain("Account")).unwrap();
    common::press(&mut sim, Named::Escape);
    let nav: Vec<_> = sim.drain().collect();
    assert!(matches!(&nav[0], WindowMessage::At(_, PlaceMessage::Page(screens::PageMessage::OpenAccount))), "{nav:?}");
    assert!(matches!(nav.last(), Some(WindowMessage::CloseDevice)), "{nav:?}");
}

/// While the probe has not started, or could not, the page still draws: no rows yet, no test sound, and a reason.
#[test]
fn the_voice_settings_without_a_probe_offer_no_test_sound_and_say_why() {
    let mut window = listed(voices());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::VoiceAndVideo), at());
    {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let _ = sim.click(Plain("Play a test sound"));
        assert!(sim.drain().next().is_none(), "nothing to play a sound on yet");
    }
    window.update(WindowMessage::Probed(screens::ProbeHeard::Failed(words::Failure::VoiceEngine)), at());
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.find(Plain("Your voice could not be set up on this device.")).is_ok());
}

/// `window` told it is drawn in `size`, with its drawer opened when `open` asks and the frame is folded.
fn sized(mut window: Window, size: Size, open: bool) -> Window {
    window.set_room(screens::frame::Room { width: size.width, height: size.height });
    if open {
        window.update(WindowMessage::ToggleChannels, at());
    }
    window
}

/// Every control a person can reach in `window` drawn at `size`, with its box.
fn reachable(window: &Window, size: Size) -> Vec<(design::Semantics, iced_core::Rectangle)> {
    let mut sim = sim_sized(app::view::<Renderer>(window, size), size);
    common::boxes(&mut sim)
}

/// The controls that fill the top bar's height, or a folded header's, as `(name, box)` left to right.
fn along_the_top(window: &Window, size: Size) -> Vec<(String, iced_core::Rectangle)> {
    let rail = if size.width >= screens::frame::RAIL_FROM { design::RAIL_WIDTH } else { 0.0 };
    let mut found: Vec<_> = reachable(window, size)
        .into_iter()
        .filter(|(_, bounds)| bounds.y + bounds.height <= design::SERVER_BAR_HEIGHT && bounds.x >= rail)
        .map(|(each, bounds)| (each.label.as_str().to_string(), bounds))
        .collect();
    found.sort_by(|(_, a), (_, b)| a.x.total_cmp(&b.x));
    found
}

/// What fills the top bar's height, by name.
fn named_along_the_top(window: &Window, size: Size) -> Vec<String> {
    along_the_top(window, size).into_iter().map(|(said, _)| said).collect()
}

/// A channel opened takes what is typed next in its composer, as every peer app does: no click on the field first.
#[test]
fn an_opened_channel_takes_what_is_typed_next_in_its_composer() {
    let clock = Clock(Instant::now());
    let (mut window, _) = harness::chatting(&clock);
    let mut kept: Option<testing::Cache> = None;
    let mut frame = |window: &mut Window, events: &[iced_core::Event]| {
        let view = app::view::<Renderer>(window, WINDOW);
        let said = testing::interact_kept(&mut kept, view, WINDOW, iced_core::Point::ORIGIN, events);
        for message in &said {
            window.update(message.clone(), at());
        }
        said
    };
    frame(&mut window, &[]);
    let said = frame(&mut window, &testing::typing("h"));
    let drafted = said.iter().any(|each| {
        matches!(each, WindowMessage::At(_, PlaceMessage::Discussion(screens::DiscussionMessage::Draft(text))) if text == "h")
    });
    assert!(drafted, "{said:?}");
}

/// Ada in Lounge on a server that serves the search, #general not yet shown over it.
fn in_lounge_with_search() -> (Window, PlaceId) {
    let clock = Clock(Instant::now());
    let (mut window, id) = harness::chatting(&clock);
    harness::every_switch_on(&mut window, &clock, id);
    harness::in_call(&mut window, &clock, id);
    (window, id)
}

/// Docked, the bar over the pane and the content ends with the call's controls while the call shown is the one this
/// account is in, then the members toggle, which every view has; the search's field is in the middle only where the
/// server offers it.
#[test]
fn the_top_bar_ends_with_the_calls_controls_while_the_shown_call_is_ones_own_and_the_members_toggle_always() {
    let (chat, hide) = (WORDS.phrase(Phrase::ShowCallChat).to_string(), WORDS.phrase(Phrase::HideMembers).to_string());
    let calls = ["Voice options", "Audio controls", chat.as_str(), hide.as_str()];
    let window = sized(call(&["Ada", "Grace"]), WINDOW, false);
    assert_eq!(named_along_the_top(&window, WINDOW), calls, "no search served: no field");
    let last = along_the_top(&window, WINDOW).pop().expect("the members toggle").1;
    assert_eq!(WINDOW.width - (last.x + last.width), design::Space::L.px() - design::Space::Xxs.px(), "the bar's end");

    let (window, id) = in_lounge_with_search();
    let mut window = sized(window, WINDOW, false);
    let found = named_along_the_top(&window, WINDOW);
    assert!(found.first().is_some_and(|said| said.starts_with("Search")), "the field first, in the middle: {found:?}");
    assert_eq!(found[1..], calls);
    let general = screens::harness::channel_id(harness::GENERAL);
    tell(&mut window, id, PlaceMessage::Discussion(screens::DiscussionMessage::Open(general)));
    let found = named_along_the_top(&window, WINDOW);
    assert_eq!(found[1..], [hide.as_str()], "a text channel shown while the call goes on: the members toggle alone");
}

/// Where the bar is folded away, each view's header ends with the bar's controls, the search's magnifier first, and
/// the way to the channels starts it: a channel's pins come before the magnifier.
#[test]
fn a_folded_header_ends_with_the_bars_controls_in_the_order_the_canvas_has() {
    let least = Size::new(360.0, 326.0);
    let (chat, show) = (WORDS.phrase(Phrase::ShowCallChat).to_string(), WORDS.phrase(Phrase::ShowMembers).to_string());
    let (channels, pins) = (WORDS.phrase(Phrase::ShowChannelsButton).to_string(), WORDS.phrase(Phrase::PinnedMessages));
    let (window, id) = in_lounge_with_search();
    let mut window = sized(window, least, false);
    let searching = |found: &[String], at: usize| found.get(at).is_some_and(|said| said.starts_with("Search"));
    let found = named_along_the_top(&window, least);
    assert!(searching(&found, 1), "{found:?}");
    assert_eq!(
        [&found[..1], &found[2..]].concat(),
        [channels.as_str(), "Voice options", "Audio controls", chat.as_str(), show.as_str()],
        "the call: Channels, the name, then the bar's controls"
    );
    let at_the_edges = |window: &Window, what: &str| {
        let edge = design::Space::L.px() - design::Space::Xxs.px();
        let along = along_the_top(window, least);
        let (first, last) = (along.first().expect("Channels").1, along.last().expect("the members toggle").1);
        assert_eq!((first.x, least.width - (last.x + last.width)), (edge, edge), "{what}: its buttons at the edges");
    };
    at_the_edges(&window, "the call");
    let general = screens::harness::channel_id(harness::GENERAL);
    tell(&mut window, id, PlaceMessage::Discussion(screens::DiscussionMessage::Open(general)));
    let found = named_along_the_top(&window, least);
    assert!(searching(&found, 2), "{found:?}");
    assert_eq!(
        [&found[..2], &found[3..]].concat(),
        [channels.as_str(), &pins, show.as_str()],
        "a text channel: Channels, #name, pins, magnifier, members, and no call controls while the call goes on"
    );
    at_the_edges(&window, "a text channel");
    for roll in [screens::Roll::Threads, screens::Roll::Conversations] {
        tell(&mut window, id, PlaceMessage::Talk(screens::TalkMessage::Show(roll)));
        let found = named_along_the_top(&window, least);
        assert!(searching(&found, 1), "{found:?}");
        assert_eq!([&found[..1], &found[2..]].concat(), [channels.as_str(), show.as_str()], "{roll:?}");
        at_the_edges(&window, "a list");
    }
}

/// Folded, the members column opens over the content under the header whose toggle opened it, its scrim under that
/// header too: the header stays the screen's, so its toggle puts the column away, a press on its name does nothing,
/// and a press on the scrim puts the column away.
#[test]
fn folded_the_members_column_opens_under_the_header_which_stays_the_screens() {
    let least = Size::new(360.0, 326.0);
    let header = design::PANE_HEADER_HEIGHT + design::BORDER_WIDTH;
    let (window, id) = in_lounge_with_search();
    let mut window = sized(window, least, false);
    let general = screens::harness::channel_id(harness::GENERAL);
    tell(&mut window, id, PlaceMessage::Discussion(screens::DiscussionMessage::Open(general)));
    let toggle = along_the_top(&window, least).pop().expect("the members toggle").1;
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let closed = testing::render::<WindowMessage>(&theme, app::view::<Renderer>(&window, least), least, 1.0);
    window.update(WindowMessage::ToggleMembers, at());
    let room = screens::frame::Room { width: least.width, height: least.height };
    assert!(matches!(window.frame(room).members, Some(screens::frame::MembersColumn::Over(_))), "over the content");
    let open = testing::render::<WindowMessage>(&theme, app::view::<Renderer>(&window, least), least, 1.0);
    let (in_header, on_scrim) = ((2, 2), (2, 120));
    assert_eq!(open.pixel(in_header.0, in_header.1), closed.pixel(in_header.0, in_header.1), "no scrim on the header");
    assert_ne!(open.pixel(on_scrim.0, on_scrim.1), closed.pixel(on_scrim.0, on_scrim.1), "the scrim under it");
    let members = WORDS.phrase(Phrase::Members).to_string();
    let column = reachable(&window, least).into_iter().find(|(each, _)| each.label.as_str() == members);
    let column = column.expect("the members column").1;
    assert_eq!((column.y, column.y + column.height), (header, least.height), "under the header: {column:?}");
    let pressed_at = |at: iced_core::Point| -> Vec<WindowMessage> {
        let mut sim = sim_sized(app::view::<Renderer>(&window, least), least);
        sim.point_at(at);
        sim.simulate([moved(at)]);
        common::settle(&mut sim);
        sim.simulate([
            mouse(iced_core::mouse::Event::ButtonPressed(LEFT)),
            mouse(iced_core::mouse::Event::ButtonReleased(LEFT)),
        ]);
        // The channel's own reports as it is first drawn are not the press's.
        sim.drain().filter(|said| !matches!(said, WindowMessage::At(_, PlaceMessage::Discussion(_)))).collect()
    };
    let pressed = pressed_at(toggle.center());
    assert!(matches!(&pressed[..], [WindowMessage::ToggleMembers]), "the toggle, live: {pressed:?}");
    let pressed = pressed_at(iced_core::Point::new(least.width / 3.0, header / 2.0));
    assert!(pressed.is_empty(), "the header's name: nothing, the column stays: {pressed:?}");
    let pressed = pressed_at(iced_core::Point::new(on_scrim.0 as f32, on_scrim.1 as f32));
    assert!(matches!(&pressed[..], [WindowMessage::CloseMembers]), "the scrim: {pressed:?}");
}

/// The frame folds by room: the pane docks with its splitter while the content keeps its least, folds into a drawer
/// beside the rail below that, and the rail goes into the drawer too below the rail and the content's least. At every
/// size nothing a person reaches lies outside the window or is smaller than a target.
#[test]
fn the_frame_folds_by_room_and_keeps_every_control_inside_the_window() {
    use screens::frame::{DOCKS_FROM, RAIL_FROM};
    let has = |found: &[(design::Semantics, iced_core::Rectangle)], said: &str| {
        found.iter().any(|(each, _)| each.label.as_str() == said)
    };
    let splitter = WORDS.phrase(Phrase::PaneWidth).to_string();
    let channels_button = WORDS.phrase(Phrase::ShowChannelsButton).to_string();
    let sizes = [
        Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1),
        Size::new(RAIL_FROM - 1.0, 400.0),
        Size::new(RAIL_FROM, 400.0),
        Size::new(DOCKS_FROM - 1.0, 600.0),
        Size::new(DOCKS_FROM, 600.0),
        Size::new(1280.0, 800.0),
        Size::new(2560.0, 1440.0),
    ];
    for size in sizes {
        let docked = size.width >= DOCKS_FROM;
        for (kind, make) in [("listed", (|| listed(voices())) as fn() -> Window), ("call", || call(&["Ada", "Grace"]))]
        {
            for open in [false, true] {
                let window = sized(make(), size, open);
                let found = reachable(&window, size);
                let at = format!("{kind} at {size:?}, drawer asked {open}");
                for (said, bounds) in &found {
                    assert!(
                        bounds.x >= 0.0
                            && bounds.y >= 0.0
                            && bounds.x + bounds.width <= size.width + 0.5
                            && bounds.y + bounds.height <= size.height + 0.5,
                        "{at}: {} at {bounds:?} is outside the window",
                        said.label.as_str()
                    );
                    if said.role == SemanticRole::Button {
                        assert!(
                            bounds.width.min(bounds.height) >= design::SMALL_TARGET,
                            "{at}: {} at {bounds:?} is under a target",
                            said.label.as_str()
                        );
                    }
                }
                assert_eq!(has(&found, &splitter), docked, "{at}: the splitter only while docked");
                let pane_shown = docked || open;
                assert_eq!(has(&found, "Booth"), pane_shown, "{at}: the channels");
                let rail_shown = size.width >= RAIL_FROM || open;
                assert_eq!(has(&found, "Add a server"), rail_shown, "{at}: the rail");
                // A server with voice channels only has no Show channels in its panel: the header's button opens them.
                assert_eq!(has(&found, &channels_button), !docked && !open, "{at}: what opens the channels");
            }
        }
    }
}

/// A voice channel's chat marks its row as a text channel's messages mark theirs, so someone outside the call knows
/// they were written to: heard as unread, and with how many call them.
#[test]
fn a_voice_channels_row_says_its_chat_is_unread_and_how_many_call_this_account() {
    let said_of = |unread: u32, mentions: u32| {
        let mut window = listed(voices());
        let id = first(&window);
        let standing = screens::ChannelStanding { grants: proto::rules::Grants::default(), read: 0, unread, mentions };
        let lounge = screens::harness::channel_id("b");
        tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Standing { channel_id: lounge, standing }));
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        names(&mut sim).into_iter().map(|(_, said)| said).collect::<Vec<String>>()
    };
    let has = |found: &[String], said: &str| found.iter().any(|each| each == said);
    let unread = WORDS.say(&words::Sentence::ChannelUnread("Lounge".into()));
    let called = WORDS.say(&words::Sentence::ChannelMentions("Lounge".into(), 2));
    assert!(!has(&said_of(0, 0), &unread), "nothing unread, nothing said");
    assert!(has(&said_of(3, 0), &unread), "{:?}", said_of(3, 0));
    assert!(has(&said_of(3, 2), &called), "{:?}", said_of(3, 2));
}

/// A text channel open in a window too narrow for the channels beside it keeps the way to them in its header.
#[test]
fn a_text_channel_in_a_folded_window_keeps_the_way_to_the_channels() {
    use screens::frame::DOCKS_FROM;
    let button = WORDS.phrase(Phrase::ShowChannelsButton).to_string();
    for (size, folded) in [(Size::new(DOCKS_FROM - 1.0, 600.0), true), (Size::new(1280.0, 800.0), false)] {
        let clock = screens::harness::Clock(std::time::Instant::now());
        let (window, _) = screens::harness::chatting(&clock);
        let window = sized(window, size, false);
        let found = reachable(&window, size);
        let shown = found.iter().any(|(each, _)| each.label.as_str() == button);
        assert_eq!(shown, folded, "at {size:?}");
    }
}

/// A drag on the splitter says each width and is let go once; a double press resets it; the arrow keys step it.
#[test]
fn the_splitter_drags_steps_and_resets_the_pane() {
    use iced_core::{Event, Point, mouse};
    use screens::frame::PanePick;
    let size = Size::new(1280.0, 800.0);
    let window = sized(listed(voices()), size, false);
    let splitter = WORDS.phrase(Phrase::PaneWidth).to_string();
    let grip = reachable(&window, size).into_iter().find(|(each, _)| each.label.as_str() == splitter).unwrap().1;
    let from = grip.center();
    assert_eq!(from.x, design::RAIL_WIDTH + design::BORDER_WIDTH + design::PANE_LEAST + design::BORDER_WIDTH / 2.0);
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    sim.point_at(from);
    let to = Point::new(from.x + 60.0, from.y);
    sim.simulate([
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::CursorMoved { position: to }),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
    let dragged: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &dragged[..],
            [WindowMessage::Pane(PanePick::To(width)), WindowMessage::Pane(PanePick::Let)]
                if *width == design::PANE_LEAST + 60.0
        ),
        "{dragged:?}"
    );
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    sim.point_at(from);
    sim.simulate(testing::click());
    sim.simulate(testing::click());
    let twice: Vec<_> = sim.drain().collect();
    assert!(twice.iter().any(|each| matches!(each, WindowMessage::Pane(PanePick::Reset))), "{twice:?}");
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    let total = focusable_total(&mut sim);
    for _ in 0..total - 1 {
        tab(&mut sim, false);
    }
    common::press(&mut sim, Named::ArrowRight);
    common::press(&mut sim, Named::ArrowLeft);
    tab(&mut sim, false);
    common::press(&mut sim, Named::ArrowRight);
    let keyed: Vec<_> = sim.drain().collect();
    assert!(
        matches!(
            &keyed[..],
            [
                WindowMessage::Pane(PanePick::Wider),
                WindowMessage::Pane(PanePick::Narrower),
                WindowMessage::MembersWidth(PanePick::Narrower)
            ]
        ),
        "the splitter steps, then the members column's, last in Tab's order, which grows the other way: {keyed:?}"
    );
}

/// The open drawer is put away by Escape and by a press on the scrim beside it; a press in it reaches its controls.
#[test]
fn escape_and_the_scrim_put_the_drawer_away() {
    let size = Size::new(480.0, 360.0);
    let window = sized(listed(voices()), size, true);
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    common::press(&mut sim, Named::Escape);
    let escaped: Vec<_> = sim.drain().collect();
    assert!(matches!(&escaped[..], [WindowMessage::CloseChannels]), "{escaped:?}");
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    sim.point_at(iced_core::Point::new(size.width - 4.0, size.height / 2.0));
    sim.simulate(testing::click());
    let pressed: Vec<_> = sim.drain().collect();
    assert!(matches!(&pressed[..], [WindowMessage::CloseChannels]), "{pressed:?}");
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    sim.click(Plain("Booth")).unwrap();
    let joined: Vec<_> = sim.drain().collect();
    assert!(
        joined
            .iter()
            .any(|each| matches!(each, WindowMessage::At(_, PlaceMessage::Lobby(screens::LobbyMessage::Join(_))))),
        "{joined:?}"
    );
}

/// The voice page's camera card with its test refused, at a height that shows it whole, and a call's side pane with
/// Grace's camera on, its mark before her other marks, as pictures.
#[test]
fn the_camera_card_and_the_list_s_camera_mark_are_the_reference_in_both_schemes() {
    let tall = Size::new(1280.0, 1700.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let mut settings = voice_settings(false);
        settings.update(WindowMessage::TestCamera, at());
        let refused = screens::ProbeHeard::CameraTest(Err(words::Failure::CameraGone));
        settings.update(WindowMessage::Probed(refused), at());
        let image = testing::render::<WindowMessage>(&theme, app::view::<Renderer>(&settings, tall), tall, 1.0);
        ui::refs!().assert(&format!("camera-card-{scheme:?}").to_lowercase(), &image);
        let mut window = call(&["Ada", "Grace"]);
        let id = first(&window);
        let started = RoomEvent::Started { user_id: screens::harness::member("grace"), track: Track::Camera };
        tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(started)));
        ui::refs!().assert(&format!("camera-mark-{scheme:?}").to_lowercase(), &render(&theme, &window));
    }
}

/// One's own camera card, once the call has looked at it, opens a popover of how it goes out from its ⓘ.
#[test]
fn ones_own_camera_card_opens_its_popover_from_its_info_button() {
    let mut window = call(&["Ada", "Grace"]);
    let id = first(&window);
    let say = |window: &mut Window, message| tell(window, id, PlaceMessage::Call(message));
    let started = screens::CameraSent::Started { width: 1280, height: 720, encoder: screens::Route::Software };
    say(&mut window, screens::CallMessage::Camera(started));
    let on = RoomEvent::Started { user_id: screens::harness::member("ada"), track: Track::Camera };
    say(&mut window, screens::CallMessage::Room(on));
    let looked = screens::Outgoing {
        codec: "H.264".into(),
        encoder: screens::Route::Software,
        width: 1280,
        height: 720,
        scaled_from: None,
        frame_rate: Some(30),
        bits_per_second: 1_500_000,
        frames: 300,
        key_frames_made: 1,
        key_frames_asked: 0,
        resent_bytes: 0,
        restarts: 0,
        limited: false,
        reduced: None,
    };
    say(&mut window, screens::CallMessage::Camera(screens::CameraSent::Looked(looked)));
    let title = WORDS.phrase(Phrase::StatsYourCamera).to_string();
    assert!(!texts(&window, WINDOW).iter().any(|(shown, _)| *shown == title), "shut, no popover");
    let about = WORDS.phrase(Phrase::AboutPicture).to_string();
    let card = card_of(&window, "Ada", None).expect("one's own card");
    let button = card_of(&window, &about, Some(card.center())).expect("the ⓘ on one's own camera");
    let pressed: Vec<WindowMessage> = {
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        sim.point_at(card.center());
        sim.simulate([moved(card.center())]);
        common::settle(&mut sim);
        sim.point_at(button.center());
        sim.simulate([moved(button.center()), mouse(iced_core::mouse::Event::ButtonPressed(LEFT))]);
        sim.simulate([mouse(iced_core::mouse::Event::ButtonReleased(LEFT))]);
        sim.drain().collect()
    };
    assert!(asks(&pressed, &screens::CallMessage::ToggleOwnCameraInfo), "{pressed:?}");
    say(&mut window, screens::CallMessage::ToggleOwnCameraInfo);
    let found = texts(&window, WINDOW);
    assert!(found.iter().any(|(shown, _)| *shown == title), "the popover names the camera: {found:?}");
}

/// The folded frame at the least window, with its drawer shut and open, as pictures.
#[test]
fn the_folded_frame_is_the_reference_in_both_schemes() {
    let size = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for open in [false, true] {
            for (kind, window) in [("listed", listed(voices())), ("call", call(&["Ada", "Grace"]))] {
                let window = sized(window, size, open);
                let image = testing::render::<WindowMessage>(&theme, app::view::<Renderer>(&window, size), size, 1.0);
                let name = format!("frame-{kind}-{}-{scheme:?}", if open { "drawer" } else { "folded" }).to_lowercase();
                ui::refs!().assert(&name, &image);
            }
        }
    }
}

/// Every text drawn in `window` at `size`, with its box.
fn texts(window: &Window, size: Size) -> Vec<(String, iced_core::Rectangle)> {
    use iced_core::widget::operation::{Operation, text::Text};
    struct Collect(Vec<(String, iced_core::Rectangle)>);
    impl Operation for Collect {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _id: Option<&iced_core::widget::Id>, bounds: iced_core::Rectangle, text: &mut dyn Text) {
            self.0.push((text.text().to_string(), bounds));
        }
    }
    let mut sim = sim_sized(app::view::<Renderer>(window, size), size);
    let mut collect = Collect(Vec::new());
    sim.operate(&mut collect);
    collect.0
}

/// Docked, the call has no header: the stage starts under the top bar, and the name and the count are not drawn
/// again over it. Folded, its header holds the name, which keeps the room the count would take.
#[test]
fn a_call_has_no_header_docked_and_a_folded_one_names_the_channel_without_a_count() {
    use screens::frame::DOCKS_FROM;
    let header = |size: Size| {
        let mut window = listed(voices());
        let id = first(&window);
        tell(&mut window, id, PlaceMessage::Lobby(screens::LobbyMessage::Join(screens::harness::channel_id("c"))));
        let joined = RoomEvent::Joined {
            members: vec![
                Person::new(screens::harness::member("ada"), "Ada"),
                Person::new(screens::harness::member("bo"), "Bo"),
            ],
            streams: vec![
                (screens::harness::member("ada"), Track::Microphone),
                (screens::harness::member("bo"), Track::Microphone),
            ],
        };
        tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(joined)));
        tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::Room(lanes_up())));
        texts(&sized(window, size, false), size)
    };
    let docked = Size::new(DOCKS_FROM, 600.0);
    let found = header(docked);
    let past_the_pane = design::RAIL_WIDTH + design::PANE_LEAST + 2.0 * design::BORDER_WIDTH;
    assert!(!found.iter().any(|(said, _)| said == "2 people"), "no count");
    assert!(
        !found.iter().any(|(said, bounds)| said.starts_with("ΩΩΩ") && bounds.x >= past_the_pane),
        "no name over the stage: {found:?}"
    );

    let folded = header(Size::new(480.0, 360.0));
    assert!(!folded.iter().any(|(said, _)| said == "2 people"), "folded, no count");
    assert!(folded.iter().any(|(said, _)| said.starts_with("ΩΩΩ")), "folded, the name");
}

/// On a stage too short to keep a pinned card its least over the strip, the strip is put away by itself; shown there,
/// it is shown there only, and the tall stage keeps its own choice.
#[test]
fn a_short_stage_puts_the_strip_away_and_keeps_each_heights_choice() {
    // The window's height: the stage's under the top bar.
    let tall = Size::new(960.0, screens::Stage::STRIP_FROM + design::SERVER_BAR_HEIGHT + design::BORDER_WIDTH);
    let short = Size::new(960.0, tall.height - 1.0);
    let in_strip = |window: &Window, size: Size| {
        reachable(window, size)
            .iter()
            .any(|(said, bounds)| said.label.as_str().starts_with("Bo") && bounds.height == design::STRIP_TILE.1)
    };
    let window = pinned_call();
    assert!(in_strip(&sized(pinned_call(), tall, false), tall), "the strip under the pinned card");
    assert!(!in_strip(&sized(pinned_call(), short, false), short), "put away by itself");
    let mut window = window;
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleStrip(screens::Stage::Short)));
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::ToggleStrip(screens::Stage::Tall)));
    let window = sized(window, short, false);
    assert!(in_strip(&window, short), "shown while short");
    assert!(!in_strip(&window, tall), "put away while tall");
}

/// A window as a fixture makes it.
type Made = fn() -> Window;

/// Every settings page, of the account, this device and the server, at sizes where its list folds and where it does
/// not: nothing a person reaches lies outside the window or under a target; folded, the list and the page each show
/// alone, Back goes to the list and a page picked there opens.
#[test]
fn settings_fold_their_list_in_a_narrow_window_and_keep_every_control_inside_it() {
    use screens::frame::{RAIL_FROM, SETTINGS_BESIDE_FROM};
    let rail = design::RAIL_WIDTH + design::BORDER_WIDTH;
    let back = WORDS.phrase(Phrase::SettingsBack).to_string();
    let pages: [(&str, Made); 8] = [
        ("account", account_page),
        ("appearance", appearance_page),
        ("voice", || voice_settings(true)),
        ("overview", || overview_page(false, None)),
        ("channels", channels_page),
        ("invites", invites_page),
        ("members", || members_page(true)),
        ("roles", roles_list_page),
    ];
    let sizes = [
        Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1),
        Size::new(RAIL_FROM - 1.0, 400.0),
        Size::new(rail + SETTINGS_BESIDE_FROM - 1.0, 500.0),
        Size::new(rail + SETTINGS_BESIDE_FROM, 500.0),
        Size::new(1280.0, 800.0),
    ];
    for (name, make) in pages {
        for size in sizes {
            for list in [false, true] {
                let mut window = sized(make(), size, false);
                if list {
                    window.update(WindowMessage::SettingsList, at());
                }
                let found = reachable(&window, size);
                let at_size = format!("{name} at {size:?}, list asked {list}");
                for (said, bounds) in &found {
                    assert!(
                        bounds.x >= 0.0 && bounds.x + bounds.width <= size.width + 0.5,
                        "{at_size}: {} at {bounds:?} is outside the window",
                        said.label.as_str()
                    );
                    if said.role == SemanticRole::Button {
                        assert!(
                            bounds.width.min(bounds.height) >= design::SMALL_TARGET,
                            "{at_size}: {} at {bounds:?} is under a target",
                            said.label.as_str()
                        );
                    }
                }
                let beside = window
                    .settings_shown(&window.frame(screens::frame::Room { width: size.width, height: size.height }))
                    == screens::frame::SettingsShown::Beside;
                let has_back = found.iter().any(|(each, _)| each.label.as_str() == back);
                assert_eq!(has_back, !beside && !list, "{at_size}: Back only on a page shown alone");
                let has_list = found
                    .iter()
                    .any(|(each, _)| each.role == SemanticRole::RadioGroup && each.label.as_str() == "Night Owls");
                assert_eq!(has_list, beside || list, "{at_size}: the list");
            }
        }
    }
    let size = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    let window = sized(account_page(), size, false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
    sim.click(back.as_str()).unwrap();
    let pressed: Vec<_> = sim.drain().collect();
    assert!(matches!(&pressed[..], [WindowMessage::SettingsList]), "{pressed:?}");
}

/// Every screen the references draw, at the least window and the next sizes up: nothing a person reaches lies past
/// the window's sides, and in a window tall enough that nothing scrolls, nothing is under a target.
#[test]
fn every_screen_fits_the_least_window_and_keeps_its_targets() {
    use std::collections::BTreeMap;
    let least = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    let side = |found: Vec<(design::Semantics, iced_core::Rectangle)>| -> BTreeMap<String, f32> {
        let mut sides = BTreeMap::new();
        for (said, bounds) in found {
            if said.role == SemanticRole::Button {
                let least_side: &mut f32 = sides.entry(said.label.as_str().to_string()).or_insert(f32::MAX);
                *least_side = least_side.min(bounds.width.min(bounds.height));
            }
        }
        sides
    };
    let wide = Size::new(1280.0, 1600.0);
    let at_wide: Vec<_> =
        states().into_iter().map(|window| side(reachable(&sized(window, wide, false), wide))).collect();
    for size in [least, Size::new(480.0, 360.0), Size::new(640.0, 400.0)] {
        let tall = Size::new(size.width, 1600.0);
        for (at, (window, whole)) in states().into_iter().zip(states()).enumerate() {
            let window = sized(window, size, false);
            for (said, bounds) in reachable(&window, size) {
                assert!(
                    bounds.x >= -0.5 && bounds.x + bounds.width <= size.width + 0.5,
                    "state {at} at {size:?}: {} at {bounds:?} is past the window's side",
                    said.label.as_str()
                );
            }
            // A control keeps a target, or the size it has in a wide window: a narrow one never shrinks it.
            for (said, narrow) in side(reachable(&sized(whole, tall, false), tall)) {
                let roomy = at_wide[at].get(&said).copied().unwrap_or(design::SMALL_TARGET);
                assert!(
                    narrow >= design::SMALL_TARGET.min(roomy) - 0.5,
                    "state {at} at {size:?}: {said} is {narrow} px across, {roomy} in a wide window"
                );
            }
        }
    }
}

/// A call with its audio controls open.
fn audio_open() -> Window {
    let mut window = call(&["Ada", "Grace", "Bo"]);
    let id = first(&window);
    tell(&mut window, id, PlaceMessage::Call(screens::CallMessage::AudioControls));
    window
}

/// What opens over the window, at the least window and the next size up: every control inside the window, and one in
/// a dialog inside the dialog, scrolling where it has no room rather than running past an edge.
#[test]
fn what_opens_over_the_window_stays_inside_it() {
    let least = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    let opened: [(&str, Made); 6] = [
        ("audio controls", audio_open),
        ("device menu", devices_open),
        ("connection", connection_open),
        ("person menu", person_menu_open),
        ("invite dialog", inviting),
        ("add dialog", || adding(None)),
    ];
    for size in [least, Size::new(480.0, 360.0)] {
        for (name, make) in opened {
            let tree = common::access_tree(
                &mut sim_sized(app::view::<Renderer>(&sized(make(), size, false), size), size),
                size,
            );
            // Hung over the call bar, a menu keeps above it rather than cover it: what it shows, its scroll, ends there.
            if name == "device menu" {
                let bar_top = size.height
                    - (design::Space::Xl.px() - design::Space::Xs.px())
                    - (design::CALL_BUTTON + 2.0 * design::STAGE_BAR_PAD);
                let window = sized(make(), size, false);
                let mut sim = sim_sized(app::view::<Renderer>(&window, size), size);
                let mut shown = Scrolls(Vec::new());
                sim.operate(&mut shown);
                // The rail scrolls too, down to the gear at its foot, where it is shown.
                shown.0.retain(|view| view.x > 0.0 || (view.width - design::RAIL_WIDTH).abs() > f32::EPSILON);
                assert!(!shown.0.is_empty(), "{name} at {size:?}: the menu scrolls");
                for view in shown.0 {
                    assert!(
                        view.y + view.height <= bar_top,
                        "{name} at {size:?}: {view:?} covers the bar at {bar_top}"
                    );
                }
            }
            let window = accesskit::Rect::new(0.0, 0.0, f64::from(size.width), f64::from(size.height));
            for (id, node, bounds) in tree.nodes.iter().filter_map(|(id, node)| Some((id, node, node.bounds()?))) {
                // A control in a dialog keeps inside the dialog's box.
                let mut room = window;
                let mut at = *id;
                while let Some(parent) = common::parent_of(&tree, at) {
                    let (_, holder) = tree.nodes.iter().find(|(each, _)| *each == parent).unwrap();
                    if matches!(holder.role(), accesskit::Role::Dialog | accesskit::Role::AlertDialog) {
                        room = holder.bounds().unwrap();
                        break;
                    }
                    at = parent;
                }
                assert!(
                    bounds.x0 >= room.x0 - 0.5 && bounds.x1 <= room.x1 + 0.5,
                    "{name} at {size:?}: {:?} at {bounds:?} runs past {room:?}",
                    node.label()
                );
            }
        }
    }
}

/// A server's owner just arrived at it from an owner's invite: the set-up dialog asks for a name and a colour, with
/// the colour's picker open when `picking`.
fn setting_up(picking: bool) -> Window {
    let (mut window, _) = Window::launch(Vec::new(), None, Instant::now());
    let adding = window.welcome_door().unwrap().id();
    let code = "Ow9kP2".to_string() + &"a".repeat(proto::rules::TOKEN_LEN - 6);
    let door = |message| PlaceMessage::Session(screens::SessionMessage::Door(message));
    tell(&mut window, adding, door(Message::Where(format!("https://chat.example.invalid/join#{code}"))));
    tell(&mut window, adding, door(Message::Submit));
    let server = Server { ..server("chat.example.invalid", "Night Owls") };
    let found = screens::Found { server, invite: Some(screens::Invite::Works { until: 0, owner: true }) };
    tell(&mut window, adding, door(Message::Reported(Heard::Found(Ok(found)))));
    tell(&mut window, adding, door(Message::Submit));
    tell(&mut window, adding, door(Message::Username(ADA.into())));
    let password = || Secret::new("evening games with friends".into());
    tell(&mut window, adding, door(Message::Password(password())));
    tell(&mut window, adding, door(Message::Again(password())));
    tell(&mut window, adding, door(Message::Submit));
    let standing = Standing {
        place: proto::rules::Place::OWNER,
        grants: proto::rules::Grant::ALL.into_iter().collect(),
        ..screens::harness::standing_as("ada")
    };
    let admitted = screens::Admitted { token: None, named: None, standing };
    tell(&mut window, adding, door(Message::Reported(Heard::Registered(Ok(admitted)))));
    if picking {
        window
            .update(WindowMessage::Dialog(DialogMessage::SetUp(screens::SetUpMessage::Colour(Picking::Toggle))), at());
    }
    window
}

/// The set-up dialog is opened to name the server: what is typed first goes on its name, as a new channel's dialog
/// takes it.
#[test]
fn the_set_up_dialog_takes_what_is_typed_first_as_the_server_s_name() {
    let mut window = setting_up(false);
    let mut kept: Option<testing::Cache> = None;
    let mut frame = |window: &mut Window, events: &[iced_core::Event]| {
        let view = app::view::<Renderer>(window, WINDOW);
        let said = testing::interact_kept(&mut kept, view, WINDOW, iced_core::Point::ORIGIN, events);
        for message in &said {
            window.update(message.clone(), at());
        }
        said
    };
    frame(&mut window, &[]);
    let said = frame(&mut window, &testing::typing("N"));
    let named = said.iter().any(|each| {
        matches!(each, WindowMessage::Dialog(DialogMessage::SetUp(screens::SetUpMessage::Name(typed))) if typed.ends_with('N'))
    });
    assert!(named, "{said:?}");
}

/// The set-up dialog with its colour picker open, in the window the issue was seen in and in the least: the dialog
/// scrolls, so the picker's Hex field keeps its height and nothing runs past the dialog.
#[test]
fn the_set_up_dialog_with_its_picker_open_scrolls_in_a_short_window() {
    for size in [Size::new(742.0, 691.0), Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1)] {
        let window = sized(setting_up(true), size, false);
        let found = reachable(&window, size);
        let dialog = found.iter().find(|(said, _)| said.role == SemanticRole::Dialog).expect("the set-up dialog").1;
        assert!(dialog.y >= 0.0 && dialog.y + dialog.height <= size.height, "{size:?}: {dialog:?}");
        let hex = found.iter().find(|(said, _)| said.label.as_str() == "Hex").expect("the Hex field").1;
        assert_eq!(hex.height, design::CONTROL_HEIGHT, "{size:?}: Hex is whole");
        for (said, bounds) in found.iter().filter(|(said, _)| said.role != SemanticRole::Dialog) {
            assert!(
                bounds.x >= dialog.x && bounds.x + bounds.width <= dialog.x + dialog.width + 0.5,
                "{size:?}: {} at {bounds:?} runs past the dialog",
                said.label.as_str()
            );
        }
    }
}

/// The box each scrollable shows, what is in it scrolled or not.
struct Scrolls(Vec<iced_core::Rectangle>);

impl iced_core::widget::Operation for Scrolls {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        bounds: iced_core::Rectangle,
        _content: Size,
        _translation: iced_core::Vector,
        _state: &mut dyn iced_core::widget::operation::Scrollable,
    ) {
        self.0.push(bounds);
    }
}

/// Book club, opened, presented another key than the one this device trusts for it as it signed in again by itself.
fn key_changed() -> Window {
    let mut window = launched(1);
    let id = window.places()[1].id();
    tell(&mut window, id, flow(token()));
    let key = |kept: &str| proto::pin::KeyPin::parse(kept).unwrap();
    let changed = terms::LinkError::KeyChanged {
        trusted: key("b41c9e0733af5d208c6ef1a904732b5ed9c26f18a07b4e9513f6c8d47a20e3b1"),
        presented: key("7e05d2a891c43b6f0a9d5e21c7f084b32f6c9b17e4d806a5c3e9718b5d0fa264"),
    };
    tell(&mut window, id, flow(Report::Unreached(changed)));
    window.update(WindowMessage::Open(id), at());
    window
}

#[test]
fn a_changed_key_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        ui::refs!().assert(&format!("app-keychanged-{scheme:?}").to_lowercase(), &render(&theme, &key_changed()));
    }
}

/// A changed key shows both fingerprints as facts, says the server is not connected, and offers two equal ways on.
#[test]
fn a_changed_key_shows_both_fingerprints_and_offers_remove_or_trust() {
    let window = key_changed();
    let books = window.places()[1].id();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    for said in [
        "The identity of Book club has changed",
        "Previous fingerprint",
        "B41C 9E07 33AF 5D20",
        "New fingerprint",
        "7E05 D2A8 91C4 3B6F",
        "Not connected.",
        "Not connected",
    ] {
        assert!(sim.find(said).is_ok(), "{said}");
    }
    assert!(says(&names(&mut sim), SemanticRole::ListItem, "Book club, identity changed"));
    sim.click(Plain("Remove from this device")).unwrap();
    sim.click(Plain("Trust the new certificate")).unwrap();
    let said: Vec<_> = sim.drain().collect();
    assert!(
        matches!(&said[..], [
            WindowMessage::Remove(removed),
            WindowMessage::At(trusted, PlaceMessage::Session(screens::SessionMessage::TrustNewKey)),
        ] if *removed == books && *trusted == books),
        "{said:?}"
    );
}

/// Settings › About Chatkord with the design's notices read, or none in a build without a package; `open` is the
/// group folded open, if one is. A package offers updates and was found up to date; a build without one offers none.
fn about_chatkord(packaged: bool, open: Option<usize>) -> Window {
    let mut window = listed(voices()).built_as(harness::BUILD);
    if packaged {
        update_reached(&mut window, Reached::UpToDate);
    }
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), at());
    window.update(WindowMessage::NoticesRead(packaged.then(harness::notices)), at());
    if let Some(group) = open {
        window.update(WindowMessage::ToggleNotice(group), at());
    }
    window
}

/// How far an update gets in a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reached {
    Unchecked,
    Checking,
    UpToDate,
    Downloading,
    Ready,
    CheckFailed,
    DownloadFailed,
}

/// The wall clock a test's updates are told.
fn wall() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000)
}

/// `window` told its build offers updates, then as far as `reached` by the messages the host and a person send.
fn update_reached(window: &mut Window, reached: Reached) {
    let mut tell = |message| drop(window.update(WindowMessage::Update(message), at()));
    tell(UpdateMessage::Offered(wall()));
    if reached == Reached::Unchecked {
        return;
    }
    tell(UpdateMessage::Check);
    let found = match reached {
        Reached::Checking => return,
        Reached::UpToDate => UpdateFound::Nothing,
        Reached::CheckFailed => UpdateFound::Failed,
        Reached::Unchecked | Reached::Downloading | Reached::Ready | Reached::DownloadFailed => {
            UpdateFound::Newer("0.9.1".into())
        }
    };
    tell(UpdateMessage::Checked { at: wall(), found });
    match reached {
        Reached::Downloading => tell(UpdateMessage::Progress(screens::Percent::new(64))),
        Reached::Ready => tell(UpdateMessage::Placed),
        Reached::DownloadFailed => tell(UpdateMessage::NotPlaced),
        Reached::Unchecked | Reached::Checking | Reached::UpToDate | Reached::CheckFailed => {}
    }
}

/// About Chatkord with the notices read and the update as far as `reached`, in a call or not.
fn about_updates(reached: Reached, in_voice: bool) -> Window {
    let mut window = if in_voice { call(&["Ada", "Bo"]) } else { listed(voices()) }.built_as(harness::BUILD);
    update_reached(&mut window, reached);
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), at());
    window.update(WindowMessage::NoticesRead(Some(harness::notices())), at());
    window
}

/// Each of the window's servers told to have greeted with its release.
fn greeted_with(window: &mut Window, builds: &[&str]) {
    let places: Vec<PlaceId> = window.places().iter().map(screens::Place::id).collect();
    for (id, build) in places.into_iter().zip(builds) {
        let build = terms::ServerBuild::read(build).ok().flatten();
        tell(window, id, PlaceMessage::Server(screens::ServerMessage::Built(build)));
    }
}

/// About Chatkord headings the app's own version as the Client and lists each signed-in server with its release
/// under Servers; a window whose servers named none has no such card.
#[test]
fn about_chatkord_lists_each_servers_release_beside_the_clients_own() {
    let mut window = about_chatkord(true, None);
    let mut bare = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut bare);
    assert!(says(&found, SemanticRole::Heading, "Client"), "{found:?}");
    assert!(!says(&found, SemanticRole::Heading, "Servers"), "no server named a release: {found:?}");
    drop(bare);
    greeted_with(&mut window, &["0.9.0", "0.8.2"]);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Heading, "Servers"), "{found:?}");
    for shown in ["Night Owls", "0.9.0", "Book club", "0.8.2"] {
        assert!(sim.find(shown).is_ok(), "{shown}");
    }
}

/// The server's About states its release and protocol, each once its greeting told it, in the card the client's About
/// draws; Overview states neither.
#[test]
fn about_shows_the_servers_release_and_protocol_in_the_card_the_clients_about_draws() {
    let (mut window, id) = owner();
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    let mut overview = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    assert!(overview.find(Plain("Version")).is_err(), "Overview holds no release");
    drop(overview);
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(screens::Page::About)));
    let mut bare = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    assert!(bare.find(Plain("Version")).is_err(), "no release greeted, no row");
    assert!(bare.find(Plain("Protocol")).is_err(), "no protocol greeted, no row");
    drop(bare);
    let build = terms::ServerBuild::read("0.9.0").ok().flatten();
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Built(build)));
    let spoke = proto::chatkord_v1::ProtoVersion { major: 1, minor: 0 };
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Spoke(spoke)));
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    for shown in ["Version", "0.9.0", "Protocol", "1.0"] {
        assert!(sim.find(shown).is_ok(), "{shown}");
    }
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::List, "About"), "the server's card: {found:?}");
    drop(sim);
    let clients = about_chatkord(true, None);
    let mut sim = sim_sized(app::view::<Renderer>(&clients, TALL), TALL);
    for shown in ["Version", "Protocol", "Licence", "Source"] {
        assert!(sim.find(shown).is_ok(), "the client's {shown}");
    }
    let this = screens::protocol_said(proto::envelope::PROTO);
    assert!(sim.find(this.as_str()).is_ok(), "the client's own protocol {this}");
}

#[test]
fn about_chatkord_is_the_reference_closed_open_ready_and_without_a_package_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, window) in [
            ("closed", about_chatkord(true, None)),
            ("open", about_chatkord(true, Some(0))),
            ("update-ready", about_updates(Reached::Ready, false)),
            ("absent", about_chatkord(false, None)),
        ] {
            ui::refs!().assert(&format!("app-about-{name}-{scheme:?}").to_lowercase(), &render(&theme, &window));
        }
    }
}

/// How far the debug report has got in a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reported {
    /// The dialog lists the report.
    Shown,
    /// The file was saved: the dialog is gone and the page says so.
    Saved,
}

/// The file name the canvas says the report was saved as.
const SAVED_AS: &str = "chatkord-client-report-1794912120.txt";

/// `window` on About Chatkord with the client's debug report asked for, as far as `reached`.
fn reported(mut window: Window, reached: Reported) -> Window {
    window.update(WindowMessage::Machine(harness::machine()), at());
    window.update(WindowMessage::Report(screens::ReportMessage::Open), at());
    window.update(WindowMessage::Report(screens::ReportMessage::Read(screens::ReportLogs::default())), at());
    if reached == Reported::Saved {
        window.update(WindowMessage::Report(screens::ReportMessage::Download), at());
        let saved = screens::Saved::Written(SAVED_AS.into());
        window.update(WindowMessage::Report(screens::ReportMessage::Saved(saved)), at());
    }
    window
}

/// Book club signed out, its door open, and the gear pressed there: About Chatkord, over Night Owls' call meanwhile.
fn door_about() -> Window {
    let mut window = launched(1);
    let id = window.places()[1].id();
    tell(&mut window, id, flow(token()));
    tell(&mut window, id, flow(Report::Expired(None)));
    window.update(WindowMessage::Open(id), at());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), at());
    window.update(WindowMessage::NoticesRead(None), at());
    window
}

#[test]
fn the_client_debug_report_is_the_reference_shown_and_saved_and_from_the_door_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, window) in [
            ("shown", reported(about_chatkord(true, None), Reported::Shown)),
            ("saved", reported(about_chatkord(true, None), Reported::Saved)),
            ("door", reported(door_about(), Reported::Shown)),
        ] {
            ui::refs!().assert(&format!("app-about-report-{name}-{scheme:?}").to_lowercase(), &render(&theme, &window));
        }
    }
}

/// The page's Debug report row offers Download, which asks for the report; the dialog lists it whole, selectable, and
/// offers Cancel and Download; saving it says the file's name on the page.
#[test]
fn the_debug_report_row_opens_a_dialog_whose_download_saves_and_whose_page_then_names_the_file() {
    let window = about_chatkord(true, None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Heading, "Debug report"), "{found:?}");
    assert!(says(&found, SemanticRole::Status, "Contains no names, addresses or messages."), "{found:?}");
    sim.click(Plain("Download")).unwrap();
    let sent: Vec<_> = sim.drain().collect();
    assert!(matches!(&sent[..], [WindowMessage::Report(screens::ReportMessage::Open)]), "{sent:?}");
    drop(sim);

    let window = reported(window, Reported::Shown);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, "Client debug report"), "{found:?}");
    for shown in [
        "The file contains only what is listed below; a long log is listed by its newest lines. It is not sent anywhere.",
        "Chatkord debug report: client",
    ] {
        assert!(sim.find(shown).is_ok() || window.report_shown().is_some_and(|report| report.text().contains(shown)));
    }
    assert!(says(&found, SemanticRole::Button, "Cancel"), "{found:?}");
    drop(sim);

    let window = reported(about_chatkord(true, None), Reported::Saved);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(!says(&found, SemanticRole::Dialog, "Client debug report"), "saved, the dialog is gone: {found:?}");
    assert!(says(&found, SemanticRole::Status, &format!("Downloaded as {SAVED_AS}.")), "{found:?}");
}

/// How far the server's debug report has got on About.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerReached {
    /// The server offers it: the card and its Download.
    Offered,
    /// Download was pressed and the server has not answered.
    Busy,
    /// The server answered: the dialog lists the report.
    Shown,
    /// The file was saved: the dialog is gone and the card names it.
    Saved,
    /// The server refused for pace: the card says so as an alert.
    Refused,
}

/// The file name the canvas says the server's report was saved as.
const SERVER_SAVED_AS: &str = "chatkord-server-report-1794912180.txt";

/// Server settings at a server of release 0.9.0 speaking protocol 1.0 that offers its report, as its owner, on
/// Overview.
fn server_settings() -> (Window, PlaceId) {
    let (mut window, id) = owner();
    let identity = terms::ServerIdentity { name: Some("Night Owls".into()), colour: None, picture: None };
    tell(
        &mut window,
        id,
        PlaceMessage::Server(screens::ServerMessage::Greeted { identity, limits: terms::FileLimits::default() }),
    );
    let build = terms::ServerBuild::read("0.9.0").ok().flatten();
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Built(build)));
    let spoke = proto::chatkord_v1::ProtoVersion { major: 1, minor: 0 };
    tell(&mut window, id, PlaceMessage::Server(screens::ServerMessage::Spoke(spoke)));
    harness::every_switch_on(&mut window, &Clock(Instant::now()), id);
    window.update(WindowMessage::Pick(id, Pick::Settings), at());
    (window, id)
}

/// Server settings › About, the report asked for as far as `reached`.
fn server_about(reached: ServerReached) -> Window {
    let (mut window, id) = server_settings();
    tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::Settings(screens::Page::About)));
    let report = |window: &mut Window, message| {
        let message = screens::ServerAboutMessage::Report(message);
        tell(window, id, PlaceMessage::Page(screens::PageMessage::About(message)))
    };
    if reached == ServerReached::Offered {
        return window;
    }
    report(&mut window, screens::ReportMessage::Open);
    let answer = match reached {
        ServerReached::Busy => None,
        ServerReached::Refused => {
            Some(Err(terms::ReportError::Reach(terms::LinkError::Refused(proto::chatkord_v1::ErrorCode::RateLimited))))
        }
        ServerReached::Shown | ServerReached::Saved | ServerReached::Offered => {
            Some(terms::ArrivedReport::read(screens::harness::SERVER_REPORT, screens::LISTED_LOG_LINES))
        }
    };
    if let Some(answer) = answer {
        tell(&mut window, id, PlaceMessage::Page(screens::PageMessage::ServerReported(answer)));
    }
    if reached == ServerReached::Saved {
        report(&mut window, screens::ReportMessage::Download);
        report(&mut window, screens::ReportMessage::Saved(screens::Saved::Written(SERVER_SAVED_AS.into())));
    }
    window
}

#[test]
fn the_servers_debug_report_is_the_reference_offered_busy_shown_saved_and_refused_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, reached) in [
            ("about", ServerReached::Offered),
            ("about-report-busy", ServerReached::Busy),
            ("about-report", ServerReached::Shown),
            ("about-report-saved", ServerReached::Saved),
            ("about-report-refused", ServerReached::Refused),
        ] {
            let window = server_about(reached);
            ui::refs!().assert(&format!("app-server-{name}-{scheme:?}").to_lowercase(), &render(&theme, &window));
        }
        let (overview, _) = server_settings();
        ui::refs!().assert(&format!("app-server-overview-{scheme:?}").to_lowercase(), &render(&theme, &overview));
    }
}

/// About offers the server's report; Download asks, the card says it is preparing,
/// and the dialog the answer opens is the client's, titled for the server, listing the text and sending Download to it.
#[test]
fn about_offers_the_servers_report_asks_for_it_and_lists_what_it_answers_in_the_one_dialog() {
    let window = server_about(ServerReached::Offered);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "Contains no member data, messages, addresses or keys."), "{found:?}");
    sim.click(Plain("Download")).unwrap();
    let sent: Vec<_> = sim.drain().collect();
    let asked = |message: &WindowMessage| {
        matches!(
            message,
            WindowMessage::At(
                _,
                PlaceMessage::Page(screens::PageMessage::About(screens::ServerAboutMessage::Report(
                    screens::ReportMessage::Open
                )))
            )
        )
    };
    assert!(matches!(&sent[..], [message] if asked(message)), "{sent:?}");
    drop(sim);

    let window = server_about(ServerReached::Busy);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "Preparing…"), "{found:?}");
    assert!(!says(&found, SemanticRole::Button, "Download"), "the button is not offered while preparing: {found:?}");
    drop(sim);

    let window = server_about(ServerReached::Shown);
    let dialog = window.report_dialog().expect("the answer opens the dialog");
    assert_eq!((dialog.side, &*dialog.shown.text()), (screens::ReportSide::Server, screens::harness::SERVER_REPORT));
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Dialog, "Server debug report"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, "Cancel"), "{found:?}");
    drop(sim);

    let window = server_about(ServerReached::Saved);
    assert!(window.report_dialog().is_none(), "saved, the dialog is gone");
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, &format!("Downloaded as {SERVER_SAVED_AS}.")), "{found:?}");
    drop(sim);

    let window = server_about(ServerReached::Refused);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Alert, "Too many requests. Try again shortly."), "{found:?}");
    assert!(says(&found, SemanticRole::Button, "Download"), "the button is back: {found:?}");
}

/// The page's Updates card says where the update stands in a status a screen reader is told, with the one button it
/// offers, and the switch sends its message only where updates are offered.
#[test]
fn about_chatkord_says_where_the_update_stands_and_offers_its_one_button() {
    let check = WORDS.phrase(Phrase::CheckForUpdates).to_string();
    let restart = WORDS.phrase(Phrase::UpdateRestart).to_string();
    for (reached, in_voice, line, button) in [
        (Reached::Unchecked, false, "Chatkord has not checked for updates yet.", Some(&check)),
        (Reached::Checking, false, "Checking for updates…", None),
        (Reached::UpToDate, false, "Chatkord is up to date.", Some(&check)),
        (Reached::Downloading, false, "Downloading the new version.", None),
        (Reached::Ready, false, "Version 0.9.1 is ready. Restart to use it.", Some(&restart)),
        (Reached::Ready, true, "Version 0.9.1 is ready. It waits until you leave the call.", None),
        (Reached::CheckFailed, false, "Couldn't check for updates. Try again later.", Some(&check)),
        (Reached::DownloadFailed, false, "Couldn't check for updates. Try again later.", Some(&check)),
    ] {
        let window = about_updates(reached, in_voice);
        let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
        let found = names(&mut sim);
        assert!(says(&found, SemanticRole::Heading, "Updates"), "{found:?}");
        assert!(says(&found, SemanticRole::Status, line), "{reached:?}: {found:?}");
        for offered in [&check, &restart] {
            let shown = says(&found, SemanticRole::Button, offered);
            assert_eq!(shown, button == Some(offered), "{reached:?} in_voice={in_voice}: {offered}");
        }
        if let Some(button) = button {
            sim.click(button.as_str()).unwrap();
            let sent = if *button == check { UpdateMessage::Check } else { UpdateMessage::Restart };
            assert!(sim.drain().any(|message| matches!(message, WindowMessage::Update(said) if said == sent)));
        }
        let automatic = WORDS.phrase(Phrase::AutoUpdate).to_string();
        assert!(says(&found, SemanticRole::Switch { on: true }, &automatic), "{found:?}");
        press_switch(&mut sim, &automatic);
        let sent: Vec<_> = sim.drain().collect();
        assert!(matches!(&sent[..], [WindowMessage::Update(UpdateMessage::ToggleAutomatic)]), "{reached:?}: {sent:?}");
    }
    let window = about_chatkord(false, None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Status, "Updates come with the installed package."), "{found:?}");
    assert!(!says(&found, SemanticRole::Button, &check), "nothing to check");
    let automatic = WORDS.phrase(Phrase::AutoUpdate).to_string();
    assert!(says(&found, SemanticRole::Switch { on: false }, &automatic), "off where nothing updates: {found:?}");
    press_switch(&mut sim, &automatic);
    assert_eq!(sim.drain().count(), 0, "and it cannot be used");
}

/// A click on the switch named `name`.
fn press_switch<'a, M: 'a>(sim: &mut common::Sim<'a, M>, name: &str) {
    let (_, at) = common::boxes(sim)
        .into_iter()
        .find(|(each, _)| matches!(each.role, SemanticRole::Switch { .. }) && each.label.as_str() == name)
        .unwrap_or_else(|| panic!("no switch {name}"));
    sim.point_at(at.center());
    sim.simulate(testing::click());
}

/// The side pane shows the notice above the account while the new version downloads and once it is ready; its Restart
/// restarts, and in a call the notice says it waits and offers none.
#[test]
fn the_side_pane_notices_a_new_version_and_its_restart_is_never_offered_in_a_call() {
    let notice = |reached, in_voice| {
        let mut window = if in_voice { call(&["Ada", "Bo"]) } else { listed(voices()) };
        update_reached(&mut window, reached);
        window
    };
    let restart = WORDS.phrase(Phrase::UpdateRestart).to_string();
    for (reached, in_voice, said) in [
        (Reached::Unchecked, false, None),
        (Reached::UpToDate, false, None),
        (Reached::CheckFailed, false, None),
        (Reached::Downloading, false, Some(Phrase::UpdateDownloading)),
        (Reached::Ready, false, Some(Phrase::UpdateReady)),
        (Reached::Ready, true, Some(Phrase::UpdateWaitsForCall)),
        (Reached::DownloadFailed, false, Some(Phrase::UpdateFailed)),
    ] {
        let window = notice(reached, in_voice);
        let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
        let found = names(&mut sim);
        let statuses: Vec<_> = found.iter().filter(|(role, _)| *role == SemanticRole::Status).collect();
        let shown = said.map(|said| WORDS.phrase(said).to_string());
        assert_eq!(
            statuses.iter().find(|(_, label)| label.contains("version") || label.contains("update")).map(|(_, l)| l),
            shown.as_ref(),
            "{reached:?} in_voice={in_voice}"
        );
        let offered = reached == Reached::Ready && !in_voice;
        assert_eq!(says(&found, SemanticRole::Button, &restart), offered, "{reached:?} in_voice={in_voice}");
        if offered {
            sim.click(restart.as_str()).unwrap();
            let sent: Vec<_> = sim.drain().collect();
            assert!(matches!(&sent[..], [WindowMessage::Update(UpdateMessage::Restart)]), "{sent:?}");
            let todo = notice(reached, in_voice).update(WindowMessage::Update(UpdateMessage::Restart), at());
            assert!(matches!(&todo[..], [Ask::Update(screens::UpdateAsk::Restart)]), "{todo:?}");
        }
    }
}

#[test]
fn the_side_panes_update_notice_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let mut window = listed(voices());
        update_reached(&mut window, Reached::Ready);
        ui::refs!().assert(&format!("app-update-{scheme:?}").to_lowercase(), &render(&theme, &window));
    }
}

/// The page says the running build's version, licence and source, lists each group by its name, and a press on one
/// folds it open; the settings list names the page last among this device's.
#[test]
fn about_chatkord_says_this_build_and_folds_a_group_open_on_a_press() {
    let mut window = listed(voices());
    window.update(WindowMessage::OpenDevice(screens::DevicePage::AboutChatkord), at());
    window.update(WindowMessage::NoticesRead(Some(harness::notices())), at());
    let mut sim = sim_sized(app::view::<Renderer>(&window, TALL), TALL);
    let found = names(&mut sim);
    assert!(says(&found, SemanticRole::Radio { chosen: true }, "About"), "{found:?}");
    assert!(says(&found, SemanticRole::Heading, "Built with"), "{found:?}");
    assert!(says(&found, SemanticRole::Button, "FFmpeg Decodes and encodes video GPL-2.0-or-later"), "{found:?}");
    let this = screens::Build::THIS;
    assert_eq!(this.version, terms::BUILD);
    assert_eq!(this.licence, "GPL-3.0-or-later", "the client's manifest names its licence");
    for said in ["0.9.0", this.licence, "tag v0.9.0"] {
        assert!(sim.find(said).is_ok(), "the package's {said} is shown");
    }
    let _ = sim.click(Plain("x264")).expect("the group's name is pressed");
    let pressed: Vec<_> = sim.drain().collect();
    assert!(pressed.iter().any(|m| matches!(m, WindowMessage::ToggleNotice(1))), "{pressed:?}");
    let window = about_chatkord(false, None);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    assert!(sim.find(Plain("The notices come with the installed package.")).is_ok());
    assert!(sim.find(Plain("a local build")).is_ok(), "a build with no package names no tag");
}

/// The node of `tree` with `role` named `name`.
fn tree_node<'t>(
    tree: &'t accesskit::TreeUpdate,
    role: accesskit::Role,
    name: &str,
) -> &'t (accesskit::NodeId, accesskit::Node) {
    tree.nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(name))
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"))
}

/// Whether `inner` is somewhere under `outer`.
fn under(tree: &accesskit::TreeUpdate, inner: accesskit::NodeId, outer: accesskit::NodeId) -> bool {
    std::iter::successors(common::parent_of(tree, inner), |at| common::parent_of(tree, *at)).any(|at| at == outer)
}

fn window_tree(window: &Window) -> accesskit::TreeUpdate {
    let mut sim = sim_sized(app::view::<Renderer>(window, WINDOW), WINDOW);
    common::settle(&mut sim);
    let tree = common::access_tree(&mut sim, WINDOW);
    common::well_formed(&tree);
    tree
}

/// The arrow beside mute says whether the menu of microphones and speakers is open, keeps its id as it opens, and the
/// focus goes into the menu, which is read over the call.
#[test]
fn the_device_menus_arrow_says_it_is_open_and_the_focus_is_in_the_menu() {
    let devices = WORDS.phrase(Phrase::DeviceMenu).to_string();
    let closed = window_tree(&call(&["Ada Lovelace", "Grace Hopper"]));
    let (arrow, node) = tree_node(&closed, accesskit::Role::Button, &devices);
    assert_eq!((node.has_popup(), node.is_expanded()), (Some(accesskit::HasPopup::Menu), Some(false)));
    let open = window_tree(&devices_open());
    let (opened, node) = tree_node(&open, accesskit::Role::Button, &devices);
    assert_eq!(node.is_expanded(), Some(true));
    assert_eq!(opened, arrow, "the same control, open");
    let (menu, _) = tree_node(&open, accesskit::Role::Menu, &devices);
    let (_, focused) = open.nodes.iter().find(|(id, _)| *id == open.focus).unwrap();
    assert_eq!(focused.role(), accesskit::Role::MenuItemRadio);
    assert!(under(&open, open.focus, *menu), "the focus is on an item of the menu");
    assert!(!under(&open, *arrow, *menu), "the call under the menu is read as it is");
}

/// The connection bars say whether the popover they open is showing.
#[test]
fn the_connection_bars_say_whether_their_popover_is_open() {
    let connection = WORDS.phrase(Phrase::StatsTitle).to_string();
    for (window, open) in [(call(&["Ada Lovelace", "Grace Hopper"]), false), (connection_open(), true)] {
        let tree = window_tree(&window);
        let (_, bars) = tree_node(&tree, accesskit::Role::Button, &connection);
        assert_eq!((bars.has_popup(), bars.is_expanded()), (Some(accesskit::HasPopup::Dialog), Some(open)));
    }
}

/// A settings page is read as what it draws: every control it names is a node of that role and name where it is drawn,
/// and the tree is the same from one turn to the next.
#[test]
fn a_settings_page_is_read_as_it_is_drawn_and_the_same_each_turn() {
    let window = voice_settings(false);
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let drawn = common::boxes(&mut sim);
    let tree = window_tree(&window);
    assert!(drawn.len() > 10, "a page of controls: {drawn:?}");
    for (said, at) in &drawn {
        let at = accesskit::Rect::new(
            f64::from(at.x),
            f64::from(at.y),
            f64::from(at.x + at.width),
            f64::from(at.y + at.height),
        );
        assert!(
            tree.nodes.iter().any(|(_, node)| node.label() == Some(said.label.as_str()) && node.bounds() == Some(at)),
            "{said:?} at {at:?}"
        );
    }
    for role in [accesskit::Role::Heading, accesskit::Role::RadioGroup, accesskit::Role::Slider] {
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == role), "the page has a {role:?}");
    }
    let ids = |tree: &accesskit::TreeUpdate| tree.nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(ids(&tree), ids(&window_tree(&window)));
}

/// A person's popover the pointer opened stands beside the control it pressed, whatever had the focus before: a press
/// on a card focuses it, and a press on a seat under the voice channel then opened Grace's popover beside the card.
#[test]
fn a_seats_popover_stands_beside_the_seat_though_a_card_had_the_focus() {
    let named = |sim: &mut common::Sim<'_, WindowMessage>, role: fn(&SemanticRole) -> bool, who: &str| {
        common::boxes(sim)
            .into_iter()
            .filter(|(said, _)| role(&said.role) && said.label.as_str().to_lowercase().starts_with(who))
            .map(|(_, bounds)| bounds)
            .collect::<Vec<_>>()
    };
    // Each step's window is the call told everything pressed so far, since the simulator borrows the one it shows.
    let after = |pressed: &[WindowMessage]| {
        let mut window = call(&["Ada", "Grace"]);
        for message in pressed {
            window.update(message.clone(), at());
        }
        window
    };
    let start = after(&[]);
    let mut sim = sim_sized(app::view::<Renderer>(&start, WINDOW), WINDOW);
    common::settle(&mut sim);
    let card = named(&mut sim, |_| true, "grace").into_iter().max_by(|a, b| a.height.total_cmp(&b.height));
    sim.point_at(card.expect("Grace's card").center());
    sim.simulate(testing::click());
    let mut pressed: Vec<WindowMessage> = sim.drain().collect();
    let carded = after(&pressed);
    let mut sim = sim.rebuild(app::view::<Renderer>(&carded, WINDOW));
    common::settle(&mut sim);
    let seat =
        named(&mut sim, |role| *role == SemanticRole::ListItem, "grace").into_iter().min_by(|a, b| a.x.total_cmp(&b.x));
    let seat = seat.expect("Grace's seat, under the voice channel");
    sim.point_at(seat.center());
    sim.simulate(testing::click());
    pressed.extend(sim.drain());
    let seated = after(&pressed);
    let mut sim = sim.rebuild(app::view::<Renderer>(&seated, WINDOW));
    common::settle(&mut sim);
    assert!(seated.person().is_some_and(|card| card.open.from == screens::PersonFrom::Seat), "the seat opened it");
    let popover = named(&mut sim, |role| *role == SemanticRole::Dialog, "grace").pop().expect("the popover");
    assert!(popover.x >= seat.x + seat.width, "to the right of the seat {seat:?}: {popover:?}");
    assert!((popover.y - seat.y).abs() < seat.height, "level with the seat {seat:?}: {popover:?}");
}
