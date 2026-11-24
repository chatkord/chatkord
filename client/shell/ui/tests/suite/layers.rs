//! The dialog and the menu: layers over a screen that hold the events, the focus and what a screen
//! reader is told while they are open, and give the focus back when they close.

use crate::common;

use common::{Plain, boxes, focusable_total, focused, label, press, semantics, settle, sim_sized, tab};
use design::{Accent, ButtonVariant, Scheme, SemanticRole, Space, Surface};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::{Column, container, stack};
use ui::testing::{self, Renderer, click};
use ui::{
    Button, FocusScope, MenuChoice, MenuEntry, Select, Surfaced, Theme, dialog, dialog_panel, menu_at, menu_items,
    reveal, stats_popover, stats_popover_over,
};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Base(u8),
    Ok,
    Cancel,
    Close,
    Rename,
    Delete,
    Left,
}

type El<'a> = Element<'a, Msg, Theme, Renderer>;

const WINDOW: Size = Size::new(640.0, 480.0);

fn button<'a>(name: &str, message: Msg, variant: ButtonVariant) -> El<'a> {
    Button::new(label(name)).variant(variant).on_press(message).view()
}

/// The screen under a layer: two buttons at the top left.
fn base<'a>() -> El<'a> {
    base_with(false)
}

/// The same, a third button standing between them when `inserted`.
fn base_with<'a>(inserted: bool) -> El<'a> {
    let between = inserted.then(|| button("Inserted", Msg::Base(3), ButtonVariant::Secondary));
    Column::with_children(
        std::iter::once(button("First", Msg::Base(1), ButtonVariant::Secondary))
            .chain(between)
            .chain(std::iter::once(button("Second", Msg::Base(2), ButtonVariant::Secondary))),
    )
    .spacing(Space::S.px())
    .width(Length::Shrink)
    ._boxed()
}

fn panel<'a>() -> El<'a> {
    dialog_panel(
        label("Delete channel"),
        ui::text("This cannot be undone.".to_string(), design::TypeStyle::Body)._boxed(),
        vec![button("Cancel", Msg::Cancel, ButtonVariant::Secondary), button("Delete", Msg::Ok, ButtonVariant::Danger)],
        Msg::Close,
        &WORDS,
    )
}

fn screen<'a>(open: bool) -> impl Widget<Msg, Theme, Renderer> + 'a {
    crowded(open, false)
}

/// The screen with its dialog `open` or not, and a button `inserted` between its two.
fn crowded<'a>(open: bool, inserted: bool) -> impl Widget<Msg, Theme, Renderer> + 'a {
    let filling = container(base_with(inserted)).width(Length::Fill).height(Length::Fill);
    FocusScope::new(dialog(filling._boxed(), open.then(panel), Msg::Close))
}

#[test]
fn an_open_dialog_holds_the_focus_inside_it_and_wraps() {
    let mut sim = sim_sized(screen(true), WINDOW);
    settle(&mut sim);
    assert_eq!(focusable_total(&mut sim), 3, "only the dialog's own controls can be reached: its two and its ✕");
    assert_eq!(focused(&mut sim), Some(0), "the focus is in the dialog as soon as it opens, on its first button");
    let seen: Vec<_> = (0..4)
        .map(|_| {
            tab(&mut sim, false);
            focused(&mut sim)
        })
        .collect();
    assert_eq!(seen, [Some(1), Some(2), Some(0), Some(1)], "round and round inside it");
    tab(&mut sim, true);
    assert_eq!(focused(&mut sim), Some(0));
}

#[test]
fn its_close_button_closes_it() {
    let mut sim = sim_sized(screen(true), WINDOW);
    settle(&mut sim);
    tab(&mut sim, true);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Close]);
}

/// The same content in a plain stack, as a dialog would be without `Layered`, lets the Tab key walk out of it into
/// the screen under it.
#[test]
fn without_the_layer_the_tab_key_walks_out_of_a_dialog_into_the_screen() {
    let naive = FocusScope::new(stack([container(base()).width(Length::Fill).height(Length::Fill)._boxed(), panel()]));
    let mut sim = sim_sized(naive, WINDOW);
    assert_eq!(focusable_total(&mut sim), 5, "the screen's controls are reachable under a stack");
}

