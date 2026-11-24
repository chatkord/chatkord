//! A slider: a value along a whole-number range, dragged on its rail or stepped by the keys while it has the focus.
//! A move sends where it is now; a release, or each key's step, sends the value to keep, so a drag is kept once.

use crate::drawn::Drawn;
use crate::{Focus, Renderer, Theme};
use design::{Interaction, Label, Radius, SLIDER_HANDLE, SLIDER_HEIGHT, SLIDER_RAIL, SemanticRole, Semantics};
use iced_core::keyboard::{Key, key::Named};
use iced_core::widget::{Tree, tree};
use iced_core::{
    Background, Border, Element, Event, Layout, Length, Point, Rectangle, Shell, Size, Widget, layout, mouse, renderer,
};
use std::ops::RangeInclusive;

/// How far Page Up and Page Down move, in steps.
const PAGE: u16 = 10;

/// A slider over `range` at `value`, called `label` for a screen reader. `on_change` is sent as it moves, `on_commit`
/// with the value to keep when a drag ends or a key moves it.
pub fn slider<'a, Message: Clone + 'a, R: Renderer + 'a>(
    label: Label,
    range: RangeInclusive<u16>,
    value: u16,
    on_change: impl Fn(u16) -> Message + 'a,
    on_commit: impl Fn(u16) -> Message + 'a,
) -> Element<'a, Message, Theme, R> {
    let (least, most) = (*range.start(), *range.end());
    let value = value.clamp(least, most);
    let semantics = Semantics { role: SemanticRole::Slider { value, least, most }, label };
    let on_commit = std::rc::Rc::new(on_commit);
    let committing = std::rc::Rc::clone(&on_commit);
    let on_key = move |key: &Key| {
        let to = match key {
            Key::Named(Named::ArrowLeft | Named::ArrowDown) => value.saturating_sub(1).max(least),
            Key::Named(Named::ArrowRight | Named::ArrowUp) => value.saturating_add(1).min(most),
            Key::Named(Named::PageDown) => value.saturating_sub(PAGE).max(least),
            Key::Named(Named::PageUp) => value.saturating_add(PAGE).min(most),
            Key::Named(Named::Home) => least,
            Key::Named(Named::End) => most,
            _ => return None,
        };
        // A key at the end it moves toward is still taken, so it never moves the focus or scrolls instead.
        Some(committing(to))
    };
    let moved = Moved { on_change: Box::new(on_change), on_commit: Box::new(move |at| on_commit(at)) };
    let rail = Rail { least, most, value, moved: Some(moved) };
    Focus::new(rail, semantics).on_key(on_key)._boxed()
}

/// [`slider`] that cannot be used now: drawn where `value` is, in the disabled roles, taking no pointer, key or focus.
/// It waits on another choice.
pub fn slider_unoffered<'a, Message: 'a, R: Renderer + 'a>(
    label: Label,
    range: RangeInclusive<u16>,
    value: u16,
) -> Element<'a, Message, Theme, R> {
    let (least, most) = (*range.start(), *range.end());
    let value = value.clamp(least, most);
    let semantics = Semantics { role: SemanticRole::Slider { value, least, most }, label };
    crate::Labelled::new(Rail::<Message> { least, most, value, moved: None }, semantics)._boxed()
}

/// What moving a slider sends.
struct Moved<'a, Message> {
    on_change: Box<dyn Fn(u16) -> Message + 'a>,
    on_commit: Box<dyn Fn(u16) -> Message + 'a>,
}

/// The rail and its handle, which a pointer drags.
struct Rail<'a, Message> {
    least: u16,
    most: u16,
    value: u16,
    /// None: it cannot be used now.
    moved: Option<Moved<'a, Message>>,
}

/// Where a drag is: the value it last sent, while a press that began on the rail is held; and how the last frame drew
/// the rail.
#[derive(Default)]
struct Dragging {
    at: Option<u16>,
    drawn: Drawn<Interaction>,
}

impl<Message> Rail<'_, Message> {
    /// The value under `x`: the nearest step, the ends past either side.
    fn at(&self, bounds: Rectangle, x: f32) -> u16 {
        let travel = (bounds.width - SLIDER_HANDLE).max(1.0);
        let along = ((x - bounds.x - SLIDER_HANDLE / 2.0) / travel).clamp(0.0, 1.0);
        let span = f32::from(self.most - self.least);
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "rounded within 0 and the span")]
        let steps = (along * span).round() as u16;
        self.least + steps
    }

    /// How the rail is drawn, dragged or not, with the pointer as `cursor` says.
    fn interaction(&self, dragged: Option<u16>, bounds: Rectangle, cursor: mouse::Cursor) -> Interaction {
        match (&self.moved, dragged, cursor.is_over(bounds)) {
            (None, _, _) => Interaction::Disabled,
            (Some(_), Some(_), _) => Interaction::Pressed,
            (Some(_), None, true) => Interaction::Hover,
            (Some(_), None, false) => Interaction::Idle,
        }
    }

    /// How far along the rail `value` is, from 0 to 1.
    fn along(&self, value: u16) -> f32 {
        let span = f32::from(self.most - self.least);
        if span == 0.0 { 0.0 } else { f32::from(value - self.least) / span }
    }
}

