//! A layer over a screen, the one widget behind a dialog and a menu: while it is open the events and the focus go to it,
//! never round it, a screen reader reads a dialog alone and a menu over the screen, and when it closes the focus goes
//! back to where it was.

use crate::focus::{Covering, FindAt, FindFocus, clear_focus, focus_index, focus_on, focused_index};
use crate::{FocusScope, Renderer, Surfaced, Theme};
use design::{ABOUT_POPOVER_WIDTH, Elevation, Radius, Space};
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::{Id, Operation, Tree, tree};
use iced_core::{
    Element, Event, Layout, Length, Point, Rectangle, Shell, Size, Vector, Widget, layout, mouse, overlay, renderer,
};
use screens::popover_place::{Rect, popover_place};

/// Where the layer sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Placement {
    /// In the middle of the window, over a scrim that shuts the screen away: a dialog.
    Centred,
    /// At this point of the screen it is over, with nothing shut away: a menu over the whole window, placed where
    /// what opened it is, so it takes the focus from (and gives it back to) any control of the window.
    At(Vector),
    /// Over the screen with its bottom edge at `bottom` and its middle at `centre`, however big it is: a menu above a
    /// bar that is in the middle of the screen.
    Anchored { centre: f32, bottom: f32 },
    /// Over the screen with its bottom edge at `bottom` and its left edge at `left`: a popover rising from a control at
    /// the side of the window.
    Rising { left: f32, bottom: f32 },
    /// Where the pointer last pressed the screen, kept inside it: a context menu. Opened from the keyboard, at `keyed`.
    Pointer { keyed: Vector },
    /// On `side` of the control that opened it (the one that held the focus as it opened), kept inside the screen: a
    /// popover beside a row. Where no control had the focus, beside `fallback`, the opener's box as its screen puts it.
    Beside { side: Side, fallback: Rectangle },
    /// At the window's left edge, as tall as the window, over a scrim on the rest of it: a drawer.
    Edge,
    /// At the window's right edge from `top` down, over a scrim on the rest below `top`: the members column where it
    /// cannot dock, under the controls that open it, which stay the screen's; or the call's chat over the call.
    EndEdge { top: f32 },
    /// As wide as the screen it hangs from, `gap` under it, or over it where the window has more room there, and no
    /// taller than that room: the menu of a field that opens one.
    Below { gap: f32 },
    /// As wide as it is, its end at the end of the screen it hangs from, `gap` under it, or over it where the window
    /// has more room there: a menu hanging from a row's buttons.
    Hung { gap: f32 },
    /// Over the whole call, [`ABOUT_POPOVER_WIDTH`] wide, hung from the card that carries [`ABOUT_CARD`] and kept whole
    /// inside `area` (in the screen's own coordinates): the popover about a screen's picture, which no card clips.
    About { area: Rectangle },
}

/// The id of the one card whose popover about its picture is open, which that popover hangs from.
pub(crate) static ABOUT_CARD: Id = Id::new("about-card");

/// Which side of what opened it a layer placed [`Placement::Beside`] opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// Its right edge a gap left of the opener's left, its top at the opener's.
    Left,
    /// Its left edge a gap right of the opener's right, its top at the opener's.
    Right,
    /// A gap under the opener, at its left edge.
    Below,
    /// Its bottom edge a gap over the opener's top, at its left edge.
    Above,
    /// Its corner where the pointer pressed the opener; opened from the keyboard, at the opener's corner.
    Pointer,
}

#[derive(Debug, Default)]
struct State {
    open: bool,
    /// Which control of the screen had the focus when the layer opened, by what it is and not where it stood, and
    /// whether its ring showed.
    saved: Option<(accesskit::NodeId, bool)>,
    /// Whether the keyboard, not the pointer, acted last: a layer it opened shows its focus, one a click opened does
    /// not.
    keyboard: bool,
    /// Where the pointer last pressed while no layer was open, from the screen's corner: where a context menu opens.
    pressed: Option<Point>,
    /// The box of the control that had the focus as the layer opened, from the screen's corner.
    opener: Option<Rectangle>,
}

