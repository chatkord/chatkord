//! Getting in: every step as a picture in both schemes, and what the keyboard and the clipboard do to the door.

use crate::common;

use common::{Plain, press, semantics, sim_sized};
use design::{Accent, Scheme, SemanticRole};
use iced_core::keyboard::key::Named;
use iced_core::widget::Id;
use iced_core::widget::operation::{Operation, Scrollable};
use iced_core::{Event, Point, Rectangle, Size, Vector, clipboard};
use proto::chatkord_v1::ErrorCode;
use screens::{Door, Found, Heard, Invite, Message, Opening, Refusal, Report, Server, Standing};
use std::sync::Arc;
use terms::{LinkError, Secret, ServerUrl};
use ui::door::Foot;
use ui::testing::{self, Image, Renderer};
use ui::{Theme, clipboard_reader};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const WINDOW: Size = Size::new(1280.0, 768.0);
const PASSWORD: &str = "evening games with friends";

fn book_club() -> Server {
    Server {
        url: ServerUrl::parse("wss://books.example.invalid/v1").unwrap(),
        name: Some("Book club".into()),
        colour: None,
        picture: None,
    }
}

fn link() -> String {
    format!("https://books.example.invalid/join#{}", "R".repeat(proto::rules::TOKEN_LEN))
}

fn told(door: &mut Door, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        door.update(message);
    }
}

fn previewed(invite: Option<Invite>) -> Door {
    let mut door = Door::default();
    let text = if invite.is_some() { link() } else { "books.example.invalid".into() };
    let found = Found { server: book_club(), invite };
    told(&mut door, [Message::Where(text), Message::Submit, Message::Reported(Heard::Found(Ok(found)))]);
    door
}

fn creating(name: &str, password: &str, again: &str) -> Door {
    let mut door = previewed(Some(Invite::Works { until: 1_793_491_200, owner: false }));
    told(
        &mut door,
        [
            Message::Submit,
            Message::Username(name.into()),
            Message::Password(Secret::new(password.into())),
            Message::Again(Secret::new(again.into())),
        ],
    );
    door
}

/// A server on this machine reached without encryption: its card says so.
fn plain() -> Door {
    let mut door = Door::default();
    let server =
        Server { url: ServerUrl::parse("ws://127.0.0.1:8443/v1").unwrap(), name: None, colour: None, picture: None };
    let found = Found { server, invite: None };
    told(
        &mut door,
        [Message::Where("127.0.0.1:8443".into()), Message::Submit, Message::Reported(Heard::Found(Ok(found)))],
    );
    door
}

/// Every step, in the order a person meets them.
fn steps() -> Vec<(&'static str, Door)> {
    let mut typo = Door::default();
    told(
        &mut typo,
        [
            Message::Where("boks.example.invalid".into()),
            Message::Submit,
            Message::Reported(Heard::Found(Err(LinkError::NoSuchHost))),
        ],
    );
    let mut untrusted = Door::default();
    told(
        &mut untrusted,
        [
            Message::Where("lan.example.invalid".into()),
            Message::Submit,
            Message::Reported(Heard::Found(Err(LinkError::Untrusted))),
        ],
    );
    let mut taken = creating("Ada", PASSWORD, PASSWORD);
    let refused = LinkError::Refused(ErrorCode::AlreadyExists);
    told(&mut taken, [Message::Submit, Message::Reported(Heard::Registered(Err(refused)))]);
    let mut wrong = previewed(None);
    let refusal = Refusal { code: ErrorCode::Unauthenticated };
    told(
        &mut wrong,
        [
            Message::Submit,
            Message::Username("Ada".into()),
            Message::Password(Secret::new("not it".into())),
            Message::Submit,
            Message::Reported(Heard::Flow(Report::Refused(refusal))),
            Message::Forgot,
        ],
    );
    let mut working = creating("Ada", PASSWORD, PASSWORD);
    working.update(Message::Submit);
    let mut new_password = Door::opening(Opening::Server(book_club()));
    let standing = Standing { must_change: true, ..screens::harness::standing_as("ada") };
    told(
        &mut new_password,
        [
            Message::Username("Ada".into()),
            Message::Password(Secret::new("river-copper-lamp-42".into())),
            Message::Submit,
            Message::Reported(Heard::Flow(Report::Accepted { token: None, named: None, standing })),
        ],
    );
    let mut opening = Door::opening(Opening::Account(book_club(), "Ada".into()));
    opening.update(Message::Begin);
    let mut old = Door::default();
    told(
        &mut old,
        [
            Message::Where("old.example.invalid".into()),
            Message::Submit,
            Message::Reported(Heard::Found(Err(LinkError::OldEncryption))),
        ],
    );
    vec![
        ("add", Door::default()),
        ("add-typo", typo),
        ("add-untrusted", untrusted),
        ("add-old-encryption", old),
        ("preview-trust", unvouched()),
        ("preview-invite", previewed(Some(Invite::Works { until: 1_793_491_200, owner: false }))),
        ("preview-owner", previewed(Some(Invite::Works { until: 1_793_491_200, owner: true }))),
        ("preview-refused", previewed(Some(Invite::Refused))),
        ("preview-address", previewed(None)),
        ("preview-plain", plain()),
        ("create", creating("Ada", PASSWORD, PASSWORD)),
        ("create-problems", creating("Ada Lovelace", "evening games", "evening game")),
        ("create-taken", taken),
        ("sign-in-wrong", wrong),
        ("working", working),
        ("new-password", new_password),
        ("opening", opening),
    ]
}

