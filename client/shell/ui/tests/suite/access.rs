//! The tree a screen reader is handed, made from what the components say of themselves: where each control is, under
//! what, which has the focus, what is off or open, how news is read out, and ids that hold from one turn to the next.

use crate::common;

use accesskit::{Action, Live, NodeId, Rect, Role, Toggled, TreeUpdate};
use common::{access_tree, boxes, label, node_named, parent_of, settle, sim, sim_sized, tab, well_formed};
use design::{CallControl, Popup, Status};
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::{Column, scrollable};
use ui::testing::Renderer;
use ui::{Button, CallButton, CalloutParts, FocusScope, Icon, Theme, callout, dialog, dialog_panel, slider, switch};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const WINDOW: Size = Size::new(480.0, 360.0);

type El<'a> = Element<'a, Msg, Theme, Renderer>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Press,
    Slid(u16),
}

fn button<'a>(name: &str) -> El<'a> {
    Button::new(label(name)).on_press(Msg::Press).view()
}

/// Mute, a camera when `camera`, Deafen, and Leave with nothing to do yet.
fn bar<'a>(camera: bool) -> El<'a> {
    let mut controls = vec![button("Mute")];
    controls.extend(camera.then(|| button("Camera")));
    controls.extend([button("Deafen"), Button::new(label("Leave")).view()]);
    FocusScope::new(Column::with_children(controls))._boxed()
}

fn id_of(tree: &TreeUpdate, name: &str) -> NodeId {
    node_named(tree, name).0
}

#[test]
fn a_control_keeps_its_id_from_turn_to_turn_and_when_a_sibling_comes() {
    let first = access_tree(&mut sim_sized(bar(false), WINDOW), WINDOW);
    let again = access_tree(&mut sim_sized(bar(false), WINDOW), WINDOW);
    let grown = access_tree(&mut sim_sized(bar(true), WINDOW), WINDOW);
    for tree in [&first, &again, &grown] {
        well_formed(tree);
    }
    assert_eq!(first.tree.as_ref().map(|info| info.root), grown.tree.as_ref().map(|info| info.root));
    for name in ["Mute", "Deafen", "Leave"] {
        assert_eq!(id_of(&first, name), id_of(&again, name), "{name}: the same from one turn to the next");
        assert_eq!(id_of(&first, name), id_of(&grown, name), "{name}: the same with a camera beside it");
    }
}

#[test]
fn controls_alike_in_kind_and_name_each_have_an_id_of_their_own() {
    let mut sim = sim_sized(Column::with_children([button("Remove"), button("Remove")]), WINDOW);
    let tree = access_tree(&mut sim, WINDOW);
    well_formed(&tree);
    let removes: Vec<_> = tree.nodes.iter().filter(|(_, node)| node.label() == Some("Remove")).collect();
    assert_eq!(removes.len(), 2);
    assert_ne!(removes[0].0, removes[1].0);
}