/// A screen with a layer over it while one is open, drawn as iced's overlay: as a widget, later siblings would draw
/// over it.
pub(crate) struct Layered<'a, Message, R> {
    /// The screen, and the layer when it is open.
    children: Vec<Element<'a, Message, Theme, R>>,
    placement: Placement,
    on_close: Option<Message>,
}

impl<'a, Message: Clone + 'a, R: Renderer + 'a> Layered<'a, Message, R> {
    /// `layer` is shown over `screen` while it is `Some`; Escape, and a press outside it, send `on_close`.
    pub(crate) fn new(
        screen: Element<'a, Message, Theme, R>,
        layer: Option<Element<'a, Message, Theme, R>>,
        placement: Placement,
        on_close: Message,
        arrows: bool,
    ) -> Self {
        let mut children = vec![screen];
        children.extend(layer.map(|layer| {
            // A menu or a popover taller than the window less its gutter scrolls inside it; a dialog scrolls its own
            // body and a drawer its own list.
            let layer = match placement {
                Placement::Centred | Placement::Edge | Placement::EndEdge { .. } => layer,
                Placement::At(_)
                | Placement::Anchored { .. }
                | Placement::Rising { .. }
                | Placement::Pointer { .. }
                | Placement::Beside { .. }
                | Placement::Below { .. }
                | Placement::Hung { .. }
                | Placement::About { .. } => iced_widget::scrollable(layer)
                    .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
                    ._boxed(),
            };
            let scope = FocusScope::new(layer);
            let scope = if arrows { scope.arrows() } else { scope };
            Widget::_boxed(scope)
        }));
        Layered { children, placement, on_close: Some(on_close) }
    }

    fn is_open(&self) -> bool {
        self.children.len() == 2
    }
}

impl<Message, R> iced_core::widget::Meta for Layered<'_, Message, R> {}

