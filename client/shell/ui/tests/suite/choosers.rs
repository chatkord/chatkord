//! The composer's chooser after `@`, `#` and `:`, as the canvas's chat.mentions, chat.mentions.member, chat.channels and
//! chat.emoji draw it: each a reference in both schemes, and what its keys and a press send.

use crate::common;

use common::{boxes, label, press, sim_sized};
use design::{Accent, Rgb, Scheme, Space, Surface};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Size, Widget};
use iced_widget::{Column, container, text_input};
use screens::harness::member;
use ui::testing::{self, Renderer};
use ui::{
    ChooserGroup, ChooserKeys, ChooserLeading, ChooserRow, ReactionEmoji, Surfaced, Theme, chooser, chooser_keys,
};
use words::{Phrase, Sentence, Words};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<Words> = std::sync::LazyLock::new(Words::english);

const GRACE: &str = "grace";
const BO: &str = "bo";
const MIRA: &str = "mira";

#[derive(Clone, Debug, PartialEq)]
enum Said {
    Chose(&'static str),
    Up,
    Down,
    Choose,
    Escape,
    Typed(String),
}

type El<'a> = Element<'a, Said, Theme, Renderer>;

fn row(leading: ChooserLeading, name: &str, secondary: Option<&str>, key: &'static str) -> ChooserRow<Said> {
    ChooserRow { leading, name: label(name), secondary: secondary.map(str::to_string), choose: Said::Chose(key) }
}

fn heading(phrase: Phrase) -> design::Label {
    label(&WORDS.phrase(phrase))
}

fn members() -> ChooserGroup<Said> {
    let person = |id: &str| ChooserLeading::Person(screens::colour_of(&member(id), None), None);
    ChooserGroup {
        heading: heading(Phrase::ChooserMembers),
        rows: vec![
            row(person(GRACE), "grace", Some("grace"), GRACE),
            row(person(BO), "Bo", Some("bo"), BO),
            row(person(MIRA), "mira", Some("mira"), MIRA),
        ],
    }
}

/// Typing `@` as an admin: the members, the roles, then everyone and here.
fn mentions_admin() -> Vec<ChooserGroup<Said>> {
    let role = |colour: u32| ChooserLeading::Role(Some(Rgb::hex(colour)));
    vec![
        members(),
        ChooserGroup {
            heading: heading(Phrase::ChooserRoles),
            rows: vec![
                row(role(0xFF8A5B), "@Owner", None, "owner"),
                row(role(0x7CB7FF), "@Admin", None, "admin"),
                row(role(0x5FD39B), "@Moderators", None, "mods"),
                row(role(0xE2A6FF), "@Regulars", None, "regulars"),
            ],
        },
        ChooserGroup {
            heading: heading(Phrase::ChooserEveryone),
            rows: vec![
                row(
                    ChooserLeading::Everyone,
                    "@everyone",
                    Some(&WORDS.say(&Sentence::EveryoneNote("general".to_string()))),
                    "everyone",
                ),
                row(ChooserLeading::Everyone, "@here", Some(&WORDS.phrase(Phrase::HereNote)), "here"),
            ],
        },
    ]
}

fn channels() -> Vec<ChooserGroup<Said>> {
    vec![ChooserGroup {
        heading: heading(Phrase::ChooserChannels),
        rows: vec![
            row(ChooserLeading::TextChannel, "general", None, "general"),
            row(ChooserLeading::TextChannel, "clips", Some("Hangouts"), "clips"),
            row(ChooserLeading::TextChannel, "ideas", Some("Hangouts"), "ideas"),
            row(ChooserLeading::VoiceChannel, "Lounge", Some("Hangouts"), "lounge"),
            row(ChooserLeading::VoiceChannel, "Quiet corner", Some("Hangouts"), "corner"),
            row(ChooserLeading::TextChannel, "game-night", Some("Projects"), "game-night"),
        ],
    }]
}

/// Typing `:par`: the server's party_owl, then unicode by shortcode.
fn emoji() -> Vec<ChooserGroup<Said>> {
    let unicode = |shown: &str| ChooserLeading::Emoji(ReactionEmoji::Unicode(shown.to_string()));
    let party_owl = ChooserLeading::Emoji(ReactionEmoji::Custom { name: "party_owl".to_string(), picture: None });
    vec![ChooserGroup {
        heading: label(&WORDS.say(&Sentence::EmojiMatching("par".to_string()))),
        rows: vec![
            row(party_owl, ":party_owl:", Some("Night Owls"), "party_owl"),
            row(unicode("🥳"), ":partying_face:", None, "partying_face"),
            row(unicode("🦜"), ":parrot:", None, "parrot"),
            row(unicode("🪂"), ":parachute:", None, "parachute"),
            row(unicode("🅿️"), ":parking:", None, "parking"),
        ],
    }]
}

fn framed(element: El<'_>) -> El<'_> {
    container(element)
        .padding(Space::S.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

const PANEL: Size = Size::new(360.0, 480.0);

#[test]
fn each_chooser_is_the_reference_in_both_schemes() {
    for (state, groups) in [
        ("chat-mentions", mentions_admin as fn() -> Vec<ChooserGroup<Said>>),
        ("chat-mentions-member", || vec![members()]),
        ("chat-channels", channels),
        ("chat-emoji", emoji),
    ] {
        for scheme in Scheme::ALL {
            let theme = Theme::new(scheme, Accent::DEFAULT);
            let drawn = framed(chooser(label("Mentions"), groups(), 0));
            let image = testing::render::<Said>(&theme, drawn, PANEL, 1.0);
            ui::refs!().assert(&format!("{state}-chooser-{scheme:?}").to_lowercase(), &image);
        }
    }
}

/// A press on a match chooses it, whichever is highlighted.
#[test]
fn a_press_on_a_row_chooses_it() {
    let mut sim = sim_sized(framed(chooser(label("Mentions"), mentions_admin(), 0)), PANEL);
    let found = boxes(&mut sim);
    let at = found.iter().find(|(each, _)| each.label.as_str() == "@Regulars").map(|(_, at)| at.center()).unwrap();
    sim.point_at(at);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Chose("regulars")]);
}

/// With the focus in the composer, the arrows, Enter and Escape go to the chooser and never reach the field, while
/// letters still do.
#[test]
fn the_keys_go_to_the_chooser_before_the_field() {
    let field: El<'_> = text_input("", "").on_input(Said::Typed)._boxed();
    // The field first, so the press that focuses it lands on it whatever the chooser's height.
    let composer = Column::with_children([field, chooser(label("Mentions"), emoji(), 1)]).spacing(Space::S.px());
    let keys = ChooserKeys { up: Said::Up, down: Said::Down, choose: Said::Choose, escape: Said::Escape };
    let mut sim = sim_sized(framed(chooser_keys(composer._boxed(), Some(keys))), Size::new(360.0, 400.0));
    sim.point_at(iced_core::Point::new(180.0, Space::S.px() + Space::M.px()));
    sim.simulate(testing::click());
    sim.drain().for_each(drop);
    for (key, sent) in [
        (Named::ArrowDown, Said::Down),
        (Named::ArrowUp, Said::Up),
        (Named::Enter, Said::Choose),
        (Named::Escape, Said::Escape),
    ] {
        press(&mut sim, key);
        assert_eq!(sim.drain().collect::<Vec<_>>(), [sent], "{key:?}");
    }
    sim.simulate(testing::typing("p"));
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Typed("p".to_string())]);
}