#[test]
fn keys_and_clicks_reach_only_the_dialog_while_it_is_open() {
    let mut sim = sim_sized(screen(true), WINDOW);
    settle(&mut sim);
    press(&mut sim, Named::Space);
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    // A click on the screen under it is a press outside: it closes, and reaches nothing.
    sim.point_at(Point::new(20.0, 20.0));
    sim.simulate(click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Cancel, Msg::Ok, Msg::Close]);
}

#[test]
fn escape_closes_it_and_a_stray_key_does_not_reach_the_screen() {
    let mut sim = sim_sized(screen(true), WINDOW);
    settle(&mut sim);
    press(&mut sim, Named::Escape);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Close, Msg::Cancel], "Space went to the dialog's focused button");
}

#[test]
fn closing_returns_the_focus_to_the_control_that_had_it() {
    let mut sim = sim_sized(screen(false), WINDOW);
    tab(&mut sim, false);
    tab(&mut sim, false);
    assert_eq!(focused(&mut sim), Some(1), "the second button of the screen has the focus");
    let mut sim = sim.rebuild(screen(true));
    settle(&mut sim);
    assert_eq!(focused(&mut sim), Some(0), "the focus moved into the dialog");
    let mut sim = sim.rebuild(screen(false));
    settle(&mut sim);
    assert_eq!(focused(&mut sim), Some(1), "and came back");
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Base(2)], "to the very control that had it");
}

#[test]
fn closing_returns_the_focus_to_the_control_itself_though_the_screen_changed_under_the_dialog() {
    let mut sim = sim_sized(screen(false), WINDOW);
    tab(&mut sim, false);
    tab(&mut sim, false);
    assert_eq!(focused(&mut sim), Some(1), "the second button of the screen has the focus");
    let mut sim = sim.rebuild(crowded(true, false));
    settle(&mut sim);
    let mut sim = sim.rebuild(crowded(true, true));
    settle(&mut sim);
    let mut sim = sim.rebuild(crowded(false, true));
    settle(&mut sim);
    assert_eq!(focused(&mut sim), Some(2), "Second, now the third button, not whichever stands where it stood");
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Base(2)]);
}

#[test]
fn a_focus_a_pointer_gave_stays_unringed_as_it_returns_to_its_control() {
    let mut sim = sim_sized(screen(false), WINDOW);
    let second = boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| each.label.as_str() == "Second")
        .map(|(_, bounds)| bounds.center())
        .expect("the second button");
    sim.point_at(second);
    sim.simulate(click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Base(2)]);
    assert_eq!(common::focus_shown(&mut sim), Some((1, false)), "focused by the pointer, its ring hidden");
    let mut sim = sim.rebuild(crowded(true, false));
    settle(&mut sim);
    let mut sim = sim.rebuild(crowded(false, true));
    settle(&mut sim);
    assert_eq!(common::focus_shown(&mut sim), Some((2, false)), "back on Second with the ring still hidden");
}

#[test]
fn a_screen_reader_hears_only_the_dialog_while_it_is_open() {
    let names = |open: bool| {
        let mut sim = sim_sized(screen(open), WINDOW);
        settle(&mut sim);
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect::<Vec<_>>()
    };
    assert_eq!(names(false), [(SemanticRole::Button, "First".into()), (SemanticRole::Button, "Second".into())]);
    assert_eq!(
        names(true),
        [
            (SemanticRole::Dialog, "Delete channel".into()),
            (SemanticRole::Button, "Cancel".into()),
            (SemanticRole::Button, "Delete".into()),
            (SemanticRole::Button, "Close".into()),
        ]
    );
}

#[test]
fn a_dialog_over_a_screen_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let on =
            |open| container(screen(open)).class(Surfaced::On(Surface::Base)).width(Length::Fill).height(Length::Fill);
        let closed = testing::render::<Msg>(&theme, on(false), WINDOW, 1.0);
        let open = testing::render::<Msg>(&theme, on(true), WINDOW, 1.0);
        ui::refs!().assert(&format!("dialog-{scheme:?}").to_lowercase(), &testing::side_by_side(&[closed, open]));
    }
}

