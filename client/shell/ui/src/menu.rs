//! A menu: a short list of actions or of choices where what opens it is, its rows reached by the arrow keys as well as
//! by Tab, on the one panel every menu is drawn on, an item's submenu beside it. It is `Layered`, which closes it and
//! gives the focus back.

use crate::layer::{Layered, Placement, on_panel};
use crate::{Icon, IconClass, Labelled, ListRow, Renderer, TextClass, Theme, coloured, icon};
use design::{
    Content, IconSize, Label, MENU_ITEM_HEIGHT, MENU_RADIUS, MENU_WIDTH, Rgb, Role, SemanticRole, Semantics, Space,
    TypeStyle,
};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container};

/// An item of a menu: its label, what it sends, whether it removes or leaves something, and whether a rule sets it
/// off from the items above it.
pub struct MenuEntry<Message> {
    /// What it says.
    pub label: Label,
    /// What choosing it sends.
    pub message: Message,
    /// It removes or leaves something: drawn in the danger colour.
    pub danger: bool,
    /// A rule sets it off from the items above it.
    pub rule_before: bool,
    /// An icon before its label, in the danger colour: what it does to someone else.
    pub icon: Option<Icon>,
}

/// What stands before a choice's name, in its menu and in the field that shows the choice taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leading {
    /// A glyph of the set.
    Icon(Icon),
    /// A colour's dot, the strong edge where the colour is none: a role.
    Dot(Option<Rgb>),
}

impl Leading {
    /// It, drawn.
    pub(crate) fn view<'a, Message: 'a, R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        match self {
            Leading::Icon(glyph) => {
                icon(glyph, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed()
            }
            Leading::Dot(colour) => crate::list_row::colour_dot(colour),
        }
    }
}

/// One choice of a menu of which one is taken: a device, a language.
pub struct MenuChoice<Message> {
    /// What stands before its name, after the check of the one taken.
    pub leading: Option<Leading>,
    /// Its name.
    pub label: Label,
    /// What is written beside it: which device the system's default follows now, the language the system asks for.
    pub note: Option<Label>,
    /// The one taken.
    pub chosen: bool,
    /// What choosing it sends.
    pub message: Message,
}

/// A choice as a menu's row: its check while it is taken, its name, which wraps rather than losing its end, and its
/// note at the end.
pub(crate) fn choice_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    choice: MenuChoice<Message>,
) -> Element<'a, Message, Theme, R> {
    let mut row =
        ListRow::<Message, R>::new(choice.label).menu_choice(choice.chosen).wrapped().on_press(choice.message);
    if let Some(leading) = choice.leading {
        row = row.after_mark(leading.view());
    }
    if let Some(note) = choice.note {
        row = row.noted(note.as_str());
    }
    row.view()
}

/// What a menu holds, which sets the room round and between its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuKind {
    /// Actions, close together: a server's menu, a message's.
    Actions,
    /// Choices of which one is taken, each set a little apart: the devices, the languages.
    Choices,
}

impl MenuKind {
    /// The room between the panel's edge and its rows.
    const fn padding(self) -> Space {
        match self {
            MenuKind::Actions => Space::Xs,
            MenuKind::Choices => Space::S,
        }
    }

    /// The room between one row and the next.
    const fn spacing(self) -> f32 {
        match self {
            MenuKind::Actions => 0.0,
            MenuKind::Choices => Space::Xxs.px(),
        }
    }
}

/// `rows` of `kind` on a menu's panel `width` wide, named `title` for a screen reader.
pub(crate) fn menu_panel<'a, Message: 'a, R: Renderer + 'a>(
    kind: MenuKind,
    title: Label,
    rows: Vec<Element<'a, Message, Theme, R>>,
    width: Length,
) -> Element<'a, Message, Theme, R> {
    let column = Column::with_children(rows).spacing(kind.spacing()).width(Length::Fill);
    let panel = container(on_panel(column._boxed(), kind.padding(), MENU_RADIUS)).width(width);
    Labelled::new(panel, Semantics { role: SemanticRole::Menu, label: title })._boxed()
}

/// The items of a menu under its `title`, which is also the menu's name for a screen reader.
pub fn menu_items<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    entries: Vec<MenuEntry<Message>>,
) -> Element<'a, Message, Theme, R> {
    let mut rows = vec![heading(&title)];
    rows.extend(entry_rows(entries));
    menu_panel(MenuKind::Actions, title, rows, Length::Fixed(MENU_WIDTH))
}

