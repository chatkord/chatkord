//! Every button variant in every state, in both schemes, as one row of pictures; each state is reached the way a
//! person reaches it (a pointer over it, a press, the Tab key), never by asking a widget to pretend.

use crate::common;

use common::label;
use design::{Accent, AccentRole, ButtonVariant, Role, Scheme, Surface};
use iced_core::{Element, Length, Point, Size};
use iced_widget::container;
use std::collections::HashMap;
use ui::testing::{self, Image, Renderer, Scene};
use ui::{Button, FocusScope, Surfaced, Theme};

const CELL: Size = Size::new(150.0, 56.0);
// A regular-weight label (a secondary button) covers fewer whole pixels than a semibold one.
const LEAST_TEXT_PIXELS: usize = 5;

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    Hover,
    Pressed,
    Disabled,
    Loading,
    FocusVisible,
}

const STATES: [State; 6] =
    [State::Idle, State::Hover, State::Pressed, State::Disabled, State::Loading, State::FocusVisible];

fn cell(theme: &Theme, variant: ButtonVariant, state: State) -> Image {
    let mut button = Button::new(label("Send")).variant(variant);
    if !matches!(state, State::Disabled) {
        button = button.on_press(());
    }
    let button: Element<'_, (), Theme, Renderer> = button.loading(matches!(state, State::Loading)).view();
    // A control on a picture is drawn on what it sits on there.
    let ground = match variant {
        ButtonVariant::OnScrim => Surfaced::ScrimPill,
        ButtonVariant::OnPicture => Surfaced::Screen,
        _ => Surfaced::On(Surface::Raised),
    };
    let panel = container(FocusScope::new(button)).center(Length::Fill).class(ground);
    let centre = Point::new(CELL.width / 2.0, CELL.height / 2.0);
    let scene = match state {
        State::Idle | State::Disabled | State::Loading => Scene::default(),
        State::Hover => Scene::hovering(centre),
        State::Pressed => Scene::pressing(centre),
        State::FocusVisible => Scene::tabbing(false),
    };
    testing::render_scene(theme, panel, CELL, 1.0, &scene)
}

#[test]
fn every_variant_in_every_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for variant in ButtonVariant::ALL {
            let row: Vec<Image> = STATES.iter().map(|state| cell(&theme, variant, *state)).collect();
            let name = format!("button-{variant:?}-{scheme:?}").to_lowercase();
            ui::refs!().assert(&name, &testing::side_by_side(&row));
        }
    }
}

/// The text of every look is painted in the colour the look names, not in another a widget brought: some of the
/// label's own pixels are exactly that role's colour.
#[test]
fn the_text_of_every_look_is_painted_in_its_role() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for variant in ButtonVariant::ALL {
            for (state, interaction) in [
                (State::Idle, design::Interaction::Idle),
                (State::Hover, design::Interaction::Hover),
                (State::Pressed, design::Interaction::Pressed),
                (State::Disabled, design::Interaction::Disabled),
                (State::Loading, design::Interaction::Loading),
            ] {
                let want = theme.palette().color(design::button(variant, interaction).text);
                let image = cell(&theme, variant, state);
                let exact = image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| **pixel == [want.r, want.g, want.b, 255])
                    .count();
                assert!(
                    exact >= LEAST_TEXT_PIXELS,
                    "{variant:?}/{state:?} in {scheme:?}: only {exact} pixels in the text colour"
                );
            }
        }
    }
}

/// The colour that fills most of an image apart from `background`.
fn dominant(image: &Image, background: [u8; 4]) -> [u8; 4] {
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for pixel in image.rgba.as_chunks::<4>().0.iter().filter(|pixel| **pixel != background) {
        *counts.entry(*pixel).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map(|(pixel, _)| pixel).unwrap()
}

/// The theme's mapping reaches the pixels: an accent button is filled with the palette's accent fill, hovered with
/// its hover fill and pressed with its pressed fill, whatever the server chose.
#[test]
fn a_primary_button_is_painted_in_the_servers_accent_fill_hover_and_pressed() {
    for seed in [0x4F6BED, 0xFFFF00, 0x00AA55, 0xB3261E] {
        for scheme in Scheme::ALL {
            let theme = Theme::new(scheme, Accent::from(design::Rgb::hex(seed)));
            let surface = theme.palette().color(Role::Surface(Surface::Raised));
            let background = [surface.r, surface.g, surface.b, 255];
            for (state, role) in [
                (State::Idle, AccentRole::Fill),
                (State::Hover, AccentRole::FillHover),
                (State::Pressed, AccentRole::FillPressed),
            ] {
                let fill = theme.palette().color(Role::Accent(role));
                let found = dominant(&cell(&theme, ButtonVariant::Primary, state), background);
                assert_eq!(found, [fill.r, fill.g, fill.b, 255], "{state:?} in {scheme:?} under {seed:06x}");
            }
        }
    }
}

/// Keyboard focus is the hover look and nothing more: a button Tab reached draws as the same button under the
/// pointer, with nothing outside it.
#[test]
fn a_button_tab_reached_looks_hovered() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for variant in ButtonVariant::ALL {
            let tabbed = cell(&theme, variant, State::FocusVisible);
            let hovered = cell(&theme, variant, State::Hover);
            let differs = testing::compare(&tabbed, &hovered).expect("the same size");
            assert!(differs.within_tolerance(), "{variant:?} in {scheme:?}: {differs:?}");
        }
    }
}
