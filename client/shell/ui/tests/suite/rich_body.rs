//! A message's styled body: every markdown style, each kind of mark and a server's emoji in its line drawn as the
//! canvas's chat.format draws them, a spoiler hidden and shown, a press on a mark sending what it names, and a server
//! emoji selected and copied as its `:name:`.

use crate::common;

use common::{Plain, sim};
use design::{Accent, MessageSize, Scheme, Space, Surface};
use iced_core::widget::operation::{self, Operation};
use iced_core::{Element, Length, Point, Rectangle, Size, Widget, image};
use iced_widget::{Column, container};
use screens::chat::{Body, EmojiId, Mark, MarkKind, SpoilerId};
use terms::{ChannelId, MemberId, RoleId};
use ui::testing::{self, Renderer};
use ui::{BodyPress, RichBody, Surfaced, Theme};

/// The English the bodies are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const GRACE: &str = "grace";
const REGULARS: &str = "regulars";
const GAME_NIGHT: &str = "game-night";
const OWL: &str = "owl";
/// The canvas's Regulars.
const REGULARS_COLOUR: design::Rgb = design::Rgb::hex(0xE2A6FF);
const SIZE: Size = Size::new(640.0, 440.0);

/// The mark of `kind` over the first `needle` in `text`, counted in scalars as the wire counts them.
fn mark(text: &str, needle: &str, kind: MarkKind) -> Mark {
    let at = text.find(needle).expect("the needle is in the text");
    let start = text.char_indices().take_while(|(index, _)| *index < at).count();
    Mark { range: start..start + needle.chars().count(), kind }
}

/// The canvas's chat.format: every style and every kind of mark, one message each.
fn format() -> Vec<Body> {
    let styled = "**Saturday, 20:00 sharp.** *Bring snacks*, __no spoilers__, and it’s ~~19:30~~ 20:00 now. \
                  Roll with `/roll 2d6`.";
    let block = "house rules, so nobody argues:\n```\ndoubles roll again\nthree doubles: skip a turn\n```";
    let quoted = "> doubles roll again\nthat one always starts a fight :owl:";
    let everyone = "@everyone the game moves to #game-night this week, @Regulars you host";
    let here = "@here the twist is ||the owl did it||";
    let linked = "@grace full rules: https://games.example.invalid/rules";
    vec![
        Body::plain(styled),
        Body::plain(block),
        Body::new(quoted, vec![mark(quoted, ":owl:", MarkKind::CustomEmoji(EmojiId::parse(OWL).unwrap()))]).unwrap(),
        Body::new(
            everyone,
            vec![
                mark(everyone, "@everyone", MarkKind::Everyone),
                mark(everyone, "#game-night", MarkKind::Channel(ChannelId::parse(GAME_NIGHT).unwrap())),
                mark(everyone, "@Regulars", MarkKind::Role(RoleId::parse(REGULARS).unwrap())),
            ],
        )
        .unwrap(),
        Body::new(here, vec![mark(here, "@here", MarkKind::Here)]).unwrap(),
        Body::new(
            linked,
            vec![
                mark(linked, "@grace", MarkKind::Mention(MemberId::parse(GRACE).unwrap())),
                mark(linked, "https://games.example.invalid/rules", MarkKind::Link),
            ],
        )
        .unwrap(),
    ]
}

/// A 16 px owl-orange square with a dark eye: a picture whose colours are its own.
fn owl() -> image::Handle {
    const SIDE: u32 = 16;
    let pixels = (0..SIDE * SIDE)
        .flat_map(|at| {
            let (x, y) = (at % SIDE, at / SIDE);
            if (5..8).contains(&x) && (5..8).contains(&y) { [0x22, 0x1A, 0x26, 0xFF] } else { [0xC2, 0x6A, 0x2E, 0xFF] }
        })
        .collect::<Vec<u8>>();
    image::Handle::from_rgba(SIDE, SIDE, pixels)
}

fn the_spoiler(body: &Body) -> SpoilerId {
    body.runs().find_map(|run| run.style.spoiler).expect("a spoiler")
}

