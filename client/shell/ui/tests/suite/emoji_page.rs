//! Server settings › Emoji as the canvas draws it: the server's twelve emoji, one renamed in place, its name refused,
//! and the server full with Upload off, each a reference in both schemes; every control sends what it is for.

use crate::common;

use common::{boxes, sim_sized};
use design::{Accent, PersonColour, Scheme, Space, Surface, Swatch};
use iced_core::{Element, Length, Size, Widget};
use iced_widget::{Column, container};
use ui::testing::{self, Renderer};
use ui::{EmojiCard, Renaming, Surfaced, Theme, emoji_action, emoji_page};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const NIGHT_OWLS: &str = "Night Owls";
const NAMES: [&str; 12] = [
    "owlwave",
    "owl_sleepy",
    "party_owl",
    "gold_die",
    "red_die",
    "trophy",
    "tomato",
    "owl_heart",
    "night",
    "gamepad",
    "pizza",
    "gg_star",
];
const ADDED_BY: [&str; 3] = ["grace", "mira", "Bo"];
/// The canvas's settings page at 1280 wide, less its list and gutters.
const PAGE: Size = Size::new(928.0, 520.0);
const ACTION: Size = Size::new(280.0, 60.0);
const MOST: u32 = 250;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    Upload,
    Rename(usize),
    Delete(usize),
    Typed(String),
    Save,
    Cancel,
}

type El<'a> = Element<'a, Did, Theme, Renderer>;

/// A flat picture in one of the swatches, as the host hands a decoded emoji over.
fn picture(index: usize) -> iced_core::image::Handle {
    const SIDE: u32 = 32;
    let step = u8::try_from(index).unwrap();
    let [r, g, b] = [step.wrapping_mul(40), 200 - step * 10, 90];
    let rgba: Vec<u8> = std::iter::repeat_n([r, g, b, 255], usize::try_from(SIDE * SIDE).unwrap()).flatten().collect();
    iced_core::image::Handle::from_rgba(SIDE, SIDE, rgba)
}

fn cards(pictured: bool) -> Vec<EmojiCard<Did>> {
    NAMES
        .iter()
        .enumerate()
        .map(|(at, name)| EmojiCard {
            name: (*name).to_string(),
            picture: pictured.then(|| picture(at)),
            added_by: ADDED_BY[at % ADDED_BY.len()].to_string(),
            added_colour: PersonColour::Swatch(Swatch::ALL[at % Swatch::ALL.len()]),
            added_picture: None,
            rename: Did::Rename(at),
            delete: Did::Delete(at),
        })
        .collect()
}

fn renaming(draft: &str, refused: bool) -> Renaming<'_, Did> {
    Renaming {
        at: 0,
        draft,
        on_change: Box::new(Did::Typed),
        on_submit: Did::Save,
        on_escape: Did::Cancel,
        refused,
        new: false,
    }
}

/// The page's action over its content, on the page's surface, as the settings frame lays them out.
fn page<'a>(held: u32, renamed: Option<Renaming<'a, Did>>) -> El<'a> {
    let column = Column::with_children([
        container(emoji_action(held, MOST, Some(Did::Upload), &WORDS)).align_right(Length::Fill)._boxed(),
        emoji_page(NIGHT_OWLS, held, MOST, None, cards(true), renamed, &WORDS),
    ])
    .spacing(Space::L.px());
    container(column)
        .padding(Space::L.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

fn render(theme: &Theme, element: El<'_>) -> testing::Image {
    testing::render::<Did>(theme, element, PAGE, 1.0)
}

#[test]
fn the_page_is_the_reference_in_each_state() {
    let typed = "owl_wave";
    let refused = "o";
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let name = |state: &str| format!("server-emoji{state}-{scheme:?}").to_lowercase();
        ui::refs!().assert(&name(""), &render(&theme, page(12, None)));
        ui::refs!().assert(&name("-rename"), &render(&theme, page(12, Some(renaming(typed, false)))));
        ui::refs!().assert(&name("-refused"), &render(&theme, page(12, Some(renaming(refused, true)))));
        ui::refs!().assert(&name("-full"), &render(&theme, page(MOST, None)));
    }
}