impl<Message: Clone, R: Renderer> Widget<Message, Theme, R> for Layered<'_, Message, R> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        self.children[0].size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        self.children[0].layout(&mut tree.children[0], renderer, limits);
        let screen = tree.children[0].size;
        tree.size = screen;
        if self.is_open() {
            // A drawer has the window's height; anything else is as big as it is, whatever room its small trigger had,
            // up to the window less a gutter round it.
            let inside = Size::new(
                (screen.width - 2.0 * Space::L.px()).max(0.0),
                (screen.height - 2.0 * Space::L.px()).max(0.0),
            );
            let loose = match self.placement {
                Placement::Edge => layout::Limits::new(Size::new(0.0, screen.height), screen),
                Placement::EndEdge { top } => {
                    let below = Size::new(screen.width, (screen.height - top).max(0.0));
                    layout::Limits::new(Size::new(0.0, below.height), below)
                }
                // Hung above a point, it has the room above it.
                Placement::Anchored { bottom, .. } | Placement::Rising { bottom, .. } => layout::Limits::new(
                    Size::ZERO,
                    Size::new(inside.width, (bottom - Space::L.px()).clamp(0.0, inside.height)),
                ),
                Placement::Centred | Placement::At(_) | Placement::Pointer { .. } | Placement::Beside { .. } => {
                    layout::Limits::new(Size::ZERO, inside)
                }
                // Its room in the window is known only where it is drawn, which holds it to that room.
                Placement::Below { .. } => {
                    layout::Limits::new(Size::new(screen.width, 0.0), Size::new(screen.width, f32::INFINITY))
                }
                // Its room is the window's, not its small trigger's: it is held to it where it is drawn.
                Placement::Hung { .. } => layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY)),
                // Held to the area it lies in; where it lies is known only where it is drawn.
                Placement::About { area } => {
                    let width = ABOUT_POPOVER_WIDTH.min(area.width.max(0.0));
                    layout::Limits::new(Size::new(width, 0.0), Size::new(width, area.height.max(0.0)))
                }
            };
            self.children[1].layout(&mut tree.children[1], renderer, &loose);
            let layer = tree.children[1].size;
            tree.children[1].translation = match self.placement {
                Placement::Centred => {
                    Vector::new((screen.width - layer.width) / 2.0, (screen.height - layer.height) / 2.0)
                }
                // Where it would run past the window, it is moved in.
                Placement::At(at) => Vector::new(
                    at.x.clamp(0.0, (screen.width - layer.width).max(0.0)),
                    at.y.clamp(0.0, (screen.height - layer.height).max(0.0)),
                ),
                Placement::Edge => Vector::ZERO,
                Placement::EndEdge { top } => Vector::new((screen.width - layer.width).max(0.0), top),
                Placement::Anchored { centre, bottom } => Vector::new(
                    (centre - layer.width / 2.0).clamp(0.0, (screen.width - layer.width).max(0.0)),
                    (bottom - layer.height).max(0.0),
                ),
                Placement::Rising { left, bottom } => Vector::new(
                    left.clamp(0.0, (screen.width - layer.width).max(0.0)),
                    (bottom - layer.height).max(0.0),
                ),
                Placement::Below { gap } => Vector::new(0.0, screen.height + gap),
                Placement::Hung { gap } => Vector::new(screen.width - layer.width, screen.height + gap),
                Placement::About { .. } => Vector::ZERO,
                Placement::Beside { side, fallback } => {
                    let state = tree.state.downcast_ref::<State>();
                    let opener = match (side, state.keyboard, state.pressed) {
                        (Side::Pointer, false, Some(point)) => Rectangle::new(point, Size::ZERO),
                        _ => state.opener.unwrap_or(fallback),
                    };
                    // Clear of a column's edge to its left, a little apart from a row in the pane to its right.
                    let (left, right, below) = (Space::Xl.px(), Space::S.px(), Space::Xs.px() + Space::Xxs.px());
                    let (x, y) = match side {
                        Side::Left => (opener.x - left - layer.width, opener.y),
                        Side::Right => (opener.x + opener.width + right, opener.y),
                        Side::Below => (opener.x, opener.y + opener.height + below),
                        Side::Above => (opener.x, opener.y - below - layer.height),
                        Side::Pointer => (opener.x, opener.y),
                    };
                    Vector::new(
                        x.clamp(0.0, (screen.width - layer.width).max(0.0)),
                        y.clamp(0.0, (screen.height - layer.height).max(0.0)),
                    )
                }
                Placement::Pointer { keyed } => {
                    let state = tree.state.downcast_ref::<State>();
                    let at = match (state.keyboard, state.pressed) {
                        (false, Some(point)) => Vector::new(point.x, point.y),
                        (true, _) | (_, None) => keyed,
                    };
                    Vector::new(
                        at.x.clamp(0.0, (screen.width - layer.width).max(0.0)),
                        at.y.clamp(0.0, (screen.height - layer.height).max(0.0)),
                    )
                }
            };
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        // Open, what is under the layer is not there to focus or to find, and under a scrim not there to read
        // either; under a menu or a popover it is read as it stands, which is how its opener says it is open. The
        // layer's own controls are reported by the overlay.
        let Some((screen_layout, screen)) = layout.iter_mut(&mut tree.children).next() else { return };
        match (self.is_open(), self.placement) {
            (false, _) => self.children[0].operate(screen, screen_layout, viewport, renderer, operation),
            (true, Placement::Centred | Placement::Edge | Placement::EndEdge { .. }) => {}
            (
                true,
                Placement::At(_)
                | Placement::Anchored { .. }
                | Placement::Rising { .. }
                | Placement::Pointer { .. }
                | Placement::Beside { .. }
                | Placement::Below { .. }
                | Placement::Hung { .. }
                | Placement::About { .. },
            ) => {
                self.children[0].operate(screen, screen_layout, viewport, renderer, &mut Covering(operation));
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { .. }) => tree.state.downcast_mut::<State>().keyboard = true,
            Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(_) => {
                let state = tree.state.downcast_mut::<State>();
                state.keyboard = false;
                if !self.is_open() {
                    let origin = layout.bounds().position();
                    state.pressed = cursor.position().map(|point| Point::new(point.x - origin.x, point.y - origin.y));
                }
            }
            _ => {}
        }
        let open = self.is_open();
        let was_open = tree.state.downcast_ref::<State>().open;
        if open != was_open {
            self.change(tree, layout, renderer, viewport, open);
            // A layer placed by what opened it is placed again now that its opener is known.
            shell.invalidate_layout();
            shell.request_redraw();
        }
        // Open, the overlay has the events first; the screen still hears the window's own (a redraw) with no pointer,
        // so it is drawn as it is and reached by nothing but the pointer where the layer leaves it the screen's.
        let left = self.left_to_screen(layout, cursor);
        let pointer = matches!(event, Event::Mouse(_) | Event::Touch(_));
        let for_screen = !open || matches!(event, Event::Window(_)) || (pointer && left.position().is_some());
        if for_screen && let Some((screen_layout, screen)) = layout.iter_mut(&mut tree.children).next() {
            let cursor = if open { left } else { cursor };
            self.children[0].update(screen, event, screen_layout, cursor, renderer, shell, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &R,
    ) -> mouse::Interaction {
        let cursor = if self.is_open() { self.left_to_screen(layout, cursor) } else { cursor };
        if cursor.position().is_none() && self.is_open() {
            return mouse::Interaction::None;
        }
        layout.iter(&tree.children).next().map_or(mouse::Interaction::None, |(screen_layout, child)| {
            self.children[0].mouse_interaction(child, screen_layout, cursor, viewport, renderer)
        })
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // With the layer open the pointer is not over the screen under it, so nothing there shows as hovered, but
        // where the layer leaves the screen its own.
        let under = if self.is_open() { self.left_to_screen(layout, cursor) } else { cursor };
        if let Some((screen_layout, child)) = layout.iter(&tree.children).next() {
            self.children[0].draw(child, renderer, theme, style, screen_layout, under, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, R>> {
        if !self.is_open() {
            return self.children[0].overlay(&mut tree.children[0], layout, renderer, viewport, translation, window);
        }
        match self.placement {
            Placement::Below { gap } => {
                self.hang(tree, layout, renderer, Hanging { translation, window, gap }, Span::Screen)
            }
            Placement::Hung { gap } => {
                self.hang(tree, layout, renderer, Hanging { translation, window, gap }, Span::Own)
            }
            Placement::About { area } => self.lie_over_card(tree, layout, renderer, viewport, translation, area),
            Placement::Centred
            | Placement::At(_)
            | Placement::Anchored { .. }
            | Placement::Rising { .. }
            | Placement::Pointer { .. }
            | Placement::Beside { .. }
            | Placement::Edge
            | Placement::EndEdge { .. } => {}
        }
        let Some((layer_layout, layer_tree)) = layout.iter_mut(&mut tree.children).nth(1) else { return Vec::new() };
        let Layered { children, placement, on_close } = self;
        let Some(layer) = children.get_mut(1) else { return Vec::new() };
        vec![overlay::Element::new(Box::new(Top {
            layer,
            tree: layer_tree,
            layout: layer_layout,
            window,
            placement: *placement,
            on_close: on_close.clone(),
        }))]
    }
}

/// The open layer, as iced's overlay: drawn last and offered the events first.
struct Top<'a, 'b, Message, R> {
    layer: &'a mut Element<'b, Message, Theme, R>,
    tree: &'a mut Tree,
    layout: Layout,
    window: Size,
    placement: Placement,
    on_close: Option<Message>,
}

impl<Message: Clone, R: Renderer> overlay::Overlay<Message, Theme, R> for Top<'_, '_, Message, R> {
    fn draw(&self, renderer: &mut R, theme: &Theme, style: &renderer::Style, cursor: mouse::Cursor) {
        let everything = Rectangle::with_size(self.window);
        renderer.with_layer(everything, |renderer| {
            let scrim = match self.placement {
                Placement::Centred => Some(everything),
                Placement::Edge => {
                    let beside = self.layout.bounds().width;
                    Some(Rectangle { x: beside, width: (self.window.width - beside).max(0.0), ..everything })
                }
                Placement::EndEdge { .. } => {
                    let layer = self.layout.bounds();
                    let height = (self.window.height - layer.y).max(0.0);
                    Some(Rectangle { x: 0.0, y: layer.y, width: layer.x.max(0.0), height })
                }
                Placement::At(_)
                | Placement::Anchored { .. }
                | Placement::Rising { .. }
                | Placement::Pointer { .. }
                | Placement::Beside { .. }
                | Placement::Below { .. }
                | Placement::Hung { .. }
                | Placement::About { .. } => None,
            };
            if let Some(bounds) = scrim {
                renderer.fill_quad(
                    renderer::Quad { bounds, ..renderer::Quad::default() },
                    iced_core::Background::Color(crate::color::translucent(theme.palette().dialog_scrim())),
                );
            }
            self.layer.draw(self.tree, renderer, theme, style, self.layout, cursor, &everything);
        });
    }

    fn operate(&mut self, renderer: &R, operation: &mut dyn Operation) {
        let everything = Rectangle::with_size(self.window);
        self.layer.operate(self.tree, self.layout, &everything, renderer, operation);
    }

    fn update(&mut self, event: &Event, cursor: mouse::Cursor, renderer: &R, shell: &mut Shell<'_, Message>) {
        let everything = Rectangle::with_size(self.window);
        let inside = cursor.is_over(self.layout.bounds());
        // Above an end drawer the pointer is the screen's: the controls there, the drawer's toggle among them, work.
        let above = matches!(self.placement, Placement::EndEdge { .. })
            && cursor.position().is_some_and(|at| at.y < self.layout.bounds().y);
        if above && matches!(event, Event::Mouse(_) | Event::Touch(_)) {
            return;
        }
        let pressed = matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(_));
        self.layer.update(self.tree, event, self.layout, cursor, renderer, shell, &everything);
        if shell.is_event_captured() {
            return;
        }
        let escape =
            matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }));
        if (escape || (pressed && !inside))
            && let Some(close) = self.on_close.clone()
        {
            shell.publish(close);
        }
        // Whatever it was, it was for the layer: nothing reaches the screen under it.
        if escape || pressed || matches!(event, Event::Keyboard(_)) {
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, cursor: mouse::Cursor, renderer: &R) -> mouse::Interaction {
        let everything = Rectangle::with_size(self.window);
        self.layer.mouse_interaction(self.tree, self.layout, cursor, &everything, renderer)
    }

    fn overlay<'c>(&'c mut self, renderer: &R) -> Vec<overlay::Element<'c, Message, Theme, R>> {
        // A menu inside the layer (a field's in a dialog) is an overlay of the layer's own.
        let window = self.window;
        self.layer.overlay(self.tree, self.layout, renderer, &Rectangle::with_size(window), Vector::ZERO, window)
    }
}

