//! Keyboard focus and the button, without a window: a simulator over the software renderer sends the
//! events a keyboard and a mouse would, and the messages and the focus are read back.

use crate::common;

use common::{Plain, focusable_total, focused, label, press, semantics, sim, tab};
use design::{ButtonVariant, SemanticRole};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Widget};
use iced_widget::Column;
use ui::testing::Renderer;
use ui::{Button, FocusScope, Theme};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pressed {
    First,
    Second,
    Third,
}

type El<'a> = Element<'a, Pressed, Theme, Renderer>;

fn buttons<'a>(second: Option<Pressed>, loading_third: bool) -> FocusScope<Column<El<'a>>> {
    let first = Button::new(label("First")).on_press(Pressed::First).variant(ButtonVariant::Primary).view::<Renderer>();
    let mut second_button = Button::new(label("Second"));
    if let Some(message) = second {
        second_button = second_button.on_press(message);
    }
    let third = Button::new(label("Third")).on_press(Pressed::Third).loading(loading_third).view::<Renderer>();
    FocusScope::new(Column::with_children([first, second_button.view::<Renderer>(), third]).width(Length::Fill))
}

#[test]
fn tab_goes_through_the_buttons_in_view_order_and_wraps_both_ways() {
    let mut sim = sim(buttons(Some(Pressed::Second), false));
    assert_eq!(focusable_total(&mut sim), 3);
    assert_eq!(focused(&mut sim), None, "nothing has the focus until the keyboard asks");
    let forward: Vec<_> = (0..4)
        .map(|_| {
            tab(&mut sim, false);
            focused(&mut sim)
        })
        .collect();
    assert_eq!(forward, [Some(0), Some(1), Some(2), Some(0)], "wraps past the last");
    let backward: Vec<_> = (0..2)
        .map(|_| {
            tab(&mut sim, true);
            focused(&mut sim)
        })
        .collect();
    assert_eq!(backward, [Some(2), Some(1)]);
}

#[test]
fn shift_tab_from_nothing_goes_to_the_last() {
    let mut sim = sim(buttons(Some(Pressed::Second), false));
    tab(&mut sim, true);
    assert_eq!(focused(&mut sim), Some(2));
}

#[test]
fn space_and_enter_press_only_the_focused_button() {
    let mut sim = sim(buttons(Some(Pressed::Second), false));
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().count(), 0, "a key sent while nothing has the focus presses nothing");
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    press(&mut sim, Named::Enter);
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed::First, Pressed::First, Pressed::Second]);
}

#[test]
fn a_disabled_button_is_skipped_and_never_pressed() {
    let mut sim = sim(buttons(None, false));
    assert_eq!(focusable_total(&mut sim), 2, "the one with no message is not reachable");
    tab(&mut sim, false);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed::Third], "Tab went from the first to the third");
    let _ = sim.click(Plain("Second"));
    assert_eq!(sim.drain().count(), 0, "a click on a disabled button sends nothing");
}

#[test]
fn a_loading_button_takes_no_input_and_says_so() {
    let mut sim = sim(buttons(Some(Pressed::Second), true));
    assert_eq!(focusable_total(&mut sim), 2);
    let _ = sim.click(Plain("Third\u{2026}"));
    assert_eq!(sim.drain().count(), 0, "its action is already under way");
}

#[test]
fn a_click_presses_a_button_and_a_key_does_not_press_another() {
    let mut sim = sim(buttons(Some(Pressed::Second), false));
    let _ = sim.click(Plain("Second"));
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed::Second]);
    assert_eq!(focused(&mut sim), Some(1), "a click gives the button the focus");
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed::Second], "and the keyboard then acts on it");
}

#[test]
fn every_control_says_what_it_is() {
    let mut sim = sim(buttons(Some(Pressed::Second), false));
    let found = semantics(&mut sim);
    let names: Vec<_> = found.iter().map(|each| each.label.as_str()).collect();
    assert_eq!(names, ["First", "Second", "Third"]);
    assert!(found.iter().all(|each| each.role == SemanticRole::Button));
}

#[test]
fn a_pointer_press_is_neither_a_stop_nor_a_control_and_still_takes_the_press() {
    let picture: El<'_> = ui::text("Picture", design::TypeStyle::Body)._boxed();
    let name = Button::new(label("Name")).on_press(Pressed::First).view::<Renderer>();
    let both = Column::with_children([ui::pointer_press(picture, Pressed::Second), name]);
    let mut sim = sim(FocusScope::new(both));
    assert_eq!(focusable_total(&mut sim), 1, "only the name is in the focus order");
    let found = semantics(&mut sim);
    let names: Vec<_> = found.iter().map(|each| each.label.as_str()).collect();
    assert_eq!(names, ["Name"], "and the only control a screen reader hears");
    let _ = sim.click(Plain("Picture"));
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed::Second]);
}
