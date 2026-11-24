//! What a share does about its load when two things can be too much for it: its encoder's time ([`KeepUp`]) and the
//! bits its pictures are made of ([`Spend`]). Either overused asks for less; more is asked for back only when both
//! have room, for as long as each waits.

use crate::Load;
use crate::keep_up::{KeepUp, Spend};
use std::time::Duration;

/// Which limit a share's load was over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The encoder takes longer to make a picture than it has.
    Encoder,
    /// The pictures are made of more bits than the rate gives them.
    Rate,
}

/// A step a share is asked to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// Down, for the limit that was overused.
    Over(Limit),
    /// Up: both had room for as long as each waits.
    Under,
}

impl Judged {
    /// The way the step goes.
    pub fn load(self) -> Load {
        match self {
            Judged::Over(_) => Load::Over,
            Judged::Under => Load::Under,
        }
    }
}

/// The two judges of a share's load, and what each last read.
#[derive(Clone, Debug)]
pub struct Governor {
    time: KeepUp,
    bits: Spend,
    time_reading: Option<Load>,
    bits_reading: Option<Load>,
}

impl Governor {
    /// A share that began at `at`.
    pub fn new(at: Duration) -> Self {
        Self { time: KeepUp::new(at), bits: Spend::new(at), time_reading: None, bits_reading: None }
    }

    /// Records that a picture given `at` took `took` to encode of its `interval`; the step to take, if any.
    pub fn encoded(&mut self, at: Duration, took: Duration, interval: Duration) -> Option<Judged> {
        self.time_reading = self.time.read(at, took, interval);
        self.decide(at)
    }

    /// Records that the encoder was busy `took` at `at` with no picture of its own (a repeat).
    pub fn repeated(&mut self, at: Duration, took: Duration) {
        self.time.busy(at, took);
    }

    /// Records that a picture, not a key frame (which the encoder holds to its own budget), left the encoder at `at`
    /// made of `bits` at the rate `bits_per_second`; the step to take, if any.
    pub fn made(&mut self, at: Duration, bits: u64, bits_per_second: u64) -> Option<Judged> {
        self.bits_reading = self.bits.read(at, bits, bits_per_second);
        self.decide(at)
    }

    /// Records that the share's budget held back a picture `at` that asked for its `interval`'s share of
    /// `bits_per_second`: the scene asks for more than the rate, read at the next picture made.
    pub fn held(&mut self, at: Duration, bits_per_second: u64, interval: Duration) {
        self.bits.held(at, bits_per_second, interval);
    }

