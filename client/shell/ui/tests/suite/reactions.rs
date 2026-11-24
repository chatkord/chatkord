//! A message's reactions, the tools over a hovered message and the emoji picker, as the canvas's chat.reactions and
//! chat.reactions.custom draw them: each a reference in both schemes, and what a press on each sends.

use crate::common;

use common::{boxes, label, sim_sized};
use design::{Accent, Rgb, Scheme, Space, Surface};
use iced_core::{Element, Length, Size, Widget};
use iced_widget::{Column, container};
use screens::chat::EmojiId;
use screens::chat::emoji::{self, Group, SkinTone};
use ui::testing::{self, Renderer};
use ui::{
    EmojiPicker, EmojiSends, HoverTools, PickedEmoji, PickerSection, ReactionChip, ReactionEmoji, ServerEmoji,
    Surfaced, Theme, emoji_picker, hover_bar, reactions,
};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const OWLWAVE: &str = "owlwave";
const PIZZA: &str = "pizza";
/// Night Owls' colour, behind its initial on the picker's tab.
const OWLS: Rgb = Rgb::hex(0xFF8A5B);

#[derive(Clone, Debug, PartialEq)]
enum Did {
    Toggled(&'static str),
    Add,
    React,
    Reply,
    More,
    Typed(String),
    Cleared,
    Tone(SkinTone),
    Section(PickerSection),
    Picked(PickedEmoji),
}

type El<'a> = Element<'a, Did, Theme, Renderer>;

fn emoji_id(name: &str) -> EmojiId {
    EmojiId::parse(name).unwrap()
}

/// grace's message's chips on the canvas: 👍 3 (one's own), 🎲 1, 😂 2, and the server's :owlwave: 2.
fn chips() -> El<'static> {
    let chip = |emoji: ReactionEmoji, count, mine, key| ReactionChip { emoji, count, mine, toggle: Did::Toggled(key) };
    reactions(
        vec![
            chip(ReactionEmoji::Unicode("👍".to_string()), 3, true, "thumbs"),
            chip(ReactionEmoji::Unicode("🎲".to_string()), 1, false, "die"),
            chip(ReactionEmoji::Unicode("😂".to_string()), 2, false, "joy"),
            chip(ReactionEmoji::Custom { name: OWLWAVE.to_string(), picture: None }, 2, false, OWLWAVE),
        ],
        Some((label("Add a reaction"), Did::Add)),
        &WORDS,
    )
}

fn tools() -> El<'static> {
    hover_bar(
        HoverTools { react: Some(Did::React), reply: Some(Did::Reply), more: Did::More, menu_open: false },
        &WORDS,
    )
}

fn server_emoji() -> Vec<ServerEmoji> {
    [OWLWAVE, "owl_sleepy", "party_owl", "gold_die", "red_die", "trophy", "tomato", "owl_heart", "night", PIZZA]
        .into_iter()
        .map(|name| ServerEmoji { id: emoji_id(name), name: name.to_string(), picture: None })
        .collect()
}

fn picker<'a>(custom: &'a [ServerEmoji], recent: &'a [PickedEmoji], shown: PickerSection, query: &'a str) -> El<'a> {
    static NAME: std::sync::LazyLock<design::Label> = std::sync::LazyLock::new(|| label("Night Owls"));
    emoji_picker(
        EmojiPicker { server: &NAME, server_colour: OWLS, custom, recent, query, tone: SkinTone::Default, shown },
        EmojiSends {
            query: Box::new(Did::Typed),
            clear: Did::Cleared,
            tone: Box::new(Did::Tone),
            section: Box::new(Did::Section),
            pick: Box::new(Did::Picked),
        },
        &WORDS,
    )
}

fn recent() -> Vec<PickedEmoji> {
    let mut picked: Vec<PickedEmoji> =
        ["👍", "🎲", "😂", "🎉", "❤️"].iter().filter_map(|each| emoji::known(each)).map(PickedEmoji::Unicode).collect();
    picked.push(PickedEmoji::Custom(emoji_id(OWLWAVE)));
    picked
}

