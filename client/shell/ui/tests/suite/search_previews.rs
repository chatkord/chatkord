//! Message search and link previews as the canvas's app.search and chat.preview states draw them: the search layer
//! (matches, nothing found, did not answer, under way), the card under a message with and without a picture, and the
//! bar over the composer while a link is read and once it is made, each a reference in both schemes; and every press
//! sends what it names.

use crate::common;

use common::{boxes, sim_sized};
use design::{Accent, PersonColour, Scheme, Space, Surface, Swatch};
use iced_core::image::Handle;
use iced_core::{Element, Length, Size, Widget};
use iced_widget::container;
use ui::testing::{self, Renderer};
use ui::{
    CardPicture, FoundFile, LinkCard, Pin, PreviewBar, SearchBody, Surfaced, Theme, link_card, preview_bar,
    search_layer,
};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const PANEL: Size = Size::new(920.0, 560.0);
const NARROW: Size = Size::new(420.0, 700.0);
const SHORT_PANEL: Size = Size::new(600.0, 420.0);
const CARD: Size = Size::new(480.0, 420.0);
const BAR: Size = Size::new(690.0, 60.0);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    Open(u8),
    More,
    Again,
    Link,
    Remove,
    Close,
}

type El<'a> = Element<'a, Did, Theme, Renderer>;

fn colour(index: usize) -> PersonColour {
    PersonColour::Swatch(Swatch::ALL[index % Swatch::ALL.len()])
}

fn match_of(at: u8, name: &str, when: &str, words: &str, place: &str) -> (Option<String>, Pin<'static, Did, Renderer>) {
    let pin = Pin {
        name: name.to_string(),
        avatar: ui::AvatarKind::Called(colour(usize::from(at))),
        picture: None,
        at: when.to_string(),
        text: words.to_string(),
        body: ui::text(words.to_string(), design::TypeStyle::Body)._boxed(),
        jump: Did::Open(at),
        unpin: None,
    };
    (Some(place.to_string()), pin)
}

fn found() -> SearchBody<'static, Did, Renderer> {
    SearchBody::Found {
        messages: vec![
            match_of(1, "mira", "Yesterday · 22:38", "who has the dice from last week?", "#general"),
            match_of(2, "Bo", "Today · 21:16", "bringing my own dice, the red ones", "#general › thread"),
            match_of(3, "grace", "Monday · 18:02", "don’t forget the dice on Saturday, all five", "grace"),
        ],
        files: Some(vec![
            FoundFile {
                name: "dice.png".to_string(),
                note: "2.4 MB · #general · Yesterday".to_string(),
                picture: true,
                jump: Did::Open(4),
            },
            FoundFile {
                name: "dice-odds.pdf".to_string(),
                note: "182 kB · #game-night · Monday".to_string(),
                picture: false,
                jump: Did::Open(5),
            },
        ]),
        more: Some(Did::More),
    }
}

fn panel(body: SearchBody<'static, Did, Renderer>, size: Size) -> El<'static> {
    let field = ui::text("dice".to_string(), design::TypeStyle::Body)._boxed();
    let layer = Size::new(size.width + 32.0, size.height + 32.0);
    search_layer(common::label("Search Night Owls"), field, body, Did::Close, layer, &WORDS)
}

/// A picture of the canvas's kind: a table over a darker floor, `width` by `height`.
fn scene(width: u32, height: u32) -> Handle {
    let horizon = height * 4 / 5;
    let rgba: Vec<u8> = (0..height)
        .flat_map(|y| {
            let [r, g, b] = if y < horizon { [0x2E, 0x5A, 0x43] } else { [0x5B, 0x3A, 0x24] };
            (0..width).flat_map(move |_| [r, g, b, 255])
        })
        .collect();
    Handle::from_rgba(width, height, rgba)
}

fn card(picture: bool) -> El<'static> {
    link_card(LinkCard {
        host: "blog.example.invalid".to_string(),
        title: "Game night, week 12: the long campaign begins".to_string(),
        description: "Five players, one map and far too many snacks. How the first session went, what broke, and \
                      what we are changing before the next one."
            .to_string(),
        picture: picture.then(|| CardPicture { width: 1600, height: 1200, thumbnail: Some(scene(400, 300)) }),
        label: common::label("Open Game night, week 12: the long campaign begins, blog.example.invalid"),
        open: Did::Link,
    })
}

fn bar(ready: bool) -> El<'static> {
    let bar = if ready {
        PreviewBar::Ready {
            host: "blog.example.invalid".to_string(),
            title: "Game night, week 12: the long campaign begins".to_string(),
            picture: Some(scene(96, 96)),
        }
    } else {
        PreviewBar::Loading("Getting a preview of blog.example.invalid…".to_string())
    };
    preview_bar(bar, Did::Remove, &WORDS)
}