/// One choice of a menu of which any are taken: a role a member holds.
pub(crate) struct MenuCheck<Message> {
    /// Its name.
    pub label: Label,
    /// The colour it is shown in; none for none.
    pub colour: Option<design::Rgb>,
    /// Ticked.
    pub checked: bool,
    /// What a press sends: it is ticked, or no longer.
    pub message: Message,
}

/// The choices of a menu of which any are taken, under its `title`, which is also the menu's name for a screen
/// reader. A press ticks one and leaves the menu open.
pub(crate) fn menu_checks<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    checks: Vec<MenuCheck<Message>>,
) -> Element<'a, Message, Theme, R> {
    let mut rows = vec![heading(&title)];
    rows.extend(checks.into_iter().map(|check| {
        ListRow::<Message, R>::new(check.label).menu_check(check.checked, check.colour).on_press(check.message).view()
    }));
    menu_panel(MenuKind::Actions, title, rows, Length::Fixed(MENU_WIDTH))
}

/// A menu's title over its items, quiet and small.
fn heading<'a, Message: 'a, R: Renderer + 'a>(title: &Label) -> Element<'a, Message, Theme, R> {
    let heading =
        coloured(title.as_str().to_string(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary));
    container(heading).padding(HEADING_PADDING)._boxed()
}

/// The room round a menu's title.
const HEADING_PADDING: Padding = Padding {
    top: Space::Xs.px() + Space::Xxs.px(),
    bottom: Space::S.px(),
    left: Space::S.px() + Space::Xxs.px(),
    right: Space::S.px() + Space::Xxs.px(),
};

/// Each entry as a menu's row, with its rule before it where it asks for one.
pub(crate) fn entry_rows<'a, Message: Clone + 'a, R: Renderer + 'a>(
    entries: Vec<MenuEntry<Message>>,
) -> Vec<Element<'a, Message, Theme, R>> {
    let mut rows = Vec::new();
    for entry in entries {
        entry_row(&mut rows, entry, false);
    }
    rows
}

/// `entry` as a menu's row after its rule where it asks for one, with a chevron at its end where it `opens` a submenu.
fn entry_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    rows: &mut Vec<Element<'a, Message, Theme, R>>,
    entry: MenuEntry<Message>,
    opens: bool,
) {
    if entry.rule_before {
        rows.push(rule());
    }
    let mut row = ListRow::<Message, R>::new(entry.label).menu_item().on_press(entry.message);
    if let Some(shown) = entry.icon {
        let danger = Role::Status(design::Status::Danger, design::StatusRole::Text);
        row = row.leading(icon(shown, IconSize::S, IconClass::Role(danger))._boxed());
    }
    if opens {
        row = row.trailing(
            icon(Icon::ChevronRight, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        );
    }
    rows.push(if entry.danger { row.danger() } else { row }.view());
}

/// A menu's items under its `title`, each with whether it opens a submenu, and `sub`, a submenu's panel, beside the
/// item at its place among them, its first row level with that item.
pub(crate) fn menu_with_submenu<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    entries: Vec<(MenuEntry<Message>, bool)>,
    sub: Option<(usize, Element<'a, Message, Theme, R>)>,
) -> Element<'a, Message, Theme, R> {
    let rule_room = RULE_PADDING.top + design::BORDER_WIDTH + RULE_PADDING.bottom;
    let mut above = heading_height();
    let mut rows = vec![heading(&title)];
    let mut sub = sub;
    let mut hung = None;
    for (at, (entry, opens)) in entries.into_iter().enumerate() {
        if entry.rule_before {
            above += rule_room;
        }
        if let Some((_, panel)) = sub.take_if(|(from, _)| *from == at) {
            hung = Some((above, panel));
        }
        entry_row(&mut rows, entry, opens);
        above += MENU_ITEM_HEIGHT;
    }
    let menu = menu_panel(MenuKind::Actions, title, rows, Length::Fixed(MENU_WIDTH));
    match hung {
        Some((above, panel)) => {
            let beside = Column::with_children([Gap::new().height(Length::Fixed(above))._boxed(), panel]);
            Row::with_children([menu, beside._boxed()]).spacing(Space::Xs.px())._boxed()
        }
        None => menu,
    }
}

