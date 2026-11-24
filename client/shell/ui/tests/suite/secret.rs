//! A secret field against a stock iced `text_input().secure(true)`: each protection shows the stock field doing the
//! thing, so an iced that stops is noticed, and the secret field not doing it.

use crate::common;

use common::{label, sim};
use iced_core::keyboard::Modifiers;
use iced_core::keyboard::key::Named;
use iced_core::{Element, InputMethod, Point, Size, Widget};
use iced_widget::{container, text_input};
use terms::Secret;
use ui::testing::{self, Renderer, click, command, typing};
use ui::{SecretInput, Theme};

#[derive(Clone, Debug, PartialEq)]
enum Typed {
    Plain(String),
    Secret(Secret),
}

const PASSWORD: &str = "hunter2";
const FIELD: Size = Size::new(240.0, 40.0);
const OVER_FIELD: Point = Point::new(120.0, 20.0);

fn stock<'a>() -> Element<'a, Typed, Theme, Renderer> {
    text_input::TextInput::new("password", "").secure(true).on_input(Typed::Plain)._boxed()
}

fn secret<'a>(value: &'a Secret) -> Element<'a, Typed, Theme, Renderer> {
    SecretInput::new(label("Password"), value).placeholder("password").on_input(Typed::Secret).view()
}

fn typed_into(element: Element<'_, Typed, Theme, Renderer>, more: &[iced_core::Event]) -> testing::Observed<Typed> {
    let events: Vec<_> = click().into_iter().chain(typing(PASSWORD)).chain(more.iter().cloned()).collect();
    testing::interact(container(element), FIELD, OVER_FIELD, &events)
}

/// What is typed reaches the app, as a secret, whole and in order: ASCII, Greek, an accent.
#[test]
fn what_is_typed_reaches_the_app_as_a_secret() {
    let empty = Secret::new(String::new());
    for password in [PASSWORD, "Καλημέρα", "café"] {
        let events: Vec<_> = click().into_iter().chain(typing(password)).collect();
        let seen = testing::interact(container(secret(&empty)), FIELD, OVER_FIELD, &events);
        assert_eq!(seen.messages.last(), Some(&Typed::Secret(Secret::new(password.to_string()))), "{password}");
        assert_eq!(seen.messages.len(), password.chars().count(), "one message per character");
    }
}

/// What the clipboard gives goes into the buffer at the caret, and what is typed after it follows.
#[test]
fn a_paste_goes_into_the_buffer_and_what_is_typed_after_it_follows() {
    let empty = Secret::new(String::new());
    let pasted = iced_core::Event::Clipboard(iced_core::clipboard::Event::Read(Ok(std::sync::Arc::new(
        iced_core::clipboard::Content::Text("river-copper\nsecond line".to_string()),
    ))));
    let events: Vec<_> = click().into_iter().chain([pasted]).chain(typing("-42")).collect();
    let seen = testing::interact(container(secret(&empty)), FIELD, OVER_FIELD, &events);
    assert_eq!(
        seen.messages.first(),
        Some(&Typed::Secret(Secret::new("river-copper".to_string()))),
        "the first line, as one message"
    );
    assert_eq!(seen.messages.last(), Some(&Typed::Secret(Secret::new("river-copper-42".to_string()))));
}

/// A key the field does not take is left to the control that does: Tab is not swallowed.
#[test]
fn the_field_does_not_take_keys_that_are_not_its_own() {
    let empty = Secret::new(String::new());
    let tab = iced_core::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
        key: iced_core::keyboard::Key::Named(Named::Tab),
        modified_key: iced_core::keyboard::Key::Named(Named::Tab),
        physical_key: iced_core::keyboard::key::Physical::Unidentified(
            iced_core::keyboard::key::NativeCode::Unidentified,
        ),
        location: iced_core::keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let events: Vec<_> = click().into_iter().chain([tab]).collect();
    let seen = testing::interact(container(secret(&empty)), FIELD, OVER_FIELD, &events);
    assert!(seen.messages.is_empty(), "{:?}", seen.messages);
}