fn items<'a>() -> El<'a> {
    menu_items(
        label("Lounge"),
        vec![
            MenuEntry { label: label("Rename"), message: Msg::Rename, danger: false, rule_before: false, icon: None },
            MenuEntry { label: label("Delete"), message: Msg::Delete, danger: true, rule_before: true, icon: None },
        ],
    )
}

/// A trigger and another control, with the menu under the trigger while `open`.
fn menu_screen<'a>(open: bool) -> impl Widget<Msg, Theme, Renderer> + 'a {
    let screen = Column::with_children([
        button("Channel options", Msg::Base(1), ButtonVariant::Secondary),
        button("Elsewhere", Msg::Base(2), ButtonVariant::Secondary),
    ])
    .width(Length::Fill)
    .height(Length::Fill);
    let under_trigger = iced_core::Vector::new(0.0, design::CONTROL_HEIGHT + Space::Xs.px());
    FocusScope::new(menu_at(screen._boxed(), open.then(items), under_trigger, Msg::Close))
}

#[test]
fn a_menu_is_reached_by_the_arrow_keys_and_closed_by_escape_or_a_press_outside() {
    let mut sim = sim_sized(menu_screen(true), WINDOW);
    settle(&mut sim);
    assert_eq!(focusable_total(&mut sim), 2, "its two items: the screen's controls are not reached while it is open");
    assert_eq!(focused(&mut sim), Some(0), "in the menu, on its first item");
    let arrow = |sim: &mut common::Sim<'_, Msg>, key| {
        sim.simulate([testing::key_press(iced_core::keyboard::Key::Named(key), false)]);
    };
    arrow(&mut sim, Named::ArrowDown);
    assert_eq!(focused(&mut sim), Some(1));
    arrow(&mut sim, Named::ArrowDown);
    assert_eq!(focused(&mut sim), Some(0), "wraps inside the menu and never leaves it for the control beside");
    arrow(&mut sim, Named::ArrowUp);
    assert_eq!(focused(&mut sim), Some(1));
    press(&mut sim, Named::Enter);
    press(&mut sim, Named::Escape);
    sim.point_at(Point::new(600.0, 400.0));
    sim.simulate(click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Msg::Delete, Msg::Close, Msg::Close]);
}

#[test]
fn a_menu_says_what_it_is_and_its_items_over_the_screen_and_the_focus_returns_to_its_trigger() {
    let mut sim = sim_sized(menu_screen(true), WINDOW);
    settle(&mut sim);
    let found: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    assert_eq!(
        found,
        [
            (SemanticRole::Button, "Channel options".into()),
            (SemanticRole::Button, "Elsewhere".into()),
            (SemanticRole::Menu, "Lounge".into()),
            (SemanticRole::MenuItem, "Rename".into()),
            (SemanticRole::MenuItem, "Delete".into()),
        ],
        "a menu is read over the screen, which a reader still reads, as the menu's opener says it is open"
    );
    let mut closed = sim_sized(menu_screen(false), WINDOW);
    tab(&mut closed, false);
    assert_eq!(focused(&mut closed), Some(0), "the trigger has it");
    let mut opened = closed.rebuild(menu_screen(true));
    settle(&mut opened);
    let mut closed = opened.rebuild(menu_screen(false));
    settle(&mut closed);
    assert_eq!(focused(&mut closed), Some(0), "back on the trigger");
}

#[test]
fn a_menu_under_its_trigger_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let on = |open| {
            container(menu_screen(open)).class(Surfaced::On(Surface::Base)).width(Length::Fill).height(Length::Fill)
        };
        let size = Size::new(360.0, 240.0);
        let open = testing::render::<Msg>(&theme, on(true), size, 1.0);
        ui::refs!().assert(&format!("menu-{scheme:?}").to_lowercase(), &open);
    }
}

const MENU_AT: iced_core::Vector = iced_core::Vector::new(64.0, 12.0);

/// The screen, with a menu over the whole of it at [`MENU_AT`] while `open`.
fn at_screen<'a>(open: bool) -> impl iced_core::Widget<Msg, Theme, Renderer> + 'a {
    FocusScope::new(menu_at(
        container(base()).width(Length::Fill).height(Length::Fill)._boxed(),
        open.then(items),
        MENU_AT,
        Msg::Close,
    ))
}