/// A submenu's panel: `rows` with no title, named `name` for a screen reader.
pub(crate) fn submenu<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    rows: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    menu_panel(MenuKind::Actions, name, rows, Length::Fixed(design::SUBMENU_WIDTH))
}

/// The height of a menu's title as [`heading`] draws it.
fn heading_height() -> f32 {
    HEADING_PADDING.top + TypeStyle::CaptionStrong.line_height() + HEADING_PADDING.bottom
}

/// A hairline between groups of items.
fn rule<'a, Message: 'a, R: Renderer + 'a>() -> Element<'a, Message, Theme, R> {
    container(crate::panel_parts::hairline()).padding(RULE_PADDING)._boxed()
}

/// The room round a hairline between groups of items.
const RULE_PADDING: Padding = Padding {
    top: Space::Xs.px(),
    bottom: Space::Xs.px(),
    left: Space::Xs.px() + Space::Xxs.px(),
    right: Space::Xs.px() + Space::Xxs.px(),
};

/// `trigger`, with `items` hanging under it while they are `Some`, their end at the trigger's end, or over it where the
/// window has more room there: a row's menu, opened from its buttons.
pub(crate) fn menu_hung<'a, Message: Clone + 'a, R: Renderer + 'a>(
    trigger: Element<'a, Message, Theme, R>,
    items: Option<Element<'a, Message, Theme, R>>,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(trigger, items, Placement::Hung { gap: Space::Xs.px() + Space::Xxs.px() }, on_close, true)._boxed()
}

/// `screen`, with `items` at `at` in it while they are `Some`: a menu over the whole window, placed by the screen
/// where what opened it is.
pub fn menu_at<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    items: Option<Element<'a, Message, Theme, R>>,
    at: iced_core::Vector,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, items, Placement::At(at), on_close, true)._boxed()
}

/// `screen`, with `drawer` at its left edge while it is `Some`, over a scrim on the rest: Escape and a press on the
/// scrim send `on_close`.
pub fn drawer<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    drawer: Option<Element<'a, Message, Theme, R>>,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, drawer, Placement::Edge, on_close, false)._boxed()
}

/// `screen`, with `column` at its right edge from `top` down while it is `Some`, over a scrim on the rest below `top`:
/// Escape and a press on the scrim send `on_close`, and what is above `top` stays the screen's.
pub(crate) fn end_drawer<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    column: Option<Element<'a, Message, Theme, R>>,
    top: f32,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, column, Placement::EndEdge { top }, on_close, false)._boxed()
}

/// `screen`, with `items` in it while they are `Some`: a context menu where the pointer pressed, kept inside the
/// screen, or at `keyed` when the keyboard opened it.
pub fn menu_at_pointer<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    items: Option<Element<'a, Message, Theme, R>>,
    keyed: iced_core::Vector,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, items, Placement::Pointer { keyed }, on_close, true)._boxed()
}

/// `screen`, with `popover` over the whole of it while it is `Some`, hung from the card that carries
/// [`ABOUT_CARD`](crate::layer::ABOUT_CARD) and kept whole inside `area`, in the screen's own coordinates: no card or
/// pane under it clips it.
pub(crate) fn popover_over_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    popover: Option<Element<'a, Message, Theme, R>>,
    area: iced_core::Rectangle,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, popover, Placement::About { area }, on_close, false)._boxed()
}