/// A stock secure field gives the real text to anything that walks the tree; ours does not.
#[test]
fn a_selector_finds_the_typed_text_in_a_stock_field_and_not_in_ours() {
    let mut stock_sim = sim(container(stock()));
    stock_sim.point_at(OVER_FIELD);
    stock_sim.simulate(click());
    stock_sim.typewrite(PASSWORD);
    assert_eq!(stock_sim.drain().count(), PASSWORD.len(), "the text was typed");
    assert!(
        stock_sim.find(PASSWORD).is_ok(),
        "the stock field exposes its text: if iced stopped, drop this protection"
    );

    let empty = Secret::new(String::new());
    let mut secret_sim = sim(container(secret(&empty)));
    secret_sim.point_at(OVER_FIELD);
    secret_sim.simulate(click());
    secret_sim.typewrite(PASSWORD);
    assert_eq!(secret_sim.drain().count(), PASSWORD.len(), "the text was typed");
    assert!(secret_sim.find(PASSWORD).is_err(), "the secret field exposes no text to an operation");
    // It is still a control a screen reader reaches: focusable, with its label.
    assert_eq!(common::focusable_total(&mut secret_sim), 1);
    let names: Vec<_> = common::semantics(&mut secret_sim).into_iter().map(|each| (each.role, each.label)).collect();
    assert_eq!(names, [(design::SemanticRole::SecretInput, label("Password"))]);
}

/// Copy and Cut write nothing to the clipboard from a secret field, and do not cut.
#[test]
fn copy_and_cut_write_nothing_from_a_secret_field() {
    let select_all_and_copy = [command('a'), command('c')];
    let stock_seen = typed_into(stock(), &select_all_and_copy);
    assert_eq!(stock_seen.clipboard.len(), 1, "the stock field writes its bullets over the clipboard");

    let empty = Secret::new(String::new());
    for chord in ['c', 'x'] {
        let seen = typed_into(secret(&empty), &[command('a'), command(chord)]);
        assert!(seen.clipboard.is_empty(), "ctrl+{chord} wrote to the clipboard");
        assert_eq!(seen.messages.len(), PASSWORD.len(), "ctrl+{chord} changed the text");
    }
    // Ctrl+Insert and Shift+Delete are Copy and Cut too.
    for (key, modifiers) in [(Named::Insert, Modifiers::CTRL), (Named::Delete, Modifiers::SHIFT)] {
        let event = iced_core::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
            key: iced_core::keyboard::Key::Named(key),
            modified_key: iced_core::keyboard::Key::Named(key),
            physical_key: iced_core::keyboard::key::Physical::Unidentified(
                iced_core::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced_core::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        });
        let seen = typed_into(secret(&empty), &[command('a'), event]);
        assert!(seen.clipboard.is_empty() && seen.messages.len() == PASSWORD.len(), "{key:?}");
    }
}

/// A secret field never asks for an input method; a stock focused field does.
#[test]
fn no_input_method_is_asked_for_by_a_secret_field() {
    let stock_seen = typed_into(stock(), &[]);
    assert!(matches!(stock_seen.input_method, InputMethod::Enabled { .. }), "the stock field asks for one");
    let empty = Secret::new(String::new());
    let seen = typed_into(secret(&empty), &[]);
    assert!(matches!(seen.input_method, InputMethod::Disabled), "{:?}", seen.input_method);
}

/// Revealing is the screen's choice and protects nothing less.
#[test]
fn a_revealed_secret_is_still_not_copied_or_read_by_an_operation() {
    let value = Secret::new(PASSWORD.to_string());
    let revealed =
        SecretInput::new(label("Password"), &value).revealed(true).on_input(Typed::Secret).view::<Renderer>();
    let mut sim = sim(container(revealed));
    assert!(sim.find(PASSWORD).is_err(), "a field showing its text still hands none to an operation");
    let tree = common::access_tree(&mut sim, FIELD);
    let (_, field) = common::node_named(&tree, "Password");
    assert_eq!((field.role(), field.value()), (accesskit::Role::PasswordInput, None));
    assert!(!format!("{:?}", tree.nodes).contains(PASSWORD), "nor a screen reader's tree, in any property");
    let seen = testing::interact(
        container(
            SecretInput::new(label("Password"), &value).revealed(true).on_input(Typed::Secret).view::<Renderer>(),
        ),
        FIELD,
        OVER_FIELD,
        &[click()[0].clone(), command('a'), command('c')],
    );
    assert!(seen.clipboard.is_empty());
}