#[test]
fn a_menu_at_a_point_opens_there_and_the_focus_returns_to_the_control_that_had_it() {
    let mut sim = sim_sized(at_screen(true), WINDOW);
    settle(&mut sim);
    let (title, rename) = (sim.find(Plain("Lounge")).unwrap().bounds(), sim.find(Plain("Rename")).unwrap().bounds());
    assert!(
        rename.x >= MENU_AT.x && title.y >= MENU_AT.y && title.y < MENU_AT.y + Space::L.px(),
        "{title:?} {rename:?}"
    );
    assert!(rename.width > 200.0, "the menu's own width: {rename:?}");
    let mut closed = sim_sized(at_screen(false), WINDOW);
    tab(&mut closed, false);
    tab(&mut closed, false);
    assert_eq!(focused(&mut closed), Some(1));
    let mut opened = closed.rebuild(at_screen(true));
    settle(&mut opened);
    assert_eq!(focusable_total(&mut opened), 2, "only the menu's items: the screen's are shut away");
    let mut closed = opened.rebuild(at_screen(false));
    settle(&mut closed);
    assert_eq!(focused(&mut closed), Some(1), "back on the control that had it");
    press(&mut closed, Named::Space);
    assert_eq!(closed.drain().collect::<Vec<_>>(), [Msg::Base(2)]);
}

fn pointer_screen<'a>(open: bool) -> impl iced_core::Widget<Msg, Theme, Renderer> + 'a {
    FocusScope::new(ui::menu_at_pointer(
        container(base()).width(Length::Fill).height(Length::Fill)._boxed(),
        open.then(items),
        MENU_AT,
        Msg::Close,
    ))
}

/// A context menu opens where the pointer pressed, kept inside the screen near an edge, and at its keyboard point when
/// a key opened it.
#[test]
fn a_context_menu_opens_where_the_pointer_pressed_and_stays_inside_the_screen() {
    let opened_after = |prepare: &dyn Fn(&mut common::Sim<'_, Msg>)| {
        let mut closed = sim_sized(pointer_screen(false), WINDOW);
        prepare(&mut closed);
        let mut opened = closed.rebuild(pointer_screen(true));
        settle(&mut opened);
        boxes(&mut opened)
            .into_iter()
            .find(|(each, _)| each.role == SemanticRole::Menu)
            .map(|(_, bounds)| bounds)
            .expect("the menu is there")
    };
    let right_at = |at: Point| {
        move |sim: &mut common::Sim<'_, Msg>| {
            sim.point_at(at);
            sim.simulate(testing::right_click());
        }
    };
    let there = opened_after(&right_at(Point::new(300.0, 200.0)));
    assert!((there.x - 300.0).abs() < 1.0 && (there.y - 200.0).abs() < 1.0, "at the press: {there:?}");
    let corner = opened_after(&right_at(Point::new(WINDOW.width - 4.0, WINDOW.height - 4.0)));
    assert!(corner.x + corner.width <= WINDOW.width + 0.5, "not past the right edge: {corner:?}");
    assert!(corner.y + corner.height <= WINDOW.height + 0.5, "nor the bottom: {corner:?}");
    let keyed = opened_after(&|sim: &mut common::Sim<'_, Msg>| {
        right_at(Point::new(300.0, 200.0))(sim);
        press(sim, Named::Tab);
    });
    assert!((keyed.x - MENU_AT.x).abs() < 1.0 && (keyed.y - MENU_AT.y).abs() < 1.0, "a key acted last: {keyed:?}");
}

/// The screen, with the connection popover over the whole of it told to rise from `left` and `bottom`.
fn rising_screen<'a>(left: f32, bottom: f32) -> impl iced_core::Widget<Msg, Theme, Renderer> + 'a {
    FocusScope::new(stats_popover_over(
        container(base()).width(Length::Fill).height(Length::Fill)._boxed(),
        Some(stats_popover(
            label("Connection"),
            None,
            &screens::RoundTrips::default(),
            false,
            None,
            |_| Msg::Close,
            &WORDS,
        )),
        (left, bottom),
        Msg::Close,
    ))
}