impl<Message> iced_core::widget::Meta for Rail<'_, Message> {}

impl<Message, R: Renderer> Widget<Message, Theme, R> for Rail<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Dragging>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Dragging::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(SLIDER_HEIGHT))
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        tree.size = layout::atomic(limits, Length::Fill, Length::Fixed(SLIDER_HEIGHT));
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
        let Some(moved) = &self.moved else { return };
        let bounds = layout.bounds();
        let dragging = tree.state.downcast_mut::<Dragging>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(bounds) {
                    let to = self.at(bounds, point.x);
                    dragging.at = Some(to);
                    if to != self.value {
                        shell.publish((moved.on_change)(to));
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if dragging.at.is_some() => {
                let to = self.at(bounds, position.x);
                if dragging.at != Some(to) {
                    dragging.at = Some(to);
                    shell.publish((moved.on_change)(to));
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(to) = dragging.at.take() {
                    shell.publish((moved.on_commit)(to));
                }
            }
            _ => {}
        }
        let now = self.interaction(dragging.at, bounds, cursor);
        dragging.drawn.follow(event, now, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        // A hand, as a button's: Windows draws a grab as the four-way move cross.
        if self.moved.is_none() {
            mouse::Interaction::default()
        } else if tree.state.downcast_ref::<Dragging>().at.is_some() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
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
        let dragged = tree.state.downcast_ref::<Dragging>().at;
        let look = design::slider(self.interaction(dragged, bounds, cursor));
        let shown = dragged.unwrap_or(self.value);
        let travel = bounds.width - SLIDER_HANDLE;
        let centre =
            Point::new(bounds.x + SLIDER_HANDLE / 2.0 + self.along(shown) * travel, bounds.y + bounds.height / 2.0);
        let line = |from: f32, to: f32| Rectangle {
            x: from,
            y: centre.y - SLIDER_RAIL / 2.0,
            width: (to - from).max(0.0),
            height: SLIDER_RAIL,
        };
        let round = |radius: f32| Border { radius: radius.into(), ..Border::default() };
        let mut fill = |area: Rectangle, radius: f32, role| {
            let quad = renderer::Quad { bounds: area, border: round(radius), ..renderer::Quad::default() };
            renderer.fill_quad(quad, Background::Color(theme.color(role)));
        };
        let (start, end) = (bounds.x + SLIDER_HANDLE / 2.0, bounds.x + bounds.width - SLIDER_HANDLE / 2.0);
        fill(line(start, end), Radius::Full.px().min(SLIDER_RAIL), look.rest);
        fill(line(start, centre.x), Radius::Full.px().min(SLIDER_RAIL), look.filled);
        let handle = Rectangle::new(
            Point::new(centre.x - SLIDER_HANDLE / 2.0, centre.y - SLIDER_HANDLE / 2.0),
            Size::new(SLIDER_HANDLE, SLIDER_HANDLE),
        );
        fill(handle, Radius::Full.px().min(SLIDER_HANDLE), look.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawn::asked_after;
    use iced_core::window::RedrawRequest;

    const SIZE: Size = Size::new(200.0, SLIDER_HEIGHT);

    fn asked(drawn: Point, now: Point) -> RedrawRequest {
        let rail = || slider(Label::new("Volume").unwrap(), 0..=100, 50, |_| (), |_| ());
        asked_after::<()>(rail, SIZE, drawn, now, &[Event::Mouse(mouse::Event::CursorMoved { position: now })])
    }

    #[test]
    fn the_pointer_coming_onto_the_rail_or_leaving_it_asks_for_a_frame() {
        let (on, off) = (Point::new(100.0, SLIDER_HEIGHT / 2.0), Point::new(100.0, SLIDER_HEIGHT * 3.0));
        assert_eq!(asked(off, on), RedrawRequest::NextFrame, "drawn hovered");
        assert_eq!(asked(on, off), RedrawRequest::NextFrame, "and idle again");
    }

    #[test]
    fn a_move_along_the_rail_asks_nothing() {
        let along = |x: f32| Point::new(x, SLIDER_HEIGHT / 2.0);
        assert_eq!(asked(along(20.0), along(180.0)), RedrawRequest::Wait);
    }
}
