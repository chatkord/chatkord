//! A wait whose end is not known: the loader circle, turning while it is in the tree and still where the system asks
//! for less motion. Its first frame is unturned, so a reference image of it is the same every time.

use crate::Theme;
use crate::icons::{Icon, handle};
use design::{MotionPreference, Role, SPIN_TURN, SPINNER_SIZE};
use iced_core::widget::{Tree, tree};
use iced_core::{
    Element, Event, Layout, Length, Radians, Rectangle, Shell, Size, Widget, layout, mouse, renderer, svg, window,
};
use std::cell::Cell;
use std::f32::consts::TAU;
use std::time::{Duration, Instant};

/// When it began turning and where it is now, and whether the last frame was drawn still. Only `draw` sees the theme,
/// and it sees the state read-only, so that word is a `Cell` it sets for the next `update`.
#[derive(Clone, Debug, Default)]
struct State {
    begun: Option<Instant>,
    angle: f32,
    still: Cell<bool>,
}

/// How far the circle has turned after `since`, in radians from 0 up to a whole turn.
fn turned(since: Duration) -> f32 {
    (since.as_secs_f32() / SPIN_TURN.as_secs_f32()).fract() * TAU
}

/// The circle, in `role`'s colour.
pub(crate) struct Spinner {
    role: Role,
}

/// The turning circle in `role`'s colour; it holds still when the system asks for less motion.
pub fn spinner<'a, Message: 'a, R: crate::Renderer + 'a>(role: Role) -> Element<'a, Message, Theme, R> {
    Spinner { role }._boxed()
}

impl iced_core::widget::Meta for Spinner {}

impl<Message, R: crate::Renderer> Widget<Message, Theme, R> for Spinner {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(SPINNER_SIZE), Length::Fixed(SPINNER_SIZE))
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, _limits: &layout::Limits) {
        tree.size = Size::new(SPINNER_SIZE, SPINNER_SIZE);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let state = tree.state.downcast_mut::<State>();
            if state.still.get() {
                return;
            }
            let begun = *state.begun.get_or_insert(*now);
            state.angle = turned(now.saturating_duration_since(begun));
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let still = theme.motion() == MotionPreference::Reduced;
        state.still.set(still);
        let angle = if still { 0.0 } else { state.angle };
        let circle = svg::Svg::new(handle(Icon::Spinner)).color(theme.color(self.role)).rotation(Radians(angle));
        renderer.draw_svg(circle, layout.bounds(), *viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_turns_once_a_turn_and_starts_again() {
        assert_eq!(turned(Duration::ZERO), 0.0);
        assert!((turned(SPIN_TURN / 4) - TAU / 4.0).abs() < 1e-4);
        assert!((turned(SPIN_TURN + SPIN_TURN / 2) - TAU / 2.0).abs() < 1e-4, "a turn later it is where it was");
    }
}