#[test]
fn a_popover_rises_from_its_left_and_bottom_edges_and_stays_inside_the_window() {
    let popover = |left: f32, bottom: f32| {
        let mut sim = sim_sized(rising_screen(left, bottom), WINDOW);
        settle(&mut sim);
        boxes(&mut sim)
            .into_iter()
            .find(|(each, _)| each.role == SemanticRole::Dialog && each.label.as_str() == "Connection")
            .map(|(_, bounds)| bounds)
            .expect("the popover is there")
    };
    let rose = popover(64.0, 400.0);
    assert!((rose.x - 64.0).abs() < 1.0 && (rose.y + rose.height - 400.0).abs() < 1.0, "{rose:?}");
    assert!(rose.height > 100.0 && (rose.width - design::STATS_POPOVER_WIDTH).abs() < 1.0, "its own size: {rose:?}");
    let pushed = popover(WINDOW.width, 400.0);
    assert!(pushed.x + pushed.width <= WINDOW.width + 0.5, "not past the right edge: {pushed:?}");
    let low = popover(64.0, 20.0);
    assert!(low.y >= 0.0, "not above the top: {low:?}");
}

/// A menu a click opened marks no item and gives the focus back unmarked; one the keyboard opened marks its first
/// item and gives it back marked.
#[test]
fn a_menu_shows_its_focus_only_when_the_keyboard_opened_it() {
    let trigger = Point::new(40.0, design::CONTROL_HEIGHT / 2.0);
    let mut closed = sim_sized(menu_screen(false), WINDOW);
    closed.point_at(trigger);
    closed.simulate(click());
    let mut opened = closed.rebuild(menu_screen(true));
    settle(&mut opened);
    assert_eq!(common::focus_shown(&mut opened), Some((0, false)), "its first item has the focus, unmarked");
    let mut closed = opened.rebuild(menu_screen(false));
    settle(&mut closed);
    assert_eq!(common::focus_shown(&mut closed), Some((0, false)), "back on the trigger, unmarked");
    let mut closed = sim_sized(menu_screen(false), WINDOW);
    tab(&mut closed, false);
    press(&mut closed, Named::Enter);
    let mut opened = closed.rebuild(menu_screen(true));
    settle(&mut opened);
    assert_eq!(common::focus_shown(&mut opened), Some((0, true)), "its first item, marked");
    let mut closed = opened.rebuild(menu_screen(false));
    settle(&mut closed);
    assert_eq!(common::focus_shown(&mut closed), Some((0, true)), "back on the trigger, marked");
}

/// A menu taller than the least window scrolls inside it, the scroll no taller than the window: without it the menu's
/// last items would lie below the window, out of reach.
#[test]
fn a_layer_taller_than_the_window_scrolls_inside_it() {
    let least = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    let items: Vec<El<'_>> =
        (0..20).map(|at| button(&format!("Item {at}"), Msg::Base(at), ButtonVariant::Secondary)).collect();
    let menu = Column::with_children(items).spacing(Space::S.px());
    let over = menu_at(base(), Some(menu._boxed()), iced_core::Vector::new(8.0, 8.0), Msg::Close);
    let mut sim = sim_sized(over, least);
    struct Scrolled(Vec<(iced_core::Rectangle, iced_core::Vector)>);
    impl iced_core::widget::Operation for Scrolled {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _id: Option<&iced_core::widget::Id>,
            bounds: iced_core::Rectangle,
            _content: iced_core::Size,
            translation: iced_core::Vector,
            _state: &mut dyn iced_core::widget::operation::Scrollable,
        ) {
            self.0.push((bounds, translation));
        }
    }
    let mut scrolled = Scrolled(Vec::new());
    sim.operate(&mut scrolled);
    let (bounds, _) = scrolled.0.first().copied().expect("the menu scrolls");
    assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= least.height, "inside the window: {bounds:?}");
}