#[test]
fn a_control_is_under_what_holds_it_and_where_it_is_drawn() {
    let panel = dialog_panel::<Msg, Renderer>(
        label("Rename the channel"),
        ui::text("The new name shows to everyone.", design::TypeStyle::Body)._boxed(),
        vec![button("Rename")],
        Msg::Press,
        &WORDS,
    );
    let mut sim = sim_sized(dialog(Column::<El<'_>>::new()._boxed(), Some(panel), Msg::Press), WINDOW);
    settle(&mut sim);
    let tree = access_tree(&mut sim, WINDOW);
    well_formed(&tree);
    let (dialog_id, dialog_node) = node_named(&tree, "Rename the channel");
    assert_eq!(dialog_node.role(), Role::Dialog);
    let (rename, node) = node_named(&tree, "Rename");
    assert_eq!(parent_of(&tree, *rename), Some(*dialog_id), "the dialog's button is under the dialog");
    let drawn =
        boxes(&mut sim).into_iter().find(|(said, _)| said.label.as_str() == "Rename").map(|(_, at)| at).unwrap();
    let at = Rect::new(
        f64::from(drawn.x),
        f64::from(drawn.y),
        f64::from(drawn.x + drawn.width),
        f64::from(drawn.y + drawn.height),
    );
    assert_eq!(node.bounds(), Some(at), "in the window's logical pixels");
    let scaled = ui::access::tree(WINDOW, 1.5, Some(&label("Chatkord")), |operation| sim.operate(operation));
    let root = scaled.tree.as_ref().unwrap().root;
    let (_, window) = scaled.nodes.iter().find(|(id, _)| *id == root).unwrap();
    assert_eq!(window.role(), Role::Window);
    assert_eq!(window.label(), Some("Chatkord"));
    assert_eq!(window.transform(), Some(&accesskit::Affine::scale(1.5)), "the window's scale makes them physical");
}

#[test]
fn the_focused_control_is_the_trees_focus_and_one_with_nothing_to_do_is_disabled() {
    let mut sim = sim_sized(bar(false), WINDOW);
    let tree = access_tree(&mut sim, WINDOW);
    assert_eq!(tree.focus, tree.tree.as_ref().unwrap().root, "nothing has the focus: the window has it");
    tab(&mut sim, false);
    let tree = access_tree(&mut sim, WINDOW);
    assert_eq!(tree.focus, id_of(&tree, "Mute"));
    let (_, mute) = node_named(&tree, "Mute");
    assert!(mute.supports_action(Action::Focus) && !mute.is_disabled());
    let (_, leave) = node_named(&tree, "Leave");
    assert!(leave.is_disabled(), "built with nothing to send");
    assert!(!leave.supports_action(Action::Focus), "and passed over by Tab");
}

#[test]
fn a_failure_is_read_out_at_once_and_other_news_in_its_turn() {
    let english = words::Words::english();
    let boxed = |kind, said: &str| {
        let shown = screens::Callout::new(words::Sentence::Said(said.into())).of(kind);
        callout::<Msg, Renderer>(&shown, CalloutParts::default(), &english)
    };
    let said = Column::with_children([
        boxed(Status::Danger, "Could not join Lounge."),
        boxed(Status::Info, "Lounge is full."),
    ]);
    let tree = access_tree(&mut sim_sized(said, WINDOW), WINDOW);
    let (_, failure) = node_named(&tree, "Could not join Lounge.");
    assert_eq!((failure.role(), failure.live()), (Role::Alert, Some(Live::Assertive)));
    let (_, news) = node_named(&tree, "Lounge is full.");
    assert_eq!((news.role(), news.live()), (Role::Status, Some(Live::Polite)));
}

#[test]
fn a_switch_says_whether_it_is_on_and_a_slider_where_it_stands() {
    let controls = Column::with_children([
        switch::<Msg, Renderer>(&label("Noise suppression"), None, true, Msg::Press),
        slider::<Msg, Renderer>(label("Volume"), 0..=200, 80, Msg::Slid, Msg::Slid),
    ]);
    let tree = access_tree(&mut sim_sized(controls, WINDOW), WINDOW);
    let (_, on) = node_named(&tree, "Noise suppression");
    assert_eq!((on.role(), on.toggled()), (Role::Switch, Some(Toggled::True)));
    let (_, volume) = node_named(&tree, "Volume");
    assert_eq!(volume.role(), Role::Slider);
    assert_eq!(
        (volume.numeric_value(), volume.min_numeric_value(), volume.max_numeric_value()),
        (Some(80.0), Some(0.0), Some(200.0))
    );
}

#[test]
fn a_menu_check_says_whether_it_is_ticked() {
    for checked in [false, true] {
        let row = ui::ListRow::<Msg, Renderer>::new(label("Moderators")).menu_check(checked, None).on_press(Msg::Press);
        let tree = access_tree(&mut sim(row.view()), WINDOW);
        let (_, node) = node_named(&tree, "Moderators");
        let toggled = if checked { Toggled::True } else { Toggled::False };
        assert_eq!((node.role(), node.toggled()), (Role::MenuItemCheckBox, Some(toggled)));
    }
}

#[test]
fn a_control_that_opens_a_menu_says_whether_it_is_open() {
    let arrow = |open| {
        CallButton::new(label("Mute"), Icon::Mic, CallControl::Rest)
            .on_press(Msg::Press)
            .menu(label("Microphone and speaker"), Msg::Press, open)
            .view::<Renderer>()
    };
    for open in [false, true] {
        let tree = access_tree(&mut sim(arrow(open)), WINDOW);
        let (_, node) = node_named(&tree, "Microphone and speaker");
        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Menu));
        assert_eq!(node.is_expanded(), Some(open));
        let (_, mute) = node_named(&tree, "Mute");
        assert_eq!(mute.is_expanded(), None, "Mute itself opens nothing");
    }
    let bars = Button::new(label("Connection")).opens(Popup::Dialog, true).on_press(Msg::Press).view::<Renderer>();
    let tree = access_tree(&mut sim(bars), WINDOW);
    let (_, node) = node_named(&tree, "Connection");
    assert_eq!((node.has_popup(), node.is_expanded()), (Some(accesskit::HasPopup::Dialog), Some(true)));
}

