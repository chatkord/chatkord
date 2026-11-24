//! Which captured pictures a share encodes, so it goes out at the frame rate chosen for it. Pictures are left out
//! before the encoder, never after: every H.264 picture but a key frame is a reference, so an encoded one dropped
//! would tear the picture until the next key frame.

use std::time::{Duration, Instant};

/// A picture this much of an interval early still counts as on time, so a capture that runs a little fast is not
/// thinned.
const EARLY_PERMILLE: u32 = 150;

/// The pictures to take at up to `frames_per_second`: each is due an interval after the last was due, so a capture
/// that does not divide the rate (24 from 60) still averages it.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    interval: Duration,
    early: Duration,
    due: Option<Instant>,
}

impl Pace {
    /// A zero rate is one a second.
    pub fn new(frames_per_second: u32) -> Self {
        let interval = Duration::from_secs(1) / frames_per_second.max(1);
        Self { interval, early: interval * EARLY_PERMILLE / 1000, due: None }
    }

    /// Whether the picture captured `at` is taken. The first is; a later one once its time is due. One taken early
    /// counts from when it came, and one long late restarts the count, so neither runs the schedule away.
    pub fn take(&mut self, at: Instant) -> bool {
        let Some(due) = self.due else {
            self.due = Some(at + self.interval);
            return true;
        };
        if at + self.early < due {
            return false;
        }
        let from = if at < due || at > due + self.interval { at } else { due };
        self.due = Some(from + self.interval);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(pace: &mut Pace, every: Duration, count: u32) -> u32 {
        let start = Instant::now();
        u32::try_from((0..count).filter(|n| pace.take(start + every * *n)).count()).expect("at most count")
    }

    #[test]
    fn a_sixty_a_second_capture_is_taken_at_the_rate_chosen() {
        let sixty = Duration::from_micros(16_667);
        assert_eq!(taken(&mut Pace::new(60), sixty, 600), 600, "the capture's own rate is all of it");
        assert_eq!(taken(&mut Pace::new(30), sixty, 600), 300);
        assert_eq!(taken(&mut Pace::new(15), sixty, 600), 150);
        assert_eq!(taken(&mut Pace::new(24), sixty, 600), 240, "24 from 60 is two and three by turns, not every third");
    }

    #[test]
    fn a_capture_a_little_fast_is_not_halved() {
        let fast = Duration::from_micros(16_000);
        assert_eq!(taken(&mut Pace::new(60), fast, 600), 600);
    }

    #[test]
    fn the_first_picture_is_taken_and_a_pause_restarts_the_count() {
        let mut pace = Pace::new(30);
        let at = Instant::now();
        assert!(pace.take(at));
        assert!(!pace.take(at + Duration::from_millis(5)));
        let back = at + Duration::from_secs(2);
        assert!(pace.take(back), "a still screen's next picture is taken");
        assert!(!pace.take(back + Duration::from_millis(5)), "and the count runs from it, not from before the pause");
    }
}
