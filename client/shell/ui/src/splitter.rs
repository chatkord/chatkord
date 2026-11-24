//! The splitter on the rule between the server pane and the content: dragged, stepped by the arrow keys while it has
//! the focus, or pressed twice to put the pane back to its least. A drag sends each width as it moves and is kept once,
//! when it is let go.

use crate::drawn::Drawn;
use crate::{Focus, Renderer, Theme};
use design::{AccentRole, DRAG_LINE, Label, Role, SPLITTER_GRIP, SemanticRole, Semantics};
use iced_core::keyboard::{Key, key::Named};
use iced_core::mouse::click::{Click, Kind};
use iced_core::widget::{Tree, tree};
use iced_core::{Background, Element, Event, Layout, Length, Rectangle, Shell, Size, Widget, layout, mouse, renderer};
use screens::frame::PanePick;

/// Which way the column a splitter sizes grows: the edge the splitter is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grows {
    /// Toward the window's right: the side pane, its splitter on its right edge.
    Right,
    /// Toward the window's left: the members column, its splitter on its left edge.
    Left,
}

/// A splitter for a column `width` wide that may be dragged from `least` to `most`, growing as `grows` says, called
/// `label` for a screen reader; what it does is sent through `on`.
pub(crate) fn splitter<'a, Message: Clone + 'a, R: Renderer + 'a>(
    label: Label,
    width: f32,
    (least, most): (f32, f32),
    grows: Grows,
    on: impl Fn(PanePick) -> Message + Clone + 'a,
) -> Element<'a, Message, Theme, R> {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a pane's width is whole and small")]
    let whole = |px: f32| px.round().max(0.0) as u16;
    let semantics =
        Semantics { role: SemanticRole::Slider { value: whole(width), least: whole(least), most: whole(most) }, label };
    let keyed = on.clone();
    // A key at the end it moves toward is still taken, so it never moves the focus or scrolls instead.
    let (left, right) = match grows {
        Grows::Right => (PanePick::Narrower, PanePick::Wider),
        Grows::Left => (PanePick::Wider, PanePick::Narrower),
    };
    let on_key = move |key: &Key| match key {
        Key::Named(Named::ArrowLeft) => Some(keyed(left)),
        Key::Named(Named::ArrowRight) => Some(keyed(right)),
        Key::Named(Named::Home) => Some(keyed(PanePick::Reset)),
        _ => None,
    };
    Focus::new(Grip { width, grows, on: Box::new(on) }, semantics).on_key(on_key)._boxed()
}

/// The grip a pointer drags.
struct Grip<'a, Message> {
    width: f32,
    grows: Grows,
    on: Box<dyn Fn(PanePick) -> Message + 'a>,
}

/// Where a drag began, the pointer's x and the pane's width then, while a press that began on the grip is held; the
/// last press, so a second one soon after at the same place is told from a drag; and whether the last frame drew the
/// line.
#[derive(Default)]
struct Held {
    from: Option<(f32, f32)>,
    last: Option<Click>,
    line: Drawn<bool>,
}

impl Held {
    /// Whether the line shows: while dragged, or with the pointer on the grip.
    fn shows_line(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        self.from.is_some() || cursor.is_over(bounds)
    }
}

impl<Message> iced_core::widget::Meta for Grip<'_, Message> {}

impl<Message, R: Renderer> Widget<Message, Theme, R> for Grip<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Held::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(SPLITTER_GRIP), Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        tree.size = layout::atomic(limits, Length::Fixed(SPLITTER_GRIP), Length::Fill);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let held = tree.state.downcast_mut::<Held>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(layout.bounds()) {
                    let click = Click::new(point, mouse::Button::Left, held.last);
                    held.last = Some(click);
                    if click.kind() == Kind::Double {
                        held.from = None;
                        shell.publish((self.on)(PanePick::Reset));
                    } else {
                        held.from = Some((point.x, self.width));
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some((x, width)) = held.from {
                    let moved = match self.grows {
                        Grows::Right => position.x - x,
                        Grows::Left => x - position.x,
                    };
                    shell.publish((self.on)(PanePick::To(width + moved)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if held.from.take().is_some() => {
                shell.publish((self.on)(PanePick::Let));
            }
            _ => {}
        }
        let line = held.shows_line(layout.bounds(), cursor);
        held.line.follow(event, line, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<Held>().shows_line(layout.bounds(), cursor) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !tree.state.downcast_ref::<Held>().shows_line(bounds, cursor) {
            return;
        }
        let line = Rectangle { x: bounds.center_x() - DRAG_LINE / 2.0, width: DRAG_LINE, ..bounds };
        renderer.fill_quad(
            renderer::Quad { bounds: line, ..renderer::Quad::default() },
            Background::Color(theme.color(Role::Accent(AccentRole::Fill))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawn::asked_after;
    use iced_core::window::RedrawRequest;
    use iced_core::{Point, Size};

    const SIZE: Size = Size::new(SPLITTER_GRIP, 200.0);

    fn asked(drawn: Point, now: Point) -> RedrawRequest {
        let grip = || splitter(Label::new("Pane").unwrap(), 240.0, (200.0, 400.0), Grows::Right, |_| ());
        asked_after::<()>(grip, SIZE, drawn, now, &[Event::Mouse(mouse::Event::CursorMoved { position: now })])
    }

    #[test]
    fn the_pointer_coming_onto_the_grip_or_leaving_it_asks_for_a_frame() {
        let (on, off) = (Point::new(SPLITTER_GRIP / 2.0, 100.0), Point::new(SPLITTER_GRIP * 4.0, 100.0));
        assert_eq!(asked(off, on), RedrawRequest::NextFrame, "the line shows");
        assert_eq!(asked(on, off), RedrawRequest::NextFrame, "and goes");
    }

    #[test]
    fn a_move_along_the_grip_asks_nothing() {
        let along = |y: f32| Point::new(SPLITTER_GRIP / 2.0, y);
        assert_eq!(asked(along(40.0), along(160.0)), RedrawRequest::Wait);
    }
}