fn body<'a>(body: &'a Body, revealed: &'a [SpoilerId], picture: Option<image::Handle>) -> RichBody<'a, BodyPress> {
    let regulars = RoleId::parse(REGULARS).unwrap();
    RichBody::new(body, MessageSize::DEFAULT, &WORDS)
        .roles(move |role| (*role == regulars).then_some(REGULARS_COLOUR))
        .emoji(move |_| picture.clone())
        .revealed(|spoiler| revealed.contains(&spoiler))
        .channels(|channel| channel.as_str() == GAME_NIGHT)
        .on_press(Some)
}

fn page<'a>(bodies: &'a [Body], revealed: &'a [SpoilerId]) -> Element<'a, BodyPress, Theme, Renderer> {
    let rows = bodies.iter().map(|each| body(each, revealed, Some(owl())).view::<Renderer>());
    container(Column::with_children(rows).spacing(Space::L.px()))
        .padding(Space::L.px())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Base))
        ._boxed()
}

/// Every style and mark, the spoiler hidden, then shown, in both schemes.
#[test]
fn every_style_and_mark_is_the_reference_with_the_spoiler_hidden_and_shown() {
    let bodies = format();
    let spoiler = [the_spoiler(&bodies[4])];
    for (name, revealed) in [("chat-format", &[][..]), ("chat-format-spoiler", &spoiler[..])] {
        for scheme in Scheme::ALL {
            let theme = Theme::new(scheme, Accent::DEFAULT);
            let image = testing::render::<BodyPress>(&theme, page(&bodies, revealed), SIZE, 1.0);
            ui::refs!().assert(&format!("{name}-{scheme:?}").to_lowercase(), &image);
        }
    }
}

/// A server emoji whose picture is not held yet stands as its `:name:`, in its line.
#[test]
fn a_server_emoji_without_its_picture_is_its_name() {
    let bodies = format();
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let shown = container(body(&bodies[2], &[], None).view::<Renderer>()).padding(Space::L.px());
        let image = testing::render::<BodyPress>(&theme, shown, Size::new(420.0, 90.0), 1.0);
        ui::refs!().assert(&format!("chat-format-emoji-name-{scheme:?}").to_lowercase(), &image);
    }
}

/// A press on a body made of one mark sends what the mark names.
fn pressed(text: &str, kind: MarkKind) -> Vec<BodyPress> {
    let one = Body::new(text, vec![mark(text, text, kind)]).unwrap();
    let mut sim = sim(body(&one, &[], None).view::<Renderer>());
    sim.point_at(Point::new(Space::M.px(), Space::S.px()));
    sim.simulate(testing::click());
    sim.drain().collect()
}

#[test]
fn a_press_on_a_mark_sends_what_it_names() {
    let grace = MemberId::parse(GRACE).unwrap();
    assert_eq!(pressed("@grace", MarkKind::Mention(grace.clone())), [BodyPress::Person(grace)]);
    let channel = ChannelId::parse(GAME_NIGHT).unwrap();
    assert_eq!(pressed("#game-night", MarkKind::Channel(channel.clone())), [BodyPress::Channel(channel)]);
    let role = RoleId::parse(REGULARS).unwrap();
    assert_eq!(pressed("@Regulars", MarkKind::Role(role.clone())), [BodyPress::Role(role)]);
    let link = "https://games.example.invalid/rules";
    assert_eq!(pressed(link, MarkKind::Link), [BodyPress::Link(link.to_string())]);
    assert!(pressed("@everyone", MarkKind::Everyone).is_empty(), "everyone names no one to open");
}

/// A mark of a channel the reader cannot open is its text alone: drawn as the plain text is, and a press on it sends
/// nothing.
#[test]
fn a_channel_mark_the_reader_cannot_open_is_its_text_alone() {
    const HIDDEN: &str = "staff-room";
    let text = "#staff-room";
    assert!(pressed(text, MarkKind::Channel(ChannelId::parse(HIDDEN).unwrap())).is_empty(), "no press");
    let marked = Body::new(text, vec![mark(text, text, MarkKind::Channel(ChannelId::parse(HIDDEN).unwrap()))]).unwrap();
    let plain = Body::plain(text);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let drawn = |each: &Body| {
            let shown = container(body(each, &[], None).view::<Renderer>()).padding(Space::L.px());
            testing::render::<BodyPress>(&theme, shown, Size::new(240.0, 60.0), 1.0)
        };
        assert!(drawn(&marked) == drawn(&plain), "{scheme:?}: no halo and no link colour");
    }
}