/// The canvas's fingerprint: B41C 9E07 … E3B1.
fn first_key() -> proto::pin::KeyPin {
    proto::pin::KeyPin::parse("b41c9e0733af5d208c6ef1a904732b5ed9c26f18a07b4e9513f6c8d47a20e3b1").unwrap()
}

/// An address whose server presents a certificate no authority signed: its key, to trust or not.
fn unvouched() -> Door {
    let mut door = Door::default();
    let found = Heard::Found(Err(LinkError::Unvouched(first_key())));
    told(&mut door, [Message::Where("lan.example.invalid".into()), Message::Submit, Message::Reported(found)]);
    door
}

fn render(theme: &Theme, door: &Door) -> Image {
    testing::render::<Message>(
        theme,
        ui::door::view::<Renderer>(door, WINDOW.width, Foot::Ask, None, &WORDS),
        WINDOW,
        1.0,
    )
}

#[test]
fn every_step_is_the_reference_in_both_schemes() {
    let steps = steps();
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, door) in &steps {
            let word = match scheme {
                Scheme::Dark => "dark",
                Scheme::Light => "light",
            };
            ui::refs!().assert(&format!("door-{name}-{word}"), &render(&theme, door));
        }
    }
}

/// A server whose certificate is not valid says so in one sentence: there is no way to go on anyway.
#[test]
fn a_server_this_device_cannot_check_says_so_and_offers_no_way_past() {
    let (_, door) = steps().into_iter().find(|(name, _)| *name == "add-untrusted").unwrap();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    assert!(sim.find(Plain("This server’s certificate is not valid, so Chatkord will not connect to it.")).is_ok());
    for way_past in ["Trust and continue", "Connect anyway"] {
        assert!(sim.find(way_past).is_err(), "{way_past}");
    }
}

#[test]
fn continue_does_nothing_until_something_is_typed_and_enter_goes_on() {
    let door = Door::default();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    assert!(sim.click(Plain("Continue")).is_ok());
    assert!(sim.into_messages().next().is_none(), "an empty field offers nothing");
    let mut door = Door::default();
    door.update(Message::Where("books.example.invalid".into()));
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    sim.click(Plain("Continue")).unwrap();
    assert!(matches!(sim.into_messages().collect::<Vec<_>>()[..], [Message::Submit]));
}

#[test]
fn enter_presses_the_main_button_on_a_step_with_no_field() {
    let door = previewed(Some(Invite::Works { until: 1_793_491_200, owner: false }));
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    press(&mut sim, Named::Enter);
    assert!(matches!(sim.into_messages().collect::<Vec<_>>()[..], [Message::Submit]));
    let refused = previewed(Some(Invite::Refused));
    let mut sim = sim_sized(ui::door::view::<Renderer>(&refused, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    press(&mut sim, Named::Enter);
    assert!(sim.into_messages().next().is_none(), "no Continue to press");
}

/// The foot offers the client's debug report beside the form, and under it at the least window, and saying what became
/// of the report once it did.
#[test]
fn the_foot_offers_the_debug_report_beside_the_form_and_under_it_at_the_least_window() {
    let door = Door::default();
    for width in [WINDOW.width, design::WINDOW_LEAST.0] {
        let size = Size::new(width, WINDOW.height);
        let mut sim = sim_sized(ui::door::view::<Renderer>(&door, width, Foot::Ask, None, &WORDS), size);
        assert!(sim.find(Plain("Something not working?")).is_ok(), "{width}");
        sim.click(Plain("Download a debug report")).unwrap();
        assert!(matches!(sim.into_messages().collect::<Vec<_>>()[..], [Message::DebugReport]), "{width}");
    }
    let saved = Foot::Downloaded("chatkord-client-report-1794912120.txt");
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, saved, None, &WORDS), WINDOW);
    assert!(sim.find(Plain("Downloaded as chatkord-client-report-1794912120.txt.")).is_ok());
    assert!(sim.find(Plain("Download a debug report")).is_err(), "the offer is replaced by what became of it");
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Refused, None, &WORDS), WINDOW);
    assert!(sim.find(Plain("Couldn't make the report.")).is_ok());
}