/// An emoji whose picture the host does not have yet shows its `:name:`, so its card is never blank.
#[test]
fn a_card_without_its_picture_shows_its_name() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let without = emoji_page(NIGHT_OWLS, 12, MOST, None, cards(false), None, &WORDS);
    let with = emoji_page(NIGHT_OWLS, 12, MOST, None, cards(true), None, &WORDS);
    let shown = |element: El<'_>| testing::render::<Did>(&theme, element, PAGE, 1.0);
    assert!(testing::compare(&shown(without), &shown(with)).is_some(), "the placeholder draws something else");
}

/// Presses the control a screen reader names `name` on `element` and returns what was sent.
fn press_named(element: El<'_>, size: Size, name: &str) -> Vec<Did> {
    let mut sim = sim_sized(element, size);
    let at = boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| each.label.as_str() == name)
        .map(|(_, bounds)| bounds.center())
        .unwrap_or_else(|| panic!("no {name}"));
    sim.point_at(at);
    sim.simulate(testing::click());
    sim.drain().collect()
}

#[test]
fn rename_delete_and_upload_send_theirs() {
    let cards_only = || emoji_page(NIGHT_OWLS, 12, MOST, None, cards(true), None, &WORDS);
    assert_eq!(press_named(cards_only(), PAGE, "Rename :party_owl:"), [Did::Rename(2)]);
    assert_eq!(press_named(cards_only(), PAGE, "Delete :gg_star:"), [Did::Delete(11)]);
    assert_eq!(press_named(emoji_action(12, MOST, Some(Did::Upload), &WORDS), ACTION, "Upload emoji"), [Did::Upload]);
}

/// At the most a server holds, Upload takes no press.
#[test]
fn upload_is_off_when_the_server_is_full() {
    assert!(press_named(emoji_action(MOST, MOST, Some(Did::Upload), &WORDS), ACTION, "Upload emoji").is_empty());
}

/// Renaming: typing sends the name, Enter and "save" save it, Escape and "cancel" leave it.
#[test]
fn the_renamed_card_types_saves_and_cancels() {
    let renamed = || emoji_page(NIGHT_OWLS, 12, MOST, None, cards(true), Some(renaming("owl", false)), &WORDS);
    assert_eq!(press_named(renamed(), PAGE, "save"), [Did::Save]);
    assert_eq!(press_named(renamed(), PAGE, "cancel"), [Did::Cancel]);
    let mut sim = sim_sized(renamed(), PAGE);
    common::settle(&mut sim);
    sim.simulate(testing::typing("_wave"));
    let typed: Vec<Did> = sim.drain().collect();
    assert!(typed.iter().any(|did| matches!(did, Did::Typed(name) if name.starts_with("owl"))), "{typed:?}");
    common::press(&mut sim, iced_core::keyboard::key::Named::Escape);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Did::Cancel]);
}

/// Upload pressed, the new emoji's name field opens and takes the keyboard by itself: Enter then saves the name, and
/// never presses Upload again though the press left it the focus.
#[test]
fn enter_in_a_name_field_that_opened_after_upload_saves_it() {
    use ui::testing::{Cache, interact_kept};
    let mut kept: Option<Cache> = None;
    let framed = |page: El<'static>| ui::FocusScope::new(page)._boxed();
    let upload = boxes(&mut sim_sized(page(12, None), PAGE))
        .into_iter()
        .find(|(each, _)| each.label.as_str() == "Upload emoji")
        .map(|(_, bounds)| bounds.center())
        .expect("Upload");
    let pressed = interact_kept(&mut kept, framed(page(12, None)), PAGE, upload, &testing::click());
    assert_eq!(pressed, [Did::Upload]);
    let naming = || Some(Renaming { new: true, ..renaming("", false) });
    let _ = interact_kept(&mut kept, framed(page(12, naming())), PAGE, upload, &[]);
    let entered = interact_kept(
        &mut kept,
        framed(page(12, naming())),
        PAGE,
        upload,
        &[testing::key_press(iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Enter), false)],
    );
    assert_eq!(entered, [Did::Save], "the field's Enter, not Upload's");
}