/// `screen`, with `layer` in it while it is `Some`, on `side` of the control that opened it, or of `fallback` (where
/// the screen puts the opener) when none had the focus: a row's menu or a person's popover.
pub(crate) fn menu_beside<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    layer: Option<Element<'a, Message, Theme, R>>,
    side: crate::layer::Side,
    fallback: iced_core::Rectangle,
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, layer, Placement::Beside { side, fallback }, on_close, true)._boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use design::{SemanticRole, Semantics};
    use iced_core::{Point, Rectangle, Size};
    use iced_test::Simulator;
    use iced_widget::{Column, Row};

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Ticked(&'static str),
        Close,
    }

    const WINDOW: Size = Size::new(640.0, 400.0);

    fn label(text: &str) -> Label {
        Label::new(text).expect("a label")
    }

    /// What a screen reader is told of, with where each is drawn.
    fn boxes_of(sim: &mut Simulator<'_, Said, Theme, Renderer>) -> Vec<(Semantics, Rectangle)> {
        struct Boxes(Vec<(Semantics, Rectangle)>);
        impl iced_core::widget::Operation for Boxes {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _id: Option<&iced_core::widget::Id>,
                bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(said) = state.downcast_ref::<Semantics>() {
                    self.0.push((said.clone(), bounds));
                }
            }
        }
        let mut boxes = Boxes(Vec::new());
        sim.operate(&mut boxes);
        boxes.0
    }

    /// A trigger at the window's right, `down` from its top, with a menu of checks hung from it.
    fn hung(down: f32) -> Simulator<'static, Said, Theme, Renderer> {
        crate::testing::hermetic_fonts();
        let checks = ["Admin", "Moderators", "Regulars"]
            .into_iter()
            .map(|name| MenuCheck {
                label: label(name),
                colour: None,
                checked: name == "Regulars",
                message: Said::Ticked(name),
            })
            .collect();
        let items = menu_checks::<Said, Renderer>(label("Roles for Bo"), checks);
        let trigger = crate::Button::new(label("Roles")).on_press(Said::Close).view::<Renderer>();
        let trigger = menu_hung(trigger, Some(items), Said::Close);
        let row = Row::with_children([iced_widget::Space::new().width(Length::Fill)._boxed(), trigger]);
        let page =
            Column::with_children([iced_widget::Space::new().height(Length::Fixed(down))._boxed(), row._boxed()]);
        Simulator::with_size(iced_core::Settings::default(), WINDOW, page)
    }

    fn found(boxes: &[(Semantics, Rectangle)], name: &str) -> (SemanticRole, Rectangle) {
        boxes.iter().find(|(said, _)| said.label.as_str() == name).map(|(said, at)| (said.role, *at)).expect(name)
    }

    #[test]
    fn a_hung_menu_is_its_own_width_its_end_at_the_triggers_and_flips_over_it_near_the_bottom() {
        let mut sim = hung(40.0);
        let boxes = boxes_of(&mut sim);
        let (_, trigger) = found(&boxes, "Roles");
        let (_, menu) = found(&boxes, "Roles for Bo");
        assert!((menu.width - MENU_WIDTH).abs() < 0.5, "its own width: {menu:?}");
        assert!((menu.x + menu.width - (trigger.x + trigger.width)).abs() < 0.5, "{trigger:?} {menu:?}");
        assert!(menu.y > trigger.y + trigger.height, "under it: {trigger:?} {menu:?}");
        let mut sim = hung(WINDOW.height - 60.0);
        let boxes = boxes_of(&mut sim);
        let (_, trigger) = found(&boxes, "Roles");
        let (_, menu) = found(&boxes, "Roles for Bo");
        assert!(menu.y + menu.height < trigger.y, "over it: {trigger:?} {menu:?}");
    }

    #[test]
    fn a_menu_hung_inside_an_open_dialog_is_shown_and_pressed() {
        crate::testing::hermetic_fonts();
        let checks =
            vec![MenuCheck { label: label("Admin"), colour: None, checked: false, message: Said::Ticked("Admin") }];
        let items = menu_checks::<Said, Renderer>(label("Roles for Bo"), checks);
        let trigger = crate::Button::new(label("Roles")).on_press(Said::Close).view::<Renderer>();
        let inside = menu_hung(trigger, Some(items), Said::Close);
        let screen = iced_widget::Space::new().width(Length::Fill).height(Length::Fill);
        let page = crate::dialog(screen._boxed(), Some(inside), Said::Close);
        let mut sim = Simulator::with_size(iced_core::Settings::default(), WINDOW, page);
        let boxes = boxes_of(&mut sim);
        let (_, admin) = found(&boxes, "Admin");
        sim.point_at(Point::new(admin.center_x(), admin.center_y()));
        sim.simulate(crate::testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Ticked("Admin")]);
    }

    #[test]
    fn a_menu_of_checks_says_each_is_ticked_or_not_and_a_press_ticks_it() {
        let mut sim = hung(40.0);
        let boxes = boxes_of(&mut sim);
        assert_eq!(found(&boxes, "Admin").0, SemanticRole::MenuItemCheckbox { checked: false });
        assert_eq!(found(&boxes, "Regulars").0, SemanticRole::MenuItemCheckbox { checked: true });
        let (_, moderators) = found(&boxes, "Moderators");
        sim.point_at(Point::new(moderators.center_x(), moderators.center_y()));
        sim.simulate(crate::testing::click());
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Ticked("Moderators")]);
    }
}
