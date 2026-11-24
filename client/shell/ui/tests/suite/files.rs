//! A message's files and the composer's side of them, as the canvas's chat.files states draw them: pictures as a
//! thumbnail and a grid, a file's card, the strip of files to send (uploading, ready, refused), the drop over a
//! channel and the picture viewer, each a reference in both schemes; and every press sends what it names.

use crate::common;

use common::{boxes, press, settle, sim_sized};
use design::{Accent, Scheme, Space, Surface};
use iced_core::image::Handle;
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::{Column, container};
use ui::testing::{self, Renderer};
use ui::{FileCard, Pending, PendingState, Picture, Surfaced, Theme, Viewed};

/// The English the views are drawn in.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a> = Element<'a, Said, Theme, Renderer>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    Open(usize),
    Save,
    Cancel(usize),
    Close,
    Attach,
}

/// A picture of the canvas's kind: a coloured table over a darker floor, `width` by `height`.
fn scene(width: u32, height: u32, table: [u8; 3], floor: [u8; 3]) -> Handle {
    let horizon = height * 4 / 5;
    let rgba: Vec<u8> = (0..height)
        .flat_map(|y| {
            let [r, g, b] = if y < horizon { table } else { floor };
            (0..width).flat_map(move |_| [r, g, b, 255])
        })
        .collect();
    Handle::from_rgba(width, height, rgba)
}

const GREEN: [u8; 3] = [0x2E, 0x5A, 0x43];
const PURPLE: [u8; 3] = [0x3B, 0x2D, 0x5C];
const WINE: [u8; 3] = [0x7A, 0x2E, 0x3B];
const SEA: [u8; 3] = [0x1F, 0x4E, 0x6B];
const WOOD: [u8; 3] = [0x5B, 0x3A, 0x24];

fn picture(name: &str, at: usize, table: [u8; 3]) -> Picture<Said> {
    Picture { thumbnail: Some(scene(400, 300, table, WOOD)), label: common::label(name), open: Said::Open(at) }
}

fn rules() -> FileCard<Said> {
    FileCard { name: "rules-v2.pdf".into(), note: "1.2 MB · PDF".into(), save: Said::Save }
}

/// The strip as chat.files has it: map-pack.zip a third sent, board.png ready.
fn sending() -> Vec<Pending<Said>> {
    vec![
        Pending {
            name: "map-pack.zip".into(),
            preview: None,
            state: PendingState::Uploading { progress: 4.1 / 12.0, said: "4.1 of 12 MB".into() },
            cancel: Said::Cancel(0),
        },
        Pending {
            name: "board.png".into(),
            preview: Some(scene(1024, 768, WOOD, GREEN)),
            state: PendingState::Ready { size: "860 KB".into() },
            cancel: Said::Cancel(1),
        },
    ]
}

/// The strip as chat.files.refused has it: one over the most a file may be, one over the account's room.
fn refused() -> Vec<Pending<Said>> {
    vec![
        Pending {
            name: "season-recap.mp4".into(),
            preview: None,
            state: PendingState::Refused("This file is over 50 MB.".into()),
            cancel: Said::Cancel(0),
        },
        Pending {
            name: "all-photos.zip".into(),
            preview: None,
            state: PendingState::Refused("You’ve used your 1 GB for files on this server.".into()),
            cancel: Said::Cancel(1),
        },
    ]
}

/// A message's files and the composer's strip under them, on the window's ground.
fn files<'a>(strip: Vec<Pending<Said>>) -> El<'a> {
    let column = Column::with_children([
        ui::pictures::<Said, Renderer>(vec![picture("dice.png", 0, GREEN)]),
        ui::pictures::<Said, Renderer>(vec![
            picture("saturday-1.png", 1, PURPLE),
            picture("saturday-2.png", 2, WINE),
            picture("saturday-3.png", 3, SEA),
        ]),
        ui::file_card::<Said, Renderer>(rules(), &WORDS),
        ui::pending_strip::<Said, Renderer>(strip, &WORDS),
        ui::attach_button::<Said, Renderer>(Some(Said::Attach), &WORDS),
    ])
    .spacing(Space::S.px());
    container(column)
        .padding(Space::L.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

const FILES: Size = Size::new(720.0, 640.0);
const WINDOW: Size = Size::new(1280.0, 768.0);

/// What the drop and the viewer lie over: the channel's files.
fn channel<'a>() -> El<'a> {
    files(Vec::new())
}

fn dropping<'a>() -> El<'a> {
    ui::drop_over::<Said, Renderer>(channel(), Some("Files go to #general, up to 50 MB each.".into()), &WORDS)
}

fn viewing<'a>() -> El<'a> {
    let viewed = Viewed {
        picture: Some(scene(1600, 1200, GREEN, WOOD)),
        under: None,
        shape: Some((1600, 1200)),
        name: "dice.png".into(),
        note: "1600 × 1200 · 2.4 MB".into(),
        save: Said::Save,
    };
    ui::viewer_over::<Said, Renderer>(channel(), Some(viewed), Said::Close, &WORDS)
}

#[test]
fn a_messages_files_and_the_strip_are_the_references_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let scheme = format!("{scheme:?}").to_lowercase();
        ui::refs!().assert(&format!("chat-files-{scheme}"), &testing::render(&theme, files(sending()), FILES, 1.0));
        ui::refs!()
            .assert(&format!("chat-files-refused-{scheme}"), &testing::render(&theme, files(refused()), FILES, 1.0));
    }
}