/// `element` on the channel's surface, padded as the canvas's chat column pads it.
fn on_channel(element: El<'_>) -> El<'_> {
    container(element)
        .padding(Space::S.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

fn picture(theme: &Theme, element: El<'_>, size: Size) -> testing::Image {
    testing::render::<Did>(theme, on_channel(element), size, 1.0)
}

/// Presses the control a screen reader names `name` and returns what was sent.
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
fn the_search_results_are_the_reference_with_matches_nothing_found_no_answer_and_waiting() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let name = |state: &str| format!("app-search{state}-{scheme:?}").to_lowercase();
        ui::refs!().assert(&name(""), &picture(&theme, panel(found(), PANEL), PANEL));
        ui::refs!().assert(&name("-start"), &picture(&theme, panel(SearchBody::Start, SHORT_PANEL), SHORT_PANEL));
        ui::refs!().assert(&name("-narrow"), &picture(&theme, panel(found(), NARROW), NARROW));
        let nothing = SearchBody::Nothing("xyzzy".to_string());
        ui::refs!().assert(&name("-empty"), &picture(&theme, panel(nothing, SHORT_PANEL), SHORT_PANEL));
        let failed = SearchBody::Failed(Did::Again);
        ui::refs!().assert(&name("-failed"), &picture(&theme, panel(failed, SHORT_PANEL), SHORT_PANEL));
        ui::refs!().assert(&name("-waiting"), &picture(&theme, panel(SearchBody::Waiting, SHORT_PANEL), SHORT_PANEL));
    }
}

/// A match's Jump, Show more and Try again each send theirs; the panel names itself for a screen reader.
#[test]
fn jump_show_more_and_try_again_send_theirs() {
    let mut files = sim_sized(panel(found(), PANEL), PANEL);
    let jumps: Vec<_> = boxes(&mut files)
        .into_iter()
        .filter(|(each, _)| each.label.as_str() == "Jump")
        .map(|(_, at)| at.center())
        .collect();
    assert_eq!(jumps.len(), 5, "three messages and two files");
    files.point_at(jumps[4]);
    files.simulate(testing::click());
    assert_eq!(files.drain().collect::<Vec<_>>(), [Did::Open(5)]);
    let mut sim = sim_sized(panel(found(), PANEL), PANEL);
    let jumps: Vec<_> = boxes(&mut sim)
        .into_iter()
        .filter(|(each, _)| each.label.as_str() == "Jump")
        .map(|(_, at)| at.center())
        .collect();
    assert_eq!(jumps.len(), 5);
    sim.point_at(jumps[1]);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Did::Open(2)]);
    assert_eq!(press_named(panel(found(), PANEL), PANEL, "Show more"), [Did::More]);
    assert_eq!(press_named(panel(SearchBody::Failed(Did::Again), SHORT_PANEL), SHORT_PANEL, "Try again"), [Did::Again]);
    let mut dialog = sim_sized(panel(found(), PANEL), PANEL);
    assert!(common::semantics(&mut dialog).iter().any(|each| each.label.as_str() == "Search Night Owls"));
}

#[test]
fn a_panel_with_no_older_matches_has_no_show_more() {
    let last = SearchBody::Found {
        messages: vec![match_of(1, "mira", "Yesterday · 22:38", "who has the dice from last week?", "#general")],
        files: Some(Vec::new()),
        more: None,
    };
    let mut sim = sim_sized(panel(last, PANEL), PANEL);
    assert!(common::semantics(&mut sim).iter().all(|each| each.label.as_str() != "Show more"));
}

/// A server that does not search files has no Files section: no heading, no "no files" line.
#[test]
fn a_server_that_does_not_search_files_shows_no_files_section() {
    let files_heading = WORDS.phrase(words::Phrase::FilesHeading).into_owned();
    let shown = |files| {
        let body = SearchBody::Found {
            messages: vec![match_of(1, "mira", "Yesterday · 22:38", "who has the dice from last week?", "#general")],
            files,
            more: None,
        };
        let mut sim = sim_sized(panel(body, PANEL), PANEL);
        sim.find(common::Plain(&files_heading)).is_ok()
    };
    assert!(shown(Some(Vec::new())), "searched, with none found: the section says so");
    assert!(!shown(None), "not searched: no section");
}

#[test]
fn the_link_preview_card_is_the_reference_with_a_picture_and_without() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let name = |state: &str| format!("chat-preview{state}-{scheme:?}").to_lowercase();
        ui::refs!().assert(&name(""), &picture(&theme, card(true), CARD));
        ui::refs!().assert(&name("-bare"), &picture(&theme, card(false), CARD));
    }
}

/// One press on the card opens the link, and a screen reader hears the page's title and the link's host.
#[test]
fn a_press_on_the_card_opens_the_link() {
    let name = "Open Game night, week 12: the long campaign begins, blog.example.invalid";
    assert_eq!(press_named(card(true), CARD, name), [Did::Link]);
}

#[test]
fn the_bar_over_the_composer_is_the_reference_while_the_link_is_read_and_once_it_is_made() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let name = |state: &str| format!("chat-compose-preview{state}-{scheme:?}").to_lowercase();
        ui::refs!().assert(&name(""), &picture(&theme, bar(true), BAR));
        ui::refs!().assert(&name("-loading"), &picture(&theme, bar(false), BAR));
    }
}

/// The bar's way out sends the removal, made and while the link is still read.
#[test]
fn the_bar_s_way_out_removes_the_preview() {
    assert_eq!(press_named(bar(true), BAR, "Remove preview"), [Did::Remove]);
    assert_eq!(press_named(bar(false), BAR, "Remove preview"), [Did::Remove]);
}
