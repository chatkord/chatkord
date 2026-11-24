//! Which of the window's events are the person's own input, and how often the scene is told of one.

use iced_winit::core::time::Instant;
use iced_winit::winit::event::WindowEvent;
use std::time::Duration;

/// The least time between two tellings of input: a pointer moves many times a second, and the scene needs no more
/// than one.
const TELL_EVERY: Duration = Duration::from_secs(1);

/// Whether `event` is the person using the window: a key, a click, a wheel turn, a pointer move or a touch.
pub(crate) fn is_input(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::KeyboardInput { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::Touch(_)
    )
}

/// When input was last told to the scene.
#[derive(Default)]
pub(crate) struct Throttle {
    told: Option<Instant>,
}

impl Throttle {
    /// Whether input at `now` is to be told: the first, and then one in each [`TELL_EVERY`].
    pub(crate) fn tell(&mut self, now: Instant) -> bool {
        let due = self.told.is_none_or(|told| now.saturating_duration_since(told) >= TELL_EVERY);
        if due {
            self.told = Some(now);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_told_at_most_once_a_second() {
        let start = Instant::now();
        let mut throttle = Throttle::default();
        assert!(throttle.tell(start), "the first");
        assert!(!throttle.tell(start + Duration::from_millis(999)));
        assert!(throttle.tell(start + TELL_EVERY), "a second after the last told");
        assert!(!throttle.tell(start + TELL_EVERY + Duration::from_millis(1)));
    }
}