/// A step whose work is typing puts the first thing typed in its first field, as a sign-in form does everywhere.
#[test]
fn a_step_to_type_in_takes_what_is_typed_first_in_its_first_field() {
    let mut create = previewed(Some(Invite::Works { until: 1_793_491_200, owner: false }));
    told(&mut create, [Message::Submit]);
    let mut sign_in = previewed(None);
    told(&mut sign_in, [Message::Submit]);
    let steps = [(Door::default(), "add"), (create, "create"), (sign_in, "sign in")];
    for (door, step) in &steps {
        let mut sim = sim_sized(ui::door::view::<Renderer>(door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
        sim.typewrite("b");
        let said: Vec<_> = sim.into_messages().collect();
        let typed = said.iter().any(|message| match message {
            Message::Where(text) | Message::Username(text) => text == "b",
            _ => false,
        });
        assert!(typed, "{step}: {said:?}");
    }
}

#[test]
fn escape_goes_back_one_step() {
    let door = previewed(None);
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    press(&mut sim, Named::Escape);
    assert!(matches!(sim.into_messages().collect::<Vec<_>>()[..], [Message::Back]));
}

#[test]
fn paste_asks_for_the_clipboard_and_the_reader_hands_its_text_on() {
    let door = Door::default();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    sim.click(Plain("Paste")).unwrap();
    assert!(matches!(sim.into_messages().collect::<Vec<_>>()[..], [Message::Paste]));
    let read = Event::Clipboard(clipboard::Event::Read(Ok(Arc::new(clipboard::Content::Text(link())))));
    let redraw = Event::Window(iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()));
    let seen = testing::interact(
        clipboard_reader(|text| Message::Reported(Heard::Pasted(text))),
        WINDOW,
        Point::ORIGIN,
        &[redraw, read],
    );
    assert!(matches!(&seen.messages[..], [Message::Reported(Heard::Pasted(text))] if *text == link()));
}

#[test]
fn a_form_taller_than_its_window_scrolls_instead_of_being_cut() {
    let door = creating("Ada Lovelace", "evening games", "evening game");
    let short = Size::new(800.0, 360.0);
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, short.width, Foot::Ask, None, &WORDS), short);
    let named = semantics(&mut sim);
    assert!(named.iter().any(|each| each.role == SemanticRole::Button && each.label.as_str() == "Sign in"));
    struct Overflow(bool);
    impl Operation for Overflow {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(&mut self, _: Option<&Id>, bounds: Rectangle, content: Size, _: Vector, _: &mut dyn Scrollable) {
            self.0 |= content.height > bounds.height;
        }
    }
    let mut overflow = Overflow(false);
    sim.operate(&mut overflow);
    assert!(overflow.0, "the form is held in a scrollable taller inside than out");
    let picture = testing::render::<Message>(
        &Theme::new(Scheme::Dark, Accent::DEFAULT),
        ui::door::view::<Renderer>(&door, short.width, Foot::Ask, None, &WORDS),
        short,
        1.0,
    );
    ui::refs!().assert("door-short-dark", &picture);
}

/// Where the text and the controls of a door were laid out against the window's sides.
struct Outside {
    width: f32,
    beyond: Vec<String>,
    said: Vec<String>,
}
impl Operation for Outside {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn iced_core::widget::operation::Text) {
        self.said.push(state.text().to_string());
        if bounds.x < 0.0 || bounds.x + bounds.width > self.width {
            self.beyond.push(state.text().to_string());
        }
    }
    fn custom(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        if let Some(semantics) = state.downcast_ref::<design::Semantics>()
            && (bounds.x < 0.0 || bounds.x + bounds.width > self.width)
        {
            self.beyond.push(semantics.label.as_str().to_string());
        }
    }
}

#[test]
fn every_step_fits_the_least_window_and_the_first_one_on_windows() {
    const LEAST: Size = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    // The first size Windows gave the window at 150 %.
    const FIRST_ON_WINDOWS: Size = Size::new(548.0, 437.0);
    for size in [LEAST, FIRST_ON_WINDOWS] {
        for (name, door) in &steps() {
            let mut sim = sim_sized(ui::door::view::<Renderer>(door, size.width, Foot::Ask, None, &WORDS), size);
            let mut outside = Outside { width: size.width, beyond: Vec::new(), said: Vec::new() };
            sim.operate(&mut outside);
            assert!(outside.beyond.is_empty(), "{name} at {size:?}: beyond the window: {:?}", outside.beyond);
            // The side beside the form is wider than these windows, so it folds away rather than squeeze the form.
            let tagline = WORDS.phrase(words::Phrase::Tagline);
            assert!(!outside.said.iter().any(|said| **said == *tagline), "{name} at {size:?}: the side is drawn");
        }
    }
    let door = previewed(Some(Invite::Works { until: 1_793_491_200, owner: false }));
    let picture = testing::render::<Message>(
        &Theme::new(Scheme::Dark, Accent::DEFAULT),
        ui::door::view::<Renderer>(&door, LEAST.width, Foot::Ask, None, &WORDS),
        LEAST,
        1.0,
    );
    ui::refs!().assert("door-preview-narrow-dark", &picture);
}