/// A layer shown under the pointer is told the pointer left when that hides it, so what it holds (a tooltip waiting to
/// show) does not stay as it was while it cannot be seen.
#[test]
fn a_layer_hidden_by_the_pointer_leaving_is_told_it_left() {
    let card = Size::new(200.0, 120.0);
    let fill = || container(iced_widget::Space::new()).width(Length::Fill).height(Length::Fill);
    let layer: El<'_> = iced_widget::mouse_area(fill()._boxed()).on_exit(Msg::Left)._boxed();
    let shown: El<'_> = fill().width(Length::Fixed(card.width)).height(Length::Fixed(card.height))._boxed();
    let mut sim = sim_sized(reveal(shown, layer, false), WINDOW);
    let inside = Point::new(card.width / 2.0, card.height / 2.0);
    sim.point_at(inside);
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: inside })]);
    settle(&mut sim);
    let away = Point::new(WINDOW.width - 10.0, WINDOW.height - 10.0);
    sim.point_at(away);
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position: away })]);
    let said: Vec<Msg> = sim.drain().collect();
    assert_eq!(said, [Msg::Left], "the layer saw the pointer go");
}

/// A field `above` pixels down the window whose menu of `count` choices is open: where the field and the menu are,
/// and the room the menu is shown in with the height of all it holds.
fn field_and_menu(above: f32, count: u8) -> (iced_core::Rectangle, iced_core::Rectangle, (iced_core::Rectangle, f32)) {
    struct Scrolled(Vec<(iced_core::Rectangle, f32)>);
    impl iced_core::widget::Operation for Scrolled {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _id: Option<&iced_core::widget::Id>,
            bounds: iced_core::Rectangle,
            content: Size,
            _translation: iced_core::Vector,
            _state: &mut dyn iced_core::widget::operation::Scrollable,
        ) {
            self.0.push((bounds, content.height));
        }
    }
    let choices = (0..count)
        .map(|at| MenuChoice {
            leading: None,
            label: label(&format!("Choice {at}")),
            note: None,
            chosen: at == 0,
            message: Msg::Base(at),
        })
        .collect();
    let select = Select {
        leading: None,
        name: label("Language"),
        shown: label("Choice 0"),
        choices,
        open: true,
        toggle: Msg::Rename,
        close: Msg::Close,
    };
    let column = Column::with_children([
        iced_widget::Space::new().height(Length::Fixed(above))._boxed(),
        container(select.view::<Renderer>()).width(Length::Fixed(300.0))._boxed(),
    ]);
    let mut sim = sim_sized(FocusScope::new(column), WINDOW);
    let found = boxes(&mut sim);
    let find = |wanted: fn(&SemanticRole) -> bool| {
        found.iter().find(|(each, _)| wanted(&each.role)).map(|(_, at)| *at).expect("drawn")
    };
    let (field, menu) =
        (find(|role| matches!(role, SemanticRole::Opens { open: true, .. })), find(|role| *role == SemanticRole::Menu));
    let mut scrolled = Scrolled(Vec::new());
    sim.operate(&mut scrolled);
    let room = scrolled.0.into_iter().find(|(bounds, _)| (bounds.y - menu.y).abs() < 0.5).expect("the menu's room");
    (field, menu, room)
}

/// A field's menu hangs under it, as wide as it; over it where the window has more room there; and, where it is taller
/// than the room it has, held to that room inside the window's gutter, so it scrolls.
#[test]
fn a_fields_menu_hangs_under_it_or_over_it_held_to_the_room_the_window_has() {
    let (field, menu, (room, holds)) = field_and_menu(40.0, 3);
    assert!(menu.y > field.y + field.height && (menu.width - field.width).abs() < 1.0, "{field:?} {menu:?}");
    assert!((room.height - holds).abs() < 0.5, "a menu that fits is its own height: {room:?} {holds}");
    let (field, menu, _) = field_and_menu(WINDOW.height - 80.0, 3);
    assert!(menu.y + menu.height < field.y, "over it: {field:?} {menu:?}");
    let (field, menu, (room, holds)) = field_and_menu(40.0, 40);
    assert!(menu.y > field.y + field.height, "{field:?} {menu:?}");
    assert!(room.y + room.height <= WINDOW.height - Space::L.px() + 0.5 && holds > room.height, "{room:?} {holds}");
}
