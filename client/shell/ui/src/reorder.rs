//! A list a person puts in order: a row is dragged to the gap it should sit in, or moved one place by Alt and an arrow
//! key while it has the focus. Each row keeps its state by key as it moves, and the list says only which move was
//! asked; the screen decides whether it is made.

use crate::drawn::Drawn;
use crate::rows::Rows;
use crate::{Labelled, Renderer, Theme};
use design::{AccentRole, DRAG_LINE, DRAG_START, Label, Radius, Role, SemanticRole, Semantics, Space};
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::operation::focusable::Focusable;
use iced_core::widget::{Id, Meta, Operation, Tree, tree};
use iced_core::{Background, Element, Event, Layout, Point, Rectangle, Shell, Widget, mouse, renderer};
use screens::Moved;
use std::hash::Hash;

/// The list named `label` of `rows`, each under its key, top to bottom, which a person reorders.
///
/// A drag or Alt+Up/Down asks for a [`Moved`]: the row, and the row it is to sit just before (`None`: last). `may`
/// says whether a move is allowed: a drop line shows only over a gap it allows, a row no allowed move takes cannot be
/// grabbed, and a refused move sends nothing. An allowed move is sent through `on_move`; the list shows the order it
/// is given, so the screen shows a move by giving the rows again in their new order.
///
/// A press that does not move [`design::DRAG_START`] stays the row's own (a row that is a button is pressed); past it
/// the row is dragged and its press let go. Escape ends a drag without a move. A screen reader is told a list.
pub fn reorder<'a, K, Message, R>(
    label: Label,
    rows: impl IntoIterator<Item = (K, Element<'a, Message, Theme, R>)>,
    may: impl Fn(&Moved<K>) -> bool + 'a,
    on_move: impl Fn(Moved<K>) -> Message + 'a,
) -> Element<'a, Message, Theme, R>
where
    K: Clone + Eq + Hash + 'static,
    Message: 'a,
    R: Renderer + 'a,
{
    let (keys, rows): (Vec<K>, Vec<_>) = rows.into_iter().map(|(key, row)| (key.clone(), (key, row))).unzip();
    let list = Reorder { content: Rows::new(rows), keys, may: Box::new(may), on_move: Box::new(on_move) };
    Labelled::new(list, Semantics { role: SemanticRole::List, label })._boxed()
}

/// Whether a move is allowed.
type May<'a, K> = Box<dyn Fn(&Moved<K>) -> bool + 'a>;

struct Reorder<'a, K, Message, R> {
    content: Rows<'a, K, Message, Theme, R>,
    keys: Vec<K>,
    may: May<'a, K>,
    on_move: Box<dyn Fn(Moved<K>) -> Message + 'a>,
}

/// What the pointer is doing to the list.
#[derive(Clone, Debug, Default)]
enum Drag<K> {
    #[default]
    Idle,
    /// The button went down on the row `key` at `from`; the row has the press until the pointer moves away.
    Pressed { key: K, from: Point },
    /// The row `key` is dragged.
    Dragging { key: K },
    /// A drag ended by Escape with the button still down: its release is no one's press.
    Cancelled,
}

/// What the list keeps between frames: what the pointer is doing to it, and the gap the last frame drew the drop line
/// in.
struct Held<K> {
    drag: Drag<K>,
    gap: Drawn<Option<usize>>,
}

impl<K> Default for Held<K> {
    fn default() -> Self {
        Held { drag: Drag::Idle, gap: Drawn::default() }
    }
}

/// Whether a row of a list is being dragged now, as the list tells an operation: what scrolls a list held at its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dragged(pub(crate) bool);

/// The bounds of the focused control an operation finds.
#[derive(Default)]
struct Focused(Option<Rectangle>);

impl Operation for Focused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.0 = Some(bounds);
        }
    }
}

impl<'a, K: Clone + Eq, Message, R> Reorder<'a, K, Message, R> {
    /// The move that puts row `from` in gap `gap` (0 above the first row, the count below the last), unless the gap
    /// is one of its own two.
    fn moved(&self, from: usize, gap: usize) -> Option<Moved<K>> {
        let key = self.keys.get(from)?.clone();
        (gap != from && gap != from + 1).then(|| Moved { key, before: self.keys.get(gap).cloned() })
    }

    /// Whether some move of row `from` is allowed.
    fn grabbable(&self, from: usize) -> bool {
        (0..=self.keys.len()).filter_map(|gap| self.moved(from, gap)).any(|moved| (self.may)(&moved))
    }

