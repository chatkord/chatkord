//! A list a drag scrolls: while a row of a list inside it is dragged and the pointer is held near the top or bottom
//! edge, the scrollable inside scrolls toward that edge, faster the nearer the pointer is to it.

use crate::Theme;
use crate::reorder::Dragged;
use design::{DRAG_EDGE, DRAG_SCROLL};
use iced_core::widget::operation::{Animation, Operation, scrollable::scroll_by};
use iced_core::widget::{Id, Tree, tree};
use iced_core::{Element, Event, Layout, Point, Rectangle, Shell, Widget, mouse, renderer, window};
use iced_widget::scrollable::AbsoluteOffset;
use std::time::Instant;

/// `content`, holding the scrollable `id` and a [`crate::reorder`] list in it, scrolled while a drag is held at its
/// top or bottom edge.
pub(crate) fn edge_scroll<'a, Message: 'a, R: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    id: Id,
) -> Element<'a, Message, Theme, R> {
    EdgeScroll { content: content.into(), id }._boxed()
}

struct EdgeScroll<'a, Message, R> {
    content: Element<'a, Message, Theme, R>,
    id: Id,
}

/// When the last scroll of a held drag was made; none while nothing scrolls.
#[derive(Default)]
struct Scrolling(Option<Instant>);

/// Finds whether a list inside is dragged.
#[derive(Default)]
struct FindDrag(bool);

impl Operation for FindDrag {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn std::any::Any) {
        if let Some(Dragged(true)) = state.downcast_ref::<Dragged>() {
            self.0 = true;
        }
    }
}

/// How fast a drag at `at` scrolls a list in `bounds`, in pixels a second down (up when negative): none away from
/// both edges.
fn speed(bounds: Rectangle, at: Point) -> Option<f32> {
    let edge = DRAG_EDGE.min(bounds.height / 2.0);
    let from_top = at.y - bounds.y;
    let from_bottom = bounds.y + bounds.height - at.y;
    let depth = |from: f32| ((edge - from) / edge).clamp(0.0, 1.0);
    if (0.0..edge).contains(&from_top) {
        Some(-DRAG_SCROLL * depth(from_top))
    } else if (0.0..edge).contains(&from_bottom) {
        Some(DRAG_SCROLL * depth(from_bottom))
    } else {
        None
    }
}

impl<Message, R> iced_core::widget::Meta for EdgeScroll<'_, Message, R> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for EdgeScroll<'_, Message, R> {
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Scrolling>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Scrolling::default())
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
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            Event::Mouse(mouse::Event::CursorMoved { .. }) => None,
            _ => return,
        };
        let mut find = FindDrag::default();
        self.content.operate(&mut tree.children[0], layout, viewport, renderer, &mut find);
        let speed = cursor.position().filter(|_| find.0).and_then(|at| speed(layout.bounds(), at));
        let scrolling: &mut Scrolling = tree.state.downcast_mut();
        let Some(speed) = speed else {
            scrolling.0 = None;
            return;
        };
        // Each frame scrolls by the time since the one before, so the speed holds whatever the frame rate.
        if let Some(now) = now
            && let Some(last) = scrolling.0.replace(now)
        {
            let by = speed * now.saturating_duration_since(last).as_secs_f32();
            let mut moved = scroll_by(self.id.clone(), AbsoluteOffset { x: 0.0, y: by }, Animation::Instant);
            self.content.operate(&mut tree.children[0], layout, viewport, renderer, &mut moved);
        }
        shell.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: Rectangle = Rectangle { x: 0.0, y: 100.0, width: 200.0, height: 400.0 };

    #[test]
    fn a_drag_scrolls_toward_the_edge_it_is_held_at_faster_nearer_it() {
        assert_eq!(speed(LIST, Point::new(50.0, 300.0)), None, "away from both edges");
        assert_eq!(speed(LIST, Point::new(50.0, 100.0 + DRAG_EDGE)), None, "just past the edge's reach");
        let up = speed(LIST, Point::new(50.0, 100.0 + DRAG_EDGE / 2.0)).expect("near the top");
        assert!(up < 0.0 && up > -DRAG_SCROLL, "{up}");
        let down = speed(LIST, Point::new(50.0, 499.0)).expect("at the bottom");
        assert!(down > DRAG_SCROLL * 0.9, "{down}");
        assert_eq!(speed(LIST, Point::new(50.0, 90.0)), None, "outside it");
    }

    /// How far the scrollable inside is scrolled down.
    #[derive(Default)]
    struct Scrolled(Option<f32>);

    impl Operation for Scrolled {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            _id: Option<&Id>,
            _bounds: Rectangle,
            _content: iced_core::Size,
            translation: iced_core::Vector,
            _state: &mut dyn iced_core::widget::operation::Scrollable,
        ) {
            self.0 = Some(translation.y);
        }
    }

    #[test]
    fn a_drag_held_at_the_bottom_edge_scrolls_the_list_down() {
        use crate::Button;
        use crate::testing::Renderer;
        use design::Label;
        use iced_test::Simulator;

        fn at(sim: &mut Simulator<'_, (), Theme, Renderer>, y: f32) {
            sim.point_at(Point::new(100.0, y));
            sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: Point::new(100.0, y) })]);
        }

        crate::testing::hermetic_fonts();
        let rows = (0..20_usize).map(|key| {
            let row = Button::new(Label::new(&format!("Row {key}")).unwrap()).width(iced_core::Length::Fill);
            (key, row.view::<Renderer>())
        });
        let list = crate::reorder(Label::new("Rows").unwrap(), rows, |_| true, |_| ());
        let id = Id::new("edge-scrolled");
        let scrolled = iced_widget::scrollable::<(), _, Theme>(list).id(id.clone()).height(iced_core::Length::Fill);
        let size = iced_core::Size::new(300.0, 200.0);
        let mut sim: Simulator<'_, (), Theme, Renderer> = Simulator::with_size(
            iced_core::Settings::default(),
            size,
            edge_scroll(Widget::<(), Theme, Renderer>::_boxed(scrolled), id),
        );
        at(&mut sim, design::CONTROL_HEIGHT * 1.5);
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        at(&mut sim, size.height - DRAG_EDGE / 4.0);
        let start = Instant::now();
        let later = start + std::time::Duration::from_millis(100);
        sim.simulate([Event::Window(window::Event::RedrawRequested(start))]);
        sim.simulate([Event::Window(window::Event::RedrawRequested(later))]);
        // A scrollable reports the translation of the frame it last drew, and the scroll comes after its own update.
        sim.simulate([Event::Window(window::Event::RedrawRequested(later))]);
        let mut scrolled = Scrolled::default();
        sim.operate(&mut scrolled);
        let by = scrolled.0.expect("the list is scrollable");
        assert!(by > DRAG_SCROLL * 0.1 * 0.5, "a tenth of a second held near the bottom scrolls it down: {by}");
    }
}