/// A link of a scheme the window does not open, as a stored 1.0 invite's, is its text alone: drawn as plain text is,
/// and a press on it sends nothing.
#[test]
fn a_link_the_window_cannot_open_is_its_text_alone() {
    let text = "chatkord://games.example.invalid/join/code";
    assert!(pressed(text, MarkKind::Link).is_empty(), "no press");
    let marked = Body::new(text, vec![mark(text, text, MarkKind::Link)]).unwrap();
    let plain = Body::plain(text);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let drawn = |each: &Body| {
            let shown = container(body(each, &[], None).view::<Renderer>()).padding(Space::L.px());
            testing::render::<BodyPress>(&theme, shown, Size::new(420.0, 60.0), 1.0)
        };
        assert!(drawn(&marked) == drawn(&plain), "{scheme:?}: no underline and no link colour");
    }
}

/// A hidden spoiler is pressed to be shown, and gives a reader the word for it, not its words, until it is.
#[test]
fn a_hidden_spoiler_is_pressed_to_show_and_keeps_its_words_from_a_reader() {
    let hidden = Body::plain("||the owl did it||");
    let spoiler = the_spoiler(&hidden);
    let mut closed = sim(body(&hidden, &[], None).view::<Renderer>());
    assert!(closed.find(Plain("the owl did it")).is_err(), "a reader is not told a hidden spoiler");
    assert!(closed.find(Plain("Spoiler, hidden. Press to show it.")).is_ok());
    closed.point_at(Point::new(Space::M.px(), Space::S.px()));
    closed.simulate(testing::click());
    assert_eq!(closed.drain().collect::<Vec<_>>(), [BodyPress::Spoiler(spoiler)]);
    let revealed = [spoiler];
    let mut open = sim(body(&hidden, &revealed, None).view::<Renderer>());
    assert!(open.find(Plain("the owl did it")).is_ok(), "a shown spoiler's words are read");
    open.point_at(Point::new(Space::M.px(), Space::S.px()));
    open.simulate(testing::click());
    assert_eq!(open.drain().count(), 0, "a shown spoiler asks nothing more");
}

/// What every text in the interface copies once all of it is selected.
fn copied<'a>(sim: &mut common::Sim<'a, BodyPress>) -> Vec<String> {
    struct Copy(Vec<String>);
    impl Operation for Copy {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, text: &mut dyn operation::Text) {
            text.select_all();
            self.0.extend(text.copy());
        }
    }
    let mut copy = Copy(Vec::new());
    sim.operate(&mut copy);
    copy.0
}

/// A server emoji drawn as its picture is read, selected and copied as its `:name:`, whole.
#[test]
fn a_server_emoji_is_read_and_copied_as_its_name() {
    let text = "hi :owl: there";
    let with_owl =
        Body::new(text, vec![mark(text, ":owl:", MarkKind::CustomEmoji(EmojiId::parse(OWL).unwrap()))]).unwrap();
    let mut pictured = sim(body(&with_owl, &[], Some(owl())).view::<Renderer>());
    assert!(pictured.find(text).is_ok(), "a reader is told the name");
    assert_eq!(copied(&mut pictured), [text]);
    let mut named = sim(body(&with_owl, &[], None).view::<Renderer>());
    assert_eq!(copied(&mut named), [text], "the same without its picture");
}

/// The picture stands in the line where the name would be: the line's height is the line's, and the picture is in
/// its own colours, beside the words.
#[test]
fn a_server_emojis_picture_is_painted_in_its_line() {
    let text = "a :owl: b";
    let with_owl =
        Body::new(text, vec![mark(text, ":owl:", MarkKind::CustomEmoji(EmojiId::parse(OWL).unwrap()))]).unwrap();
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let line = MessageSize::DEFAULT.line_height(design::TypeStyle::Message);
    let shown = container(body(&with_owl, &[], Some(owl())).view::<Renderer>()).class(Surfaced::On(Surface::Base));
    let image = testing::render::<BodyPress>(&theme, shown, Size::new(120.0, line), 1.0);
    let orange = image.rgba.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [0xC2, 0x6A, 0x2E, 0xFF]).count();
    // A 19.25 px picture less its eye covers more than 300 pixels; a missing one none.
    assert!(orange > 300, "only {orange} pixels of the picture");
}