#[test]
fn a_scrolled_control_is_placed_where_it_shows() {
    let list = || {
        let rows = (0..30).map(|row| button(&format!("Row {row}")));
        scrollable(Column::with_children(rows)).height(Length::Fixed(200.0))
    };
    let mut sim = sim_sized(list(), WINDOW);
    let before = node_named(&access_tree(&mut sim, WINDOW), "Row 10").1.bounds().unwrap();
    sim.point_at(Point::new(40.0, 100.0));
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::WheelScrolled {
        delta: iced_core::mouse::ScrollDelta::Pixels { x: 0.0, y: -120.0 },
    })]);
    settle(&mut sim);
    let after = node_named(&access_tree(&mut sim, WINDOW), "Row 10").1.bounds().unwrap();
    assert!(after.y0 < before.y0, "scrolled down, the row stands higher: {before:?} then {after:?}");
    assert!((after.height() - before.height()).abs() < f64::EPSILON);
}

#[test]
fn a_button_offers_click_and_its_id_maps_to_its_bounds() {
    let mut sim = sim_sized(bar(false), WINDOW);
    let (tree, controls) = ui::access::tree_with_controls(WINDOW, 1.0, None, |operation| sim.operate(operation));
    let (mute, node) = node_named(&tree, "Mute");
    assert!(node.supports_action(Action::Click) && node.supports_action(Action::Focus));
    let drawn = boxes(&mut sim).into_iter().find(|(said, _)| said.label.as_str() == "Mute").map(|(_, at)| at).unwrap();
    let control = controls.get(mute).expect("the node maps to its control");
    assert_eq!(control.bounds, drawn);
    assert_eq!(control.focus, Some(0), "the first focusable in the walk");
    let (deafen, _) = node_named(&tree, "Deafen");
    assert_eq!(controls.get(deafen).and_then(|control| control.focus), Some(1));
    let (leave, node) = node_named(&tree, "Leave");
    assert!(!node.supports_action(Action::Click), "nothing to do, so nothing to press");
    assert_eq!(controls.get(leave).and_then(|control| control.focus), None);
}

#[test]
fn plain_text_is_in_the_tree() {
    const SAID: &str = "The new name shows to everyone.";
    let said: El<'_> = ui::text(SAID, design::TypeStyle::Body)._boxed();
    let tree = access_tree(&mut sim_sized(Column::with_children([said]), WINDOW), WINDOW);
    well_formed(&tree);
    let (_, node) = tree.nodes.iter().find(|(_, node)| node.value() == Some(SAID)).expect("the text has a node");
    assert_eq!(node.role(), Role::Label);
}

#[test]
fn a_plain_field_s_value_is_what_it_holds_and_an_empty_one_has_none() {
    const HINT: &str = "Search members";
    let field = |value| -> El<'_> {
        ui::TextInput::new(label("Find"), value).placeholder(HINT).on_input(|_| Msg::Press).view()
    };
    let tree = access_tree(&mut sim_sized(Column::with_children([field("ada")]), WINDOW), WINDOW);
    assert_eq!(node_named(&tree, "Find").1.value(), Some("ada"));
    let tree = access_tree(&mut sim_sized(Column::with_children([field("")]), WINDOW), WINDOW);
    let (_, node) = node_named(&tree, "Find");
    assert_eq!(node.role(), Role::TextInput);
    assert!(node.value().is_none_or(str::is_empty), "{:?}", node.value());
    assert!(tree.nodes.iter().all(|(_, node)| node.value() != Some(HINT)), "the placeholder read as typed text");
}

#[test]
fn a_secret_field_s_text_is_never_in_the_tree() {
    const PASSWORD: &str = "hunter2";
    let secret = terms::Secret::new(PASSWORD.to_string());
    let field: El<'_> = ui::SecretInput::new(label("Password"), &secret).view();
    let tree = access_tree(&mut sim_sized(Column::with_children([field]), WINDOW), WINDOW);
    let (_, node) = node_named(&tree, "Password");
    assert_eq!(node.role(), Role::PasswordInput);
    for (_, node) in &tree.nodes {
        assert_ne!(node.value(), Some(PASSWORD));
        assert!(!node.label().is_some_and(|label| label.contains(PASSWORD)));
    }
}
