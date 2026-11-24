//! A text channel's messages at the size and spacing Settings › Appearance picks: the page's two choices, its preview
//! drawn with the channel's own row, and a channel's rows at the largest size, each a reference in both schemes.

use crate::common;

use choices::LookPick;
use common::{semantics, sim_sized};
use design::{Accent, MessageSize, RunGap, Scheme, SemanticRole};
use iced_core::Size;
use screens::harness::{ADA, Clock, chatting, first_page, said};
use screens::{Window, WindowMessage};
use std::time::Instant;
use ui::testing::{self, Renderer};
use ui::{Theme, app};

const WINDOW: Size = Size::new(1280.0, 768.0);

fn size(px: u16) -> MessageSize {
    MessageSize::of_px(px).expect("a step")
}

fn gap(px: u16) -> RunGap {
    RunGap::of_px(px).expect("a step")
}

fn picked(window: &mut Window, picks: &[LookPick]) {
    for pick in picks {
        window.update(WindowMessage::Look(*pick), Clock(Instant::now()).at());
    }
}

/// Night Owls' #general open with its people listed and two runs, Ada's and Bo's, the second of two messages.
fn chat() -> Window {
    let clock = Clock(Instant::now());
    let (mut window, owls) = chatting(&clock);
    screens::harness::people_listed(&mut window, &clock, owls);
    let messages = vec![
        said(1, 1, ADA, "who’s around tonight?"),
        said(2, 2, "bo", "yes! give me ten minutes"),
        said(3, 3, "bo", "and a long one that wraps once the text is large enough to need a second line"),
    ];
    screens::harness::tell(
        &mut window,
        &clock,
        owls,
        first_page(screens::chat::Page { messages, older: false, newer: false, revision: 3 }),
    );
    window
}

/// Appearance, opened from Night Owls' channel.
fn appearance() -> Window {
    let (mut window, _) = chatting(&Clock(Instant::now()));
    window.update(WindowMessage::OpenDevice(screens::DevicePage::Appearance), Clock(Instant::now()).at());
    window
}

fn render(theme: &Theme, window: &Window) -> testing::Image {
    testing::render::<WindowMessage>(theme, app::view::<Renderer>(window, WINDOW), WINDOW, 1.0)
}

/// Text size and space between message groups are radio groups of their steps in pixels, the defaults chosen, and a
/// press on a step sends it.
#[test]
fn the_messages_choices_name_their_steps_and_a_press_sends_one() {
    let window = appearance();
    let mut sim = sim_sized(app::view::<Renderer>(&window, WINDOW), WINDOW);
    let found: Vec<(SemanticRole, String)> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    for (role, said) in [
        (SemanticRole::RadioGroup, "Text size"),
        (SemanticRole::RadioGroup, "Space between message groups"),
        (SemanticRole::Radio { chosen: true }, "14"),
        (SemanticRole::Radio { chosen: false }, "24"),
        (SemanticRole::Radio { chosen: true }, "16"),
        (SemanticRole::Radio { chosen: false }, "0"),
    ] {
        assert!(found.iter().any(|(each, label)| *each == role && label == said), "{role:?} {said} in {found:?}");
    }
    let boxes = common::boxes(&mut sim);
    let at =
        |said: &str| boxes.iter().find(|(each, _)| each.label.as_str() == said).map(|(_, at)| at.center()).unwrap();
    for point in [at("24"), at("0")] {
        sim.point_at(point);
        sim.simulate(testing::click());
    }
    let got: Vec<LookPick> =
        sim.drain().filter_map(|each| if let WindowMessage::Look(pick) = each { Some(pick) } else { None }).collect();
    assert_eq!(got, [LookPick::MessageSize(size(24)), LookPick::RunGap(gap(0))]);
}

/// The preview draws its two runs at the size and spacing picked, at both ends of each ladder.
#[test]
fn the_preview_is_the_reference_at_each_end_of_the_scale() {
    for (name, picks) in [
        ("smallest", [LookPick::MessageSize(size(12)), LookPick::RunGap(gap(0))]),
        ("largest", [LookPick::MessageSize(size(24)), LookPick::RunGap(gap(24))]),
    ] {
        let mut window = appearance();
        picked(&mut window, &picks);
        for scheme in Scheme::ALL {
            let theme = Theme::new(scheme, Accent::DEFAULT);
            ui::refs!().assert(&format!("messages-preview-{name}-{scheme:?}").to_lowercase(), &render(&theme, &window));
        }
    }
}

/// A channel's rows at 24 px: the author, the time and the words grow together, a long line wraps at the larger size,
/// and the runs stand the picked space apart.
#[test]
fn a_channels_rows_at_24_px_are_the_reference() {
    let mut window = chat();
    picked(&mut window, &[LookPick::MessageSize(size(24))]);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        ui::refs!().assert(&format!("messages-rows-24-{scheme:?}").to_lowercase(), &render(&theme, &window));
    }
}