    /// The allowed move a drag of `key` asks with the pointer at `y`, among `rows`, and the gap it lands in.
    /// The gap the drop line is drawn in during `drag`, with the pointer as `cursor` says; none outside a drag or where
    /// the row may not go.
    fn drop_gap(&self, drag: &Drag<K>, cursor: mouse::Cursor, rows: &[Rectangle]) -> Option<usize> {
        let Drag::Dragging { key } = drag else { return None };
        cursor.position().and_then(|at| self.dropped(key, at.y, rows)).map(|(gap, _)| gap)
    }

    fn dropped(&self, key: &K, y: f32, rows: &[Rectangle]) -> Option<(usize, Moved<K>)> {
        let from = self.keys.iter().position(|held| held == key)?;
        let gap = rows.iter().filter(|row| row.center_y() < y).count();
        self.moved(from, gap).filter(|moved| (self.may)(moved)).map(|moved| (gap, moved))
    }
}

/// Each row's bounds, top to bottom.
fn row_bounds(layout: Layout, tree: &Tree) -> Vec<Rectangle> {
    layout.iter(&tree.children[0].children).map(|(row, _)| row.bounds()).collect()
}

/// The row under `point`.
fn row_at(rows: &[Rectangle], point: Point) -> Option<usize> {
    rows.iter().position(|row| row.contains(point))
}

impl<K, Message, R> Meta for Reorder<'_, K, Message, R> {}