impl<Message: Clone, R: Renderer> Layered<'_, Message, R> {
    /// The pointer where the open layer leaves the screen its own, above an end drawer's top; none elsewhere.
    fn left_to_screen(&self, layout: Layout, cursor: mouse::Cursor) -> mouse::Cursor {
        match (self.placement, cursor.position()) {
            (Placement::EndEdge { top }, Some(at)) if at.y < layout.bounds().y + top => cursor,
            _ => mouse::Cursor::Unavailable,
        }
    }

    /// Places a layer that hangs from its screen where the window has room for it, `gap` under the screen (scrolled
    /// by `translation`) or over it where there is more room there, held to that room less the window's gutter; as
    /// wide as the screen, or as wide as it is with its end at the screen's.
    fn hang(&mut self, tree: &mut Tree, layout: Layout, renderer: &R, hanging: Hanging, span: Span) {
        let Hanging { translation, window, gap } = hanging;
        let field = Rectangle::new(layout.position() + translation, layout.size());
        let gutter = Space::L.px();
        let below = (window.height - gutter - (field.y + field.height + gap)).max(0.0);
        let above = (field.y - gap - gutter).max(0.0);
        let room = below.max(above);
        let width = match span {
            Span::Screen => field.width,
            Span::Own => tree.children[1].size.width,
        };
        if tree.children[1].size.height > room {
            let limits = layout::Limits::new(Size::new(width, 0.0), Size::new(width, room));
            self.children[1].layout(&mut tree.children[1], renderer, &limits);
        }
        let height = tree.children[1].size.height;
        let top = if height <= below || below >= above { field.y + field.height + gap } else { field.y - gap - height };
        // Its end at the screen's, moved in where it would run past the window's side.
        let left = (field.x + field.width - width).clamp(0.0, (window.width - width).max(0.0));
        tree.children[1].translation = Vector::new(left - layout.position().x, top - layout.position().y);
    }

    /// Places the layer over the card that carries [`ABOUT_CARD`], wherever the call drew it, by [`popover_place`].
    fn lie_over_card(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
        area: Rectangle,
    ) {
        let mut find = FindCard::default();
        if let Some((screen_layout, screen)) = layout.iter_mut(&mut tree.children).next() {
            self.children[0].operate(screen, screen_layout, viewport, renderer, &mut find);
        }
        let Some(card) = find.0 else { return };
        let origin = layout.position() + translation;
        let placed = popover_place(
            Rect::new(card.x + translation.x, card.y + translation.y, card.width, card.height),
            Rect::new(area.x + origin.x, area.y + origin.y, area.width, area.height),
            tree.children[1].size.height,
        );
        tree.children[1].translation = Vector::new(placed.x - layout.position().x, placed.y - layout.position().y);
    }

    /// The layer opened or closed: on opening, remember where the focus was, take it from the screen and give it
    /// to the layer's first control where none of its own took it; on closing, give it back to the control that had
    /// it.
    fn change(&mut self, tree: &mut Tree, layout: Layout, renderer: &R, viewport: &Rectangle, open: bool) {
        let mut parts = layout.iter_mut(&mut tree.children);
        let Some((screen_layout, screen)) = parts.next() else { return };
        let screen_widget = &mut self.children[0];
        let mut walk_screen = |operation: &mut dyn Operation| {
            screen_widget.operate(screen, screen_layout, viewport, renderer, operation);
        };
        let keyboard = tree.state.downcast_ref::<State>().keyboard;
        let origin = screen_layout.bounds().position();
        let (state_saved, opener) = if open {
            let mut find = FindFocus::default();
            walk_screen(&mut find);
            // Opened by the pointer, beside the control under the press, which an open layer kept from taking the
            // focus; opened from the keyboard, beside the control that has the focus.
            let pressed = tree.state.downcast_ref::<State>().pressed.filter(|_| !keyboard);
            let under = pressed.and_then(|point| {
                let mut at = FindAt::new(Point::new(point.x + origin.x, point.y + origin.y));
                walk_screen(&mut at);
                at.bounds()
            });
            clear_focus(&mut walk_screen);
            let opener = under.or(find.bounds()).map(|at| Rectangle { x: at.x - origin.x, y: at.y - origin.y, ..at });
            (find.identity(), opener)
        } else {
            (None, None)
        };
        if open {
            if let Some((layer_layout, layer)) = parts.next() {
                let layer_widget = &mut self.children[1];
                let mut walk_layer = |operation: &mut dyn Operation| {
                    layer_widget.operate(layer, layer_layout, viewport, renderer, operation);
                };
                // The layer, an overlay, saw this event first: a field in it opened to be typed in has the focus.
                if focused_index(&mut walk_layer).is_none() {
                    focus_index(walk_layer, 0, keyboard);
                }
            }
            let state: &mut State = tree.state.downcast_mut();
            state.saved = state_saved;
            state.opener = opener;
        } else {
            let saved = tree.state.downcast_ref::<State>().saved;
            if let Some((id, shown)) = saved {
                focus_on(&mut walk_screen, id, shown);
            }
        }
        tree.state.downcast_mut::<State>().open = open;
    }
}

/// The box of the card that carries [`ABOUT_CARD`], where the screen laid it out.
#[derive(Default)]
struct FindCard(Option<Rectangle>);

impl Operation for FindCard {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle, _viewport: &Rectangle) {
        if id == Some(&ABOUT_CARD) {
            self.0 = Some(bounds);
        }
    }
}

/// Where a hung layer hangs: the screen's scroll, the window, and the gap between them.
#[derive(Clone, Copy)]
struct Hanging {
    translation: Vector,
    window: Size,
    gap: f32,
}

/// How wide a hung layer is.
#[derive(Clone, Copy)]
enum Span {
    /// As wide as the screen it hangs from.
    Screen,
    /// As wide as it is, its end at the screen's.
    Own,
}

/// A panel for a layer to be drawn on.
pub(crate) fn on_panel<'a, Message: 'a, R: Renderer + 'a>(
    content: Element<'a, Message, Theme, R>,
    padding: Space,
    radius: Radius,
) -> Element<'a, Message, Theme, R> {
    iced_widget::container(content).padding(padding.px()).class(Surfaced::Panel(Elevation::Overlay, radius))._boxed()
}
