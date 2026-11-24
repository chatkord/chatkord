//! A zoomed picture under the pointer: the wheel zooms it toward the pointer and a drag moves it, ended wherever the
//! button is let go. A press goes on to what holds the picture, so a double-click still reaches it.

use crate::Theme;
use iced_core::widget::{Meta, Tree, tree};
use iced_core::{
    Element, Event, Layout, Length, Point, Rectangle, Shell, Size, Vector, Widget, layout, mouse, renderer,
};

/// Where a drag last was, while the button is held.
#[derive(Clone, Copy, Debug, Default)]
struct Dragging(Option<Point>);

/// What the wheel sends: tenths in (positive) or out, where the pointer is in the picture's area, and its size.
type OnWheel<'a, Message> = Box<dyn Fn(i8, Point, Size) -> Message + 'a>;
/// What a drag sends: how far the pointer moved, and the picture's area's size.
type OnDrag<'a, Message> = Box<dyn Fn(Vector, Size) -> Message + 'a>;

pub(crate) struct Zoomed<'a, Message, R> {
    content: Element<'a, Message, Theme, R>,
    on_wheel: OnWheel<'a, Message>,
    on_drag: OnDrag<'a, Message>,
}

/// `content` zoomed: the wheel over it sends `on_wheel`, a drag from it `on_drag`, and the pointer is a hand that grabs.
pub(crate) fn zoomed<'a, Message: 'a, R: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    on_wheel: impl Fn(i8, Point, Size) -> Message + 'a,
    on_drag: impl Fn(Vector, Size) -> Message + 'a,
) -> Element<'a, Message, Theme, R> {
    Zoomed { content: content.into(), on_wheel: Box::new(on_wheel), on_drag: Box::new(on_drag) }._boxed()
}

impl<Message, R> Meta for Zoomed<'_, Message, R> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for Zoomed<'_, Message, R> {
    crate::wrap::forward!(diff operate draw overlay);

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    /// The whole room it is given, as the picture under it takes: the wheel and a drag reach it anywhere there.
    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        self.content.layout(&mut tree.children[0], renderer, limits);
        tree.size = layout::atomic(limits, Length::Fill, Length::Fill);
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Dragging>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Dragging::default())
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
        if shell.is_event_captured() {
            return;
        }
        let bounds = layout.bounds();
        let dragging = tree.state.downcast_mut::<Dragging>();
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let (mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. }) = *delta;
                if let Some(at) = cursor.position_in(bounds)
                    && y != 0.0
                {
                    let notches = if y > 0.0 { 1 } else { -1 };
                    shell.publish((self.on_wheel)(notches, at, bounds.size()));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                dragging.0 = cursor.position_over(bounds);
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(last) = dragging.0 {
                    dragging.0 = Some(*position);
                    shell.publish((self.on_drag)(*position - last, bounds.size()));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => dragging.0 = None,
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        match (tree.state.downcast_ref::<Dragging>().0, cursor.is_over(layout.bounds())) {
            (Some(_), _) => mouse::Interaction::Grabbing,
            (None, true) => mouse::Interaction::Grab,
            (None, false) => mouse::Interaction::None,
        }
    }
}