fn on_raised(element: El<'_>) -> El<'_> {
    container(element)
        .padding(Space::S.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

const CHIPS: Size = Size::new(360.0, 56.0);
const PICKER: Size = Size::new(340.0, 440.0);

#[test]
fn the_chips_and_the_tools_are_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let both = Column::with_children([chips(), tools()]).spacing(Space::S.px());
        let image = testing::render::<Did>(&theme, on_raised(both._boxed()), CHIPS + Size::new(0.0, 48.0), 1.0);
        ui::refs!().assert(&format!("chat-reactions-chips-{scheme:?}").to_lowercase(), &image);
    }
}

#[test]
fn the_picker_on_recent_and_on_the_servers_tab_is_the_reference() {
    let custom = server_emoji();
    let recent = recent();
    for (state, shown) in [("chat-reactions", PickerSection::Recent), ("chat-reactions-custom", PickerSection::Server)]
    {
        for scheme in Scheme::ALL {
            let theme = Theme::new(scheme, Accent::DEFAULT);
            let image = testing::render::<Did>(&theme, on_raised(picker(&custom, &recent, shown, "")), PICKER, 1.0);
            ui::refs!().assert(&format!("{state}-picker-{scheme:?}").to_lowercase(), &image);
        }
    }
}

/// A press on one's own chip takes it back, on another's adds it, and the chips say who gave them.
#[test]
fn a_chip_sends_its_toggle_and_says_its_count() {
    let mut sim = sim_sized(on_raised(chips()), CHIPS);
    let found = boxes(&mut sim);
    let said: Vec<&str> = found.iter().map(|(each, _)| each.label.as_str()).collect();
    assert!(said.contains(&"👍: 3, yours among them"), "{said:?}");
    assert!(said.contains(&":owlwave:: 2"), "{said:?}");
    for (name, sent) in [("👍: 3, yours among them", Did::Toggled("thumbs")), ("🎲: 1", Did::Toggled("die"))] {
        let at = found.iter().find(|(each, _)| each.label.as_str() == name).map(|(_, at)| at.center()).unwrap();
        sim.point_at(at);
        sim.simulate(testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [sent]);
    }
}

#[test]
fn each_tool_sends_its_own() {
    let mut sim = sim_sized(on_raised(tools()), Size::new(160.0, 60.0));
    let found = boxes(&mut sim);
    for (name, sent) in [("React", Did::React), ("Reply", Did::Reply), ("More", Did::More)] {
        let at = found.iter().find(|(each, _)| each.label.as_str() == name).map(|(_, at)| at.center()).unwrap();
        sim.point_at(at);
        sim.simulate(testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [sent], "{name}");
    }
}

/// A tab starts the grid at its section, an emoji picks itself in the tone shown, the server's own by its id, and the
/// tone button offers the next tone.
#[test]
fn the_picker_sends_a_section_a_pick_and_the_next_tone() {
    let custom = server_emoji();
    let recent = recent();
    let mut sim = sim_sized(on_raised(picker(&custom, &recent, PickerSection::Recent, "")), PICKER);
    let found = boxes(&mut sim);
    let at =
        |name: &str| found.iter().find(|(each, _)| each.label.as_str() == name).map(|(_, at)| at.center()).unwrap();
    let thumbs = emoji::known("👍").unwrap();
    for (name, sent) in [
        ("Smileys", Did::Section(PickerSection::Group(Group::SmileysAndEmotion))),
        ("Night Owls", Did::Section(PickerSection::Server)),
        (thumbs.name(), Did::Picked(PickedEmoji::Unicode(thumbs))),
        (":owlwave:", Did::Picked(PickedEmoji::Custom(emoji_id(OWLWAVE)))),
        ("Skin tone", Did::Tone(SkinTone::Light)),
    ] {
        sim.point_at(at(name));
        sim.simulate(testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [sent], "{name}");
    }
}

/// While something is typed the grid holds only what matches, the server's own first, and says so when nothing does.
#[test]
fn a_search_shows_only_its_matches() {
    let custom = server_emoji();
    let mut sim = sim_sized(on_raised(picker(&custom, &[], PickerSection::Recent, "piz")), PICKER);
    let said: Vec<String> = boxes(&mut sim).into_iter().map(|(each, _)| each.label.as_str().to_string()).collect();
    let pizza = emoji::known("🍕").unwrap().name().to_string();
    let first = said.iter().position(|name| name == ":pizza:").expect("the server's pizza");
    let second = said.iter().position(|name| *name == pizza).expect("unicode's pizza");
    assert!(first < second, "{said:?}");
    assert!(!said.iter().any(|name| name == ":owlwave:"), "{said:?}");
    let mut none = sim_sized(on_raised(picker(&custom, &[], PickerSection::Recent, "zzzzzz")), PICKER);
    let image = testing::render::<Did>(
        &Theme::new(Scheme::Dark, Accent::DEFAULT),
        on_raised(picker(&custom, &[], PickerSection::Recent, "zzzzzz")),
        PICKER,
        1.0,
    );
    ui::refs!().assert("chat-reactions-picker-none-dark", &image);
    assert!(boxes(&mut none).iter().all(|(each, _)| !each.label.as_str().starts_with(':')));
}
