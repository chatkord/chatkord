//! The text fields in every state, in both schemes, as rows of pictures; focus and hover are reached the way a person
//! reaches them (a pointer over the field, a click), composing by an input method's own events.

use crate::common;

use common::label;
use design::{Accent, Scheme, Space, Surface};
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::container;
use terms::Secret;
use ui::testing::{self, Image, Renderer, Scene, click};
use ui::{SecretInput, Surfaced, TextInput, Theme};

const CELL: Size = Size::new(190.0, 56.0);

fn cell(theme: &Theme, field: Element<'_, (), Theme, Renderer>, scene: &Scene) -> Image {
    let panel = container(field)
        .padding(Space::S.px())
        .center_y(Length::Fill)
        .width(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
    testing::render_scene(theme, panel._boxed(), CELL, 1.0, scene)
}

fn over() -> Point {
    Point::new(CELL.width / 2.0, CELL.height / 2.0)
}

fn focused() -> Scene {
    Scene { cursor: Some(over()), events: click().to_vec(), ..Scene::default() }
}

fn plain<'a>(value: &'a str, enabled: bool, invalid: bool) -> Element<'a, (), Theme, Renderer> {
    let mut field = TextInput::new(label("Name"), value).placeholder("Name").invalid(invalid);
    if enabled {
        field = field.on_input(|_| ());
    }
    field.view()
}

fn secret<'a>(value: &'a Secret, revealed: bool, invalid: bool, enabled: bool) -> Element<'a, (), Theme, Renderer> {
    let mut field =
        SecretInput::new(label("Password"), value).placeholder("Password").revealed(revealed).invalid(invalid);
    if enabled {
        field = field.on_input(|_| ());
    }
    field.view()
}

#[test]
fn the_plain_field_in_every_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row = [
            cell(&theme, plain("", true, false), &Scene::default()),
            cell(&theme, plain("", true, false), &Scene::hovering(over())),
            cell(&theme, plain("Ada Lovelace", true, false), &Scene::default()),
            cell(&theme, plain("", true, false), &focused()),
            cell(&theme, plain("Ada", true, true), &Scene::default()),
            cell(&theme, plain("Ada Lovelace", false, false), &Scene::default()),
        ];
        ui::refs!().assert(&format!("input-plain-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[test]
fn the_secret_field_in_every_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let (empty, typed) = (Secret::new(String::new()), Secret::new("hunter2".to_string()));
        let row = [
            cell(&theme, secret(&empty, false, false, true), &Scene::default()),
            cell(&theme, secret(&typed, false, false, true), &Scene::default()),
            cell(&theme, secret(&typed, true, false, true), &Scene::default()),
            cell(&theme, secret(&empty, false, false, true), &focused()),
            cell(&theme, secret(&typed, false, true, true), &Scene::default()),
            cell(&theme, secret(&typed, false, false, false), &Scene::default()),
        ];
        ui::refs!().assert(&format!("input-secret-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

/// What an input method is composing at the caret of the focused field, and the clause it is converting if any.
fn composing(text: &str, clause: Option<std::ops::Range<usize>>) -> Scene {
    use iced_core::{Event, input_method};
    let mut scene = focused();
    scene.events.push(Event::InputMethod(input_method::Event::Opened));
    scene.events.push(Event::InputMethod(input_method::Event::Preedit(text.to_string(), clause)));
    scene
}

#[test]
fn composing_text_shows_at_the_caret_and_never_over_a_secret_field() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row = [
            cell(&theme, plain("Ada ", true, false), &composing("lovelace", None)),
            cell(&theme, plain("Ada ", true, false), &composing("lovelace", Some(0..4))),
        ];
        ui::refs!().assert(&format!("input-composing-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        let empty = Secret::new(String::new());
        let asked = cell(&theme, secret(&empty, false, false, true), &composing("lovelace", None));
        let plain_focus = cell(&theme, secret(&empty, false, false, true), &focused());
        let difference = testing::compare(&asked, &plain_focus).expect("one size");
        assert!(difference.within_tolerance(), "a secret field drew composing text: {difference:?}");
    }
}

#[test]
fn an_empty_field_hides_its_placeholder_while_text_is_composed() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let bare = TextInput::new(label("Name"), "").on_input(|_| ()).view();
        let composed = cell(&theme, plain("", true, false), &composing("a", None));
        let without = cell(&theme, bare, &composing("a", None));
        let difference = testing::compare(&composed, &without).expect("one size");
        assert!(difference.within_tolerance(), "the placeholder showed beside composing text: {difference:?}");
    }
}
