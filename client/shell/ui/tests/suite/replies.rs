//! A reply and a channel's pins as the canvas draws them: the bar over the composer with its mention's toggle, the
//! quoted line over a reply and over one whose original is gone, and the pinned messages' panel, each a reference in
//! both schemes; every control sends what it is for.

use crate::common;

use common::{boxes, semantics, sim_sized};
use design::{Accent, PersonColour, Scheme, SemanticRole, Space, Surface, Swatch};
use iced_core::{Element, Length, Size, Widget};
use iced_widget::container;
use ui::testing::{self, Renderer};
use ui::{Pin, Quoted, QuotedReply, Replying, Surfaced, Theme, pins_panel, quoted_reply, reply_bar};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const GRACE: &str = "grace";
const BO: &str = "Bo";

/// The composer's width on the canvas's chat board.
const COMPOSER: Size = Size::new(690.0, 60.0);
const QUOTED: Size = Size::new(690.0, 40.0);
const PANEL: Size = Size::new(420.0, 300.0);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    Toggle,
    Cancel,
    Jump(u8),
    Unpin(u8),
}

type El<'a> = Element<'a, Did, Theme, Renderer>;

fn colour(index: usize) -> PersonColour {
    PersonColour::Swatch(Swatch::ALL[index % Swatch::ALL.len()])
}

fn bar(mention: bool) -> El<'static> {
    reply_bar(Replying { name: GRACE.to_string(), mention, toggle: Did::Toggle, cancel: Did::Cancel }, &WORDS)
}

fn quoted(deleted: bool) -> El<'static> {
    let quoted = if deleted {
        Quoted::Deleted
    } else {
        Quoted::Live {
            name: BO.to_string(),
            colour: colour(1),
            picture: None,
            snippet: "yes! give me ten minutes".to_string(),
        }
    };
    quoted_reply(QuotedReply { quoted, jump: Did::Jump(0) }, &WORDS)
}

fn pins(may_unpin: bool) -> Vec<Pin<'static, Did, Renderer>> {
    let unpin = |at: u8| may_unpin.then_some(Did::Unpin(at));
    vec![
        Pin {
            name: BO.to_string(),
            avatar: ui::AvatarKind::Called(colour(1)),
            picture: None,
            at: "Today · 21:06".to_string(),
            text: "rules for the new one: https://games.example.invalid/rules".to_string(),
            body: ui::text("rules for the new one: https://games.example.invalid/rules", design::TypeStyle::Body)
                ._boxed(),
            jump: Did::Jump(1),
            unpin: unpin(1),
        },
        Pin {
            name: GRACE.to_string(),
            avatar: ui::AvatarKind::Called(colour(2)),
            picture: None,
            at: "Monday 9 November at 20:40".to_string(),
            text: "Game night is Saturdays at 20:00. Bring snacks, not spoilers.".to_string(),
            body: ui::text("Game night is Saturdays at 20:00. Bring snacks, not spoilers.", design::TypeStyle::Body)
                ._boxed(),
            jump: Did::Jump(2),
            unpin: unpin(2),
        },
    ]
}

/// `element` on the channel's surface, padded as the canvas's chat column pads the composer.
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
fn the_reply_bar_is_the_reference_with_the_mention_on_and_off() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        ui::refs!().assert(&format!("chat-reply-{scheme:?}").to_lowercase(), &picture(&theme, bar(true), COMPOSER));
        ui::refs!().assert(
            &format!("chat-reply-mention-off-{scheme:?}").to_lowercase(),
            &picture(&theme, bar(false), COMPOSER),
        );
    }
}

/// The toggle is a switch that says whether the author is mentioned; a press on it, and on the close, sends theirs.
#[test]
fn the_toggle_says_its_setting_and_the_bars_controls_send_theirs() {
    for on in [true, false] {
        let mut sim = sim_sized(bar(on), COMPOSER);
        let found = semantics(&mut sim);
        assert!(
            found
                .iter()
                .any(|each| each.role == SemanticRole::Switch { on } && each.label.as_str() == "Mention the author"),
            "{found:?}"
        );
    }
    assert_eq!(press_named(bar(true), COMPOSER, "Mention the author"), [Did::Toggle]);
    assert_eq!(press_named(bar(true), COMPOSER, "Cancel reply"), [Did::Cancel]);
}

#[test]
fn the_quoted_line_is_the_reference_live_and_deleted() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        ui::refs!()
            .assert(&format!("chat-reply-quoted-{scheme:?}").to_lowercase(), &picture(&theme, quoted(false), QUOTED));
        ui::refs!()
            .assert(&format!("chat-reply-deleted-{scheme:?}").to_lowercase(), &picture(&theme, quoted(true), QUOTED));
    }
}

/// A press anywhere on the quoted line jumps to what it quotes, whether or not that is still there.
#[test]
fn a_press_on_the_quoted_line_jumps() {
    assert_eq!(press_named(quoted(false), QUOTED, "Bo: yes! give me ten minutes"), [Did::Jump(0)]);
    assert_eq!(press_named(quoted(true), QUOTED, "Original message was deleted"), [Did::Jump(0)]);
}

#[test]
fn the_pins_panel_is_the_reference_with_unpin_and_without() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let panel = |may_unpin| pins_panel(pins(may_unpin), PANEL.height, &WORDS);
        ui::refs!().assert(&format!("chat-pins-{scheme:?}").to_lowercase(), &picture(&theme, panel(true), PANEL));
        ui::refs!()
            .assert(&format!("chat-pins-member-{scheme:?}").to_lowercase(), &picture(&theme, panel(false), PANEL));
        let empty = pins_panel::<Did, Renderer>(Vec::new(), PANEL.height, &WORDS);
        ui::refs!().assert(&format!("chat-pins-empty-{scheme:?}").to_lowercase(), &picture(&theme, empty, PANEL));
    }
}

/// Jump and Unpin send theirs for the pin they are on; who may not pin has no Unpin.
#[test]
fn jump_and_unpin_send_theirs_and_unpin_is_only_for_who_may_pin() {
    let mut sim = sim_sized(pins_panel(pins(true), PANEL.height, &WORDS), PANEL);
    let found = boxes(&mut sim);
    let centres: Vec<_> =
        found.iter().filter(|(each, _)| each.label.as_str() == "Unpin").map(|(_, at)| at.center()).collect();
    assert_eq!(centres.len(), 2);
    sim.point_at(centres[1]);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Did::Unpin(2)]);
    let jump = found.iter().find(|(each, _)| each.label.as_str() == "Jump").map(|(_, at)| at.center()).unwrap();
    sim.point_at(jump);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Did::Jump(1)]);

    let mut member = sim_sized(pins_panel(pins(false), PANEL.height, &WORDS), PANEL);
    assert!(semantics(&mut member).iter().all(|each| each.label.as_str() != "Unpin"));
}