/// The first-use prompt names the server by its address, shows its key in four rows, and trusts only on its own
/// button: Enter does nothing and Escape goes back.
#[test]
fn a_self_signed_server_shows_its_fingerprint_and_is_trusted_only_by_its_button() {
    let door = unvouched();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    for said in [
        "lan.example.invalid",
        "This server uses a self-signed certificate.",
        "Chatkord cannot verify who operates it. Continue only if you know and trust the server’s administrator.",
        "Fingerprint",
        "B41C 9E07 33AF 5D20",
        "13F6 C8D4 7A20 E3B1",
        "You’ll sign in with your account there.",
    ] {
        assert!(sim.find(said).is_ok(), "{said}");
    }
    press(&mut sim, Named::Enter);
    assert!(sim.drain().next().is_none(), "Enter trusts nothing");
    press(&mut sim, Named::Escape);
    sim.click(Plain("Trust this server")).unwrap();
    let said: Vec<_> = sim.into_messages().collect();
    assert!(matches!(said[..], [Message::Back, Message::Trust]), "{said:?}");
}

/// A server that speaks only an older TLS says so in one sentence.
#[test]
fn a_server_speaking_an_older_tls_says_it_is_not_connected_to() {
    let (_, door) = steps().into_iter().find(|(name, _)| *name == "add-old-encryption").unwrap();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, None, &WORDS), WINDOW);
    assert!(
        sim.find(Plain("This server uses an outdated version of TLS, so Chatkord will not connect to it.")).is_ok()
    );
}

/// The window the first run opens on: no server added, so the door offers the language.
fn first_run() -> screens::Window {
    screens::Window::launch(Vec::new(), None, std::time::Instant::now()).0
}

#[test]
fn the_first_run_door_offers_the_language_on_every_step_inside_the_least_window() {
    let window = first_run();
    const LEAST: Size = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    for (name, door) in &steps() {
        let mut sim = sim_sized(ui::door::view::<Renderer>(door, LEAST.width, Foot::Ask, Some(&window), &WORDS), LEAST);
        let named = semantics(&mut sim);
        assert!(
            named
                .iter()
                .any(|each| matches!(each.role, SemanticRole::Opens { .. })
                    && each.label.as_str().starts_with("Language")),
            "{name}: the Language field"
        );
        assert!(named.iter().any(|each| each.label.as_str() == "Download a debug report"), "{name}: the report link");
        let mut outside = Outside { width: LEAST.width, beyond: Vec::new(), said: Vec::new() };
        sim.operate(&mut outside);
        assert!(outside.beyond.is_empty(), "{name}: beyond the window: {:?}", outside.beyond);
    }
}

#[test]
fn the_language_field_on_the_door_sends_the_windows_own_language_messages() {
    let window = first_run();
    let door = Door::default();
    let mut sim = sim_sized(ui::door::view::<Renderer>(&door, WINDOW.width, Foot::Ask, Some(&window), &WORDS), WINDOW);
    let boxes = common::boxes(&mut sim);
    let field = boxes
        .iter()
        .find(|(each, _)| each.label.as_str() == "Language Match system (English)")
        .map(|(_, at)| at.center())
        .unwrap_or_else(|| panic!("the field: {boxes:?}"));
    sim.point_at(field);
    sim.simulate(testing::click());
    assert!(matches!(sim.drain().collect::<Vec<_>>()[..], [Message::Language(screens::LanguageSaid::Toggle)]));
}

#[test]
fn the_door_with_its_language_menu_open_is_the_reference_in_both_schemes() {
    let mut window = first_run();
    window.update(screens::WindowMessage::LanguageMenu, screens::harness::Clock(std::time::Instant::now()).at());
    assert!(window.language_menu_open());
    let shown = Door::default();
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let door = ui::door::view::<Renderer>(&shown, WINDOW.width, Foot::Ask, Some(&window), &WORDS);
        let word = match scheme {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        };
        ui::refs!()
            .assert(&format!("door-language-open-{word}"), &testing::render::<Message>(&theme, door, WINDOW, 1.0));
    }
}