    fn decide(&mut self, at: Duration) -> Option<Judged> {
        let judged = match (self.time_reading, self.bits_reading) {
            (Some(Load::Over), _) => Judged::Over(Limit::Encoder),
            (_, Some(Load::Over)) => Judged::Over(Limit::Rate),
            (Some(Load::Under), Some(Load::Under)) => Judged::Under,
            _ => return None,
        };
        self.time.stepped(at, judged.load());
        self.bits.stepped(at, judged.load());
        (self.time_reading, self.bits_reading) = (None, None);
        Some(judged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keep_up::{BITS_OVERUSED_PERCENT, BITS_UNDERUSED_PERCENT, FIRST_WAIT_UP, SETTLE};

    const SIXTY: Duration = Duration::from_micros(16_667);
    const RATE: u64 = 2_400_000;

    /// The bits a picture at 60 a second has at `RATE`.
    fn had() -> f64 {
        RATE as f64 * SIXTY.as_secs_f64()
    }

    /// Feeds `seconds` of pictures from `from`, each encoded in `took` and made of `percent` of its bits, returning
    /// each step taken and when.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a share of a count of bits")]
    fn feed(
        governor: &mut Governor,
        from: Duration,
        seconds: u32,
        took: Duration,
        percent: f64,
    ) -> Vec<(Duration, Judged)> {
        let bits = (had() * percent / 100.0) as u64;
        let mut steps = Vec::new();
        for n in 0..seconds * 60 {
            let at = from + SIXTY * n;
            steps.extend(governor.encoded(at, took, SIXTY).map(|judged| (at, judged)));
            steps.extend(governor.made(at, bits, RATE).map(|judged| (at, judged)));
        }
        steps
    }

    #[test]
    fn a_picture_that_fits_its_time_and_its_bits_is_left_alone() {
        let mut governor = Governor::new(Duration::ZERO);
        assert!(feed(&mut governor, Duration::ZERO, 30, Duration::from_millis(8), 80.0).is_empty());
    }

    #[test]
    fn a_scene_that_makes_more_bits_than_its_rate_gives_asks_for_less_once_it_has_settled_and_says_it_is_the_rate() {
        let mut governor = Governor::new(Duration::ZERO);
        let over = BITS_OVERUSED_PERCENT + 20.0;
        let steps = feed(&mut governor, Duration::ZERO, 4, Duration::from_millis(5), over);
        assert_eq!(steps.first().map(|(_, judged)| *judged), Some(Judged::Over(Limit::Rate)));
        assert!(steps[0].0 >= SETTLE, "not before the share has settled, {steps:?}");
    }

    #[test]
    fn bits_just_over_the_rate_are_the_controllers_own_error_and_no_overuse() {
        let mut governor = Governor::new(Duration::ZERO);
        let within = BITS_OVERUSED_PERCENT - 5.0;
        assert!(feed(&mut governor, Duration::ZERO, 30, Duration::from_millis(5), within).is_empty());
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a count of bits")]
    fn a_screen_that_changes_seldom_but_much_has_the_time_between_its_pictures() {
        // Ten pictures a second on a step of 60, each three times a sixtieth's bits: half the rate over the second.
        let mut governor = Governor::new(Duration::ZERO);
        let bits = (had() * 3.0) as u64;
        let mut steps = Vec::new();
        for n in 0..30 * 60 {
            let at = SIXTY * n;
            steps.extend(governor.encoded(at, Duration::from_millis(5), SIXTY).map(|judged| (at, judged)));
            if n % 6 == 0 {
                steps.extend(governor.made(at, bits, RATE).map(|judged| (at, judged)));
            }
        }
        assert!(steps.iter().all(|(_, judged)| *judged != Judged::Over(Limit::Rate)), "{steps:?}");
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a count of bits")]
    fn pictures_the_budget_holds_back_are_bits_the_scene_asks_for_and_step_it_down() {
        // The budget sends four in five pictures at the rate and holds the fifth back: a fifth more than the rate.
        let mut governor = Governor::new(Duration::ZERO);
        let bits = (had() * 5.0 / 4.0) as u64;
        let mut steps = Vec::new();
        for n in 0..4 * 60 {
            let at = SIXTY * n;
            if n % 5 == 4 {
                governor.held(at, RATE, SIXTY);
            } else {
                steps.extend(governor.made(at, bits, RATE).map(|judged| (at, judged)));
            }
        }
        assert_eq!(steps.first().map(|(_, judged)| *judged), Some(Judged::Over(Limit::Rate)), "{steps:?}");
    }

    #[test]
    fn an_encoder_too_slow_for_its_interval_asks_for_less_whatever_its_bits_say() {
        let mut governor = Governor::new(Duration::ZERO);
        let steps = feed(&mut governor, Duration::ZERO, 4, Duration::from_millis(25), 60.0);
        assert_eq!(steps.first().map(|(_, judged)| *judged), Some(Judged::Over(Limit::Encoder)));
    }

    #[test]
    fn room_in_the_time_alone_does_not_ask_for_more_and_room_in_both_does() {
        let (quick, room) = (Duration::from_millis(3), BITS_UNDERUSED_PERCENT - 10.0);
        let mut governor = Governor::new(Duration::ZERO);
        let steps = feed(&mut governor, Duration::ZERO, 30, quick, BITS_UNDERUSED_PERCENT + 10.0);
        assert!(steps.is_empty(), "the encoder is idle but the pictures use half their bits, {steps:?}");
        let mut governor = Governor::new(Duration::ZERO);
        let steps = feed(&mut governor, Duration::ZERO, 30, quick, room);
        assert_eq!(steps.first().map(|(_, judged)| *judged), Some(Judged::Under));
        assert!(steps[0].0 >= SETTLE + FIRST_WAIT_UP, "after the wait, {steps:?}");
    }

    #[test]
    fn a_step_up_the_bits_undo_waits_longer_the_next_time() {
        let (quick, room) = (Duration::from_millis(3), BITS_UNDERUSED_PERCENT - 10.0);
        let mut governor = Governor::new(Duration::ZERO);
        let up = feed(&mut governor, Duration::ZERO, 20, quick, room)[0].0;
        let down = feed(&mut governor, Duration::from_secs(20), 4, quick, 200.0)[0].0;
        assert!(down > up);
        let again = feed(&mut governor, Duration::from_secs(24), 60, quick, room);
        assert!(again[0].0 - down >= SETTLE + FIRST_WAIT_UP * 2, "twice the wait, {:?}", again[0].0 - down);
    }
}