impl<K, Message, R> Widget<Message, Theme, R> for Reorder<'_, K, Message, R>
where
    K: Clone + Eq + Hash + 'static,
    R: Renderer,
{
    crate::wrap::forward!(size diff layout overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held<K>>()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        let dragging = matches!(tree.state.downcast_ref::<Held<K>>().drag, Drag::Dragging { .. });
        operation.custom(None, layout.bounds(), &mut Dragged(dragging));
        self.content.operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }

    fn state(&self) -> tree::State {
        tree::State::new(Held::<K>::default())
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
        let rows = row_bounds(layout, tree);
        let drag = tree.state.downcast_ref::<Held<K>>().drag.clone();
        let next = match (&drag, event) {
            (Drag::Idle, Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))) => cursor
                .position()
                .and_then(|from| Some((from, row_at(&rows, from)?)))
                .filter(|(_, at)| self.grabbable(*at))
                .and_then(|(from, at)| Some(Drag::Pressed { key: self.keys.get(at)?.clone(), from })),
            (Drag::Pressed { key, from }, Event::Mouse(mouse::Event::CursorMoved { position }))
                if position.distance(*from) > DRAG_START =>
            {
                Some(Drag::Dragging { key: key.clone() })
            }
            (
                Drag::Dragging { .. },
                Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }),
            ) => {
                tree.state.downcast_mut::<Held<K>>().drag = Drag::Cancelled;
                shell.request_redraw();
                shell.capture_event();
                return;
            }
            _ => None,
        };
        if let Some(next) = next {
            tree.state.downcast_mut::<Held<K>>().drag = next;
            shell.request_redraw();
        }

        // From the drag on, the rows are not told where the pointer is: a row's press is let go without a click.
        let held = matches!(tree.state.downcast_ref::<Held<K>>().drag, Drag::Dragging { .. } | Drag::Cancelled);
        let seen = if held { mouse::Cursor::Unavailable } else { cursor };
        self.content.update(&mut tree.children[0], event, layout, seen, renderer, shell, viewport);
        let state = tree.state.downcast_mut::<Held<K>>();
        let gap = self.drop_gap(&state.drag, cursor, &rows);
        state.gap.follow(event, gap, shell);

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) if held => shell.capture_event(),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let ended = std::mem::take(&mut tree.state.downcast_mut::<Held<K>>().drag);
                if let Drag::Dragging { key } = ended {
                    let dropped = cursor.position().and_then(|at| self.dropped(&key, at.y, &rows));
                    if let Some((_, moved)) = dropped {
                        shell.publish((self.on_move)(moved));
                    }
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(arrow), modifiers, .. })
                if modifiers.alt() && !shell.is_event_captured() =>
            {
                let step: fn(usize, usize) -> Option<usize> = match arrow {
                    Named::ArrowUp => |at, _| at.checked_sub(1),
                    Named::ArrowDown => |at, count| (at + 2 <= count).then_some(at + 2),
                    _ => return,
                };
                let mut focused = Focused::default();
                self.content.operate(&mut tree.children[0], layout, viewport, renderer, &mut focused);
                let Some(at) = focused.0.and_then(|control| row_at(&rows, control.center())) else { return };
                let asked = step(at, self.keys.len()).and_then(|gap| self.moved(at, gap));
                if let Some(moved) = asked.filter(|moved| (self.may)(moved)) {
                    shell.publish((self.on_move)(moved));
                }
                // Taken at either end too, so it never moves the focus or scrolls instead.
                shell.capture_event();
            }
            _ => {}
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
        match &tree.state.downcast_ref::<Held<K>>().drag {
            Drag::Dragging { .. } => mouse::Interaction::Grabbing,
            Drag::Cancelled => mouse::Interaction::default(),
            // Until a drag starts a row is what it shows, as in every peer app: Windows draws a grab as the move cross.
            Drag::Idle | Drag::Pressed { .. } => {
                self.content.mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
            }
        }
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
        let drag = &tree.state.downcast_ref::<Held<K>>().drag;
        let Drag::Dragging { .. } = drag else {
            let seen = if matches!(drag, Drag::Cancelled) { mouse::Cursor::Unavailable } else { cursor };
            self.content.draw(&tree.children[0], renderer, theme, style, layout, seen, viewport);
            return;
        };
        self.content.draw(&tree.children[0], renderer, theme, style, layout, mouse::Cursor::Unavailable, viewport);
        let rows = row_bounds(layout, tree);
        let Some(gap) = self.drop_gap(drag, cursor, &rows) else { return };
        let y = match (gap.checked_sub(1).and_then(|above| rows.get(above)), rows.get(gap)) {
            (Some(above), Some(below)) => (above.y + above.height + below.y) / 2.0,
            (Some(above), None) => above.y + above.height,
            (None, Some(below)) => below.y,
            (None, None) => return,
        };
        let bounds = layout.bounds();
        let inset = Space::Xs.px() + Space::Xxs.px();
        let line = Rectangle {
            x: bounds.x + inset,
            y: y - DRAG_LINE / 2.0,
            width: (bounds.width - 2.0 * inset).max(0.0),
            height: DRAG_LINE,
        };
        // A layer of its own, so no row's text is drawn over it.
        renderer.with_layer(line, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: line,
                    border: iced_core::Border { radius: Radius::Full.px().into(), ..iced_core::Border::default() },
                    ..renderer::Quad::default()
                },
                Background::Color(theme.color(Role::Accent(AccentRole::Fill))),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Button;
    use crate::testing::Renderer;
    use iced_core::event::Status;
    use iced_core::keyboard::Modifiers;
    use iced_test::Simulator;

    const NAMES: [&str; 4] = ["Owner", "Admin", "Moderators", "Regulars"];

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Pressed(usize),
        Moved(Moved<usize>),
    }

    type Sim = Simulator<'static, Said, Theme, Renderer>;

    /// Four rows, each a button [`design::CONTROL_HEIGHT`] tall, so row `i` spans `40 i` to `40 (i + 1)`.
    fn list(may: impl Fn(&Moved<usize>) -> bool + 'static) -> Sim {
        crate::testing::hermetic_fonts();
        let rows = NAMES.iter().enumerate().map(|(key, name)| {
            let row =
                Button::new(Label::new(name).unwrap()).width(iced_core::Length::Fill).on_press(Said::Pressed(key));
            (key, row.view::<Renderer>())
        });
        let list = reorder(Label::new("Roles").unwrap(), rows, may, Said::Moved);
        Simulator::with_size(iced_core::Settings::default(), iced_core::Size::new(300.0, 200.0), list)
    }

    fn centre(row: usize) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "a row's place is small")]
        let row = row as f32;
        design::CONTROL_HEIGHT * (row + 0.5)
    }

    fn press_at(sim: &mut Sim, y: f32) {
        sim.point_at(Point::new(100.0, y));
        sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: Point::new(100.0, y) })]);
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
    }

    fn move_to(sim: &mut Sim, y: f32) {
        sim.point_at(Point::new(100.0, y));
        sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: Point::new(100.0, y) })]);
    }

    fn release(sim: &mut Sim) {
        sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
    }

    fn drag(sim: &mut Sim, from: f32, to: f32) {
        press_at(sim, from);
        move_to(sim, to);
        release(sim);
    }

    fn alt(sim: &mut Sim, arrow: Named, alt: bool) -> Status {
        let key = Key::Named(arrow);
        let modifiers = if alt { Modifiers::ALT } else { Modifiers::empty() };
        let pressed = Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        });
        sim.simulate([pressed])[0]
    }

    fn said(sim: &mut Sim) -> Vec<Said> {
        sim.drain().collect()
    }

    fn moved(key: usize, before: Option<usize>) -> Said {
        Said::Moved(Moved { key, before })
    }

    /// The cursor a list shows with the pointer at `y` after each of `events`, the button held where they press it.
    fn cursors(steps: &[(f32, Event)]) -> Vec<mouse::Interaction> {
        use iced_runtime::user_interface::{Cache, State, UserInterface};
        crate::testing::hermetic_fonts();
        let rows = NAMES.iter().enumerate().map(|(key, name)| {
            let row =
                Button::new(Label::new(name).unwrap()).width(iced_core::Length::Fill).on_press(Said::Pressed(key));
            (key, row.view::<Renderer>())
        });
        let list = reorder(Label::new("Roles").unwrap(), rows, |_| true, Said::Moved);
        let mut renderer = Renderer::new(crate::fonts::settings());
        let size = iced_core::Size::new(300.0, 200.0);
        let mut interface = UserInterface::build(list, size, Cache::default(), &mut renderer);
        let mut messages = iced_core::shell::Bus::new();
        steps
            .iter()
            .map(|(y, event)| {
                let cursor = mouse::Cursor::Available(Point::new(100.0, *y));
                let (state, _) = interface.update(
                    &iced_core::window::Headless,
                    &iced_core::shell::Waker::noop(),
                    std::slice::from_ref(event),
                    cursor,
                    &mut renderer,
                    &mut messages,
                );
                match state {
                    State::Updated { mouse_interaction, .. } => mouse_interaction,
                    State::Outdated => mouse::Interaction::None,
                }
            })
            .collect()
    }

    /// A row is pressed far more often than it is dragged: it shows the pointer, as a channel does in every peer app,
    /// and the grabbing hand only once a drag has started.
    #[test]
    fn a_row_shows_the_pointer_until_a_drag_starts() {
        let at = |y: f32| Event::Mouse(mouse::Event::CursorMoved { position: Point::new(100.0, y) });
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let (from, to) = (centre(1), centre(1) + DRAG_START + 4.0);
        let shown = cursors(&[(from, at(from)), (from, press), (to, at(to))]);
        assert_eq!(
            shown,
            [mouse::Interaction::Pointer, mouse::Interaction::Pointer, mouse::Interaction::Grabbing],
            "hovered, pressed, dragged"
        );
    }

    #[test]
    fn a_row_dragged_between_two_others_moves_before_the_lower() {
        let mut sim = list(|_| true);
        drag(&mut sim, centre(3), design::CONTROL_HEIGHT + 5.0);
        assert_eq!(said(&mut sim), [moved(3, Some(1))], "Regulars between Owner and Admin, and not pressed");
        drag(&mut sim, centre(0), centre(3) + 10.0);
        assert_eq!(said(&mut sim), [moved(0, None)], "below the last row is last");
    }

    #[test]
    fn a_drag_to_a_gap_it_may_not_reach_moves_nothing() {
        let mut sim = list(|moved| moved.before != Some(0));
        drag(&mut sim, centre(2), 5.0);
        assert_eq!(said(&mut sim), [], "nothing may go above the first row");
        drag(&mut sim, centre(2), centre(2) + 15.0);
        assert_eq!(said(&mut sim), [], "its own gap is no move");
        drag(&mut sim, centre(2), centre(0) + 5.0);
        assert_eq!(said(&mut sim), [moved(2, Some(1))], "an allowed gap still moves it");
    }

    #[test]
    fn a_press_without_a_drag_presses_the_row_and_a_drag_does_not() {
        let mut sim = list(|_| true);
        drag(&mut sim, centre(1), centre(1) + DRAG_START - 1.0);
        assert_eq!(said(&mut sim), [Said::Pressed(1)], "a hand's jitter is still a press");
        drag(&mut sim, centre(1), centre(1) + DRAG_START + 10.0);
        assert_eq!(said(&mut sim), [], "a drag lets the row's press go");
        let mut fixed = list(|moved| moved.key != 1);
        drag(&mut fixed, centre(1), centre(3));
        assert_eq!(said(&mut fixed), [], "a row that cannot move is never dragged, so the release is off it");
        drag(&mut fixed, centre(1), centre(1) + DRAG_START + 10.0);
        assert_eq!(said(&mut fixed), [Said::Pressed(1)], "and a press on it is its own");
    }

    #[test]
    fn alt_up_and_down_move_the_focused_row_one_place_and_are_taken_at_the_ends() {
        let mut sim = list(|_| true);
        assert_eq!(alt(&mut sim, Named::ArrowDown, true), Status::Ignored, "no row has the focus");
        drag(&mut sim, centre(0), centre(0));
        said(&mut sim);
        assert_eq!(alt(&mut sim, Named::ArrowUp, true), Status::Captured, "the first row goes no higher");
        assert_eq!(said(&mut sim), []);
        assert_eq!(alt(&mut sim, Named::ArrowDown, true), Status::Captured);
        assert_eq!(said(&mut sim), [moved(0, Some(2))], "down one place is before the row after next");
        assert_eq!(alt(&mut sim, Named::ArrowDown, false), Status::Ignored, "an arrow alone is not a move");
        drag(&mut sim, centre(2), centre(2));
        said(&mut sim);
        assert_eq!(alt(&mut sim, Named::ArrowDown, true), Status::Captured);
        assert_eq!(said(&mut sim), [moved(2, None)], "down from the last but one is last");
        drag(&mut sim, centre(3), centre(3));
        said(&mut sim);
        assert_eq!(alt(&mut sim, Named::ArrowDown, true), Status::Captured, "the last row goes no lower");
        assert_eq!(said(&mut sim), []);
        assert_eq!(alt(&mut sim, Named::ArrowUp, true), Status::Captured);
        assert_eq!(said(&mut sim), [moved(3, Some(2))]);
        let mut sim = list(|moved| moved.before != Some(2));
        drag(&mut sim, centre(3), centre(3));
        said(&mut sim);
        assert_eq!(alt(&mut sim, Named::ArrowUp, true), Status::Captured, "a refused move is still taken");
        assert_eq!(said(&mut sim), []);
    }

    #[test]
    fn escape_ends_a_drag_without_a_move() {
        let mut sim = list(|_| true);
        press_at(&mut sim, centre(3));
        move_to(&mut sim, centre(0) + 5.0);
        let escape = sim.simulate([crate::testing::key_press(Key::Named(Named::Escape), false)]);
        assert_eq!(escape, [Status::Captured]);
        move_to(&mut sim, centre(3));
        release(&mut sim);
        assert_eq!(said(&mut sim), [], "neither a move nor the press it began as");
        drag(&mut sim, centre(3), centre(0) + 5.0);
        assert_eq!(said(&mut sim), [moved(3, Some(1))], "the next drag is a drag again");
    }

    #[test]
    fn the_list_is_said_as_one_and_its_rows_keep_their_names() {
        let mut sim = list(|_| true);
        let tree = crate::access::tree(iced_core::Size::new(300.0, 200.0), 1.0, None, |operation| {
            sim.operate(operation);
        });
        let named = |name: &str| {
            tree.nodes.iter().find(|(_, node)| node.label() == Some(name)).unwrap_or_else(|| panic!("no {name}"))
        };
        let (list, node) = named("Roles");
        assert_eq!(node.role(), accesskit::Role::List);
        for name in NAMES {
            let (row, node) = named(name);
            assert_eq!(node.role(), accesskit::Role::Button, "{name}");
            assert!(tree.nodes.iter().any(|(id, node)| id == list && node.children().contains(row)), "{name}");
        }
    }

    /// What the list asks for with row 1 held and dragged, drawn with the pointer at `drawn` and moved to `now`.
    fn asked_dragging(drawn: f32, now: f32) -> iced_core::window::RedrawRequest {
        let list = || {
            let rows = NAMES.iter().enumerate().map(|(key, name)| {
                let row =
                    Button::new(Label::new(name).unwrap()).width(iced_core::Length::Fill).on_press(Said::Pressed(key));
                (key, row.view::<Renderer>())
            });
            reorder(Label::new("Roles").unwrap(), rows, |_| true, Said::Moved)
        };
        let at = |y: f32| Point::new(100.0, y);
        let moved = |y: f32| Event::Mouse(mouse::Event::CursorMoved { position: at(y) });
        let (from, away) = (centre(1), centre(1) + DRAG_START + 4.0);
        let held = [
            (at(from), moved(from)),
            (at(from), Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))),
            (at(away), moved(away)),
            (at(drawn), moved(drawn)),
        ];
        let size = iced_core::Size::new(300.0, 200.0);
        crate::drawn::asked_after_held(list, size, &held, at(drawn), at(now), &[moved(now)])
    }

    #[test]
    fn a_drag_moved_into_another_gap_asks_for_a_frame() {
        let (into_three, into_four) = (centre(2) + 5.0, centre(3) + 5.0);
        assert_eq!(asked_dragging(into_three, into_four), iced_core::window::RedrawRequest::NextFrame);
    }

    #[test]
    fn a_drag_moved_within_its_gap_asks_nothing() {
        let (drawn, now) = (centre(3) + 2.0, centre(3) + 10.0);
        assert_eq!(asked_dragging(drawn, now), iced_core::window::RedrawRequest::Wait);
    }
}