#[test]
fn the_drop_and_the_viewer_are_the_references_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let scheme = format!("{scheme:?}").to_lowercase();
        ui::refs!().assert(&format!("chat-files-drop-{scheme}"), &testing::render(&theme, dropping(), FILES, 1.0));
        ui::refs!().assert(&format!("chat-files-viewer-{scheme}"), &testing::render(&theme, viewing(), WINDOW, 1.0));
    }
}

/// The centre of the control a screen reader names `said`.
fn centre_of<'a>(sim: &mut common::Sim<'a, Said>, said: &str) -> iced_core::Point {
    boxes(sim)
        .into_iter()
        .find(|(semantics, _)| semantics.label.as_str() == said)
        .map(|(_, bounds)| bounds.center())
        .unwrap_or_else(|| panic!("no {said}"))
}

/// Each thumbnail opens its own picture; Save, each strip file's way out and the paperclip send theirs.
#[test]
fn every_press_on_the_files_sends_what_it_names() {
    let mut sim = sim_sized(files(sending()), FILES);
    for said in
        ["dice.png", "saturday-1.png", "saturday-3.png", "Save", "Cancel upload", "Remove board.png", "Attach a file"]
    {
        let at = centre_of(&mut sim, said);
        sim.point_at(at);
        sim.simulate(testing::click());
    }
    let got: Vec<Said> = sim.drain().collect();
    assert_eq!(
        got,
        [Said::Open(0), Said::Open(1), Said::Open(3), Said::Save, Said::Cancel(0), Said::Cancel(1), Said::Attach]
    );
}

/// A refused file is taken out by its X, named for it.
#[test]
fn a_refused_file_is_removed_by_its_way_out() {
    let mut sim = sim_sized(files(refused()), FILES);
    let at = centre_of(&mut sim, "Remove all-photos.zip");
    sim.point_at(at);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Cancel(1)]);
}

/// The viewer's Save and close send theirs, and Escape closes it as the close button does.
#[test]
fn the_viewer_saves_and_closes_by_its_buttons_and_escape() {
    let mut sim = sim_sized(viewing(), WINDOW);
    settle(&mut sim);
    for said in ["Save", "Close"] {
        let at = centre_of(&mut sim, said);
        sim.point_at(at);
        sim.simulate(testing::click());
    }
    press(&mut sim, Named::Escape);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Save, Said::Close, Said::Close]);
}

/// A press on the picture is the picture's, and one beside it closes the viewer as it does in every chat app.
#[test]
fn a_press_beside_the_viewers_picture_closes_it_and_one_on_it_does_not() {
    let mut sim = sim_sized(viewing(), WINDOW);
    settle(&mut sim);
    sim.point_at(Point::new(WINDOW.width / 2.0, WINDOW.height / 2.0 + 40.0));
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [], "on the picture");
    sim.point_at(Point::new(60.0, WINDOW.height / 2.0 + 40.0));
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Close], "beside it");
}

/// While nothing is dragged, the channel is drawn as it is: the drop adds nothing.
#[test]
fn no_drag_leaves_the_channel_as_it_is() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let bare = testing::render(&theme, channel(), FILES, 1.0);
    let wrapped = testing::render(&theme, ui::drop_over::<Said, Renderer>(channel(), None, &WORDS), FILES, 1.0);
    assert!(bare.rgba == wrapped.rgba, "the same picture");
}
