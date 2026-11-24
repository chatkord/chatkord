//! A share's own rate, kept before its encoder. A hardware encoder at the coarsest quantiser it has makes more bits
//! than a rate under what a busy picture needs asks of it (AMF at 1080p60 asked 2.4 Mbit/s made 5.5), and libwebrtc's
//! frame dropper, which would hold the surplus back, is off for an encoder that says it keeps its own rate: the
//! surplus waits in the pacer, and the picture falls behind its sound by more each second. So the share keeps the
//! account a frame dropper keeps: a leaky bucket that fills with the bits sent and empties at the rate the engine
//! asked, and a picture that would be made while it holds more than the encoder's own key frame budget is left to
//! wait, as the newest of those not yet sent.

use std::time::Duration;

/// A leaky bucket of the bits sent beyond the rate.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// How much of the rate may be owed before a picture waits.
    allowance: Duration,
    /// The rate the bucket empties at, in bits a second.
    bits_per_second: u64,
    /// The bits sent beyond the rate as of `at`.
    owed: u128,
    /// When `owed` was last made current.
    at: Duration,
}

impl Budget {
    /// A bucket at `at` that lets `allowance` of the rate be owed: a key frame may take that much, so it is no more
    /// than the encoder itself lets one take.
    pub fn new(allowance: Duration, bits_per_second: u64, at: Duration) -> Self {
        Self { allowance, bits_per_second, owed: 0, at }
    }

    /// The engine asked the encoder for `bits_per_second`.
    pub fn set_rate(&mut self, at: Duration, bits_per_second: u64) {
        self.drain(at);
        self.bits_per_second = bits_per_second;
    }

    /// Records that `bits` were sent at `at`.
    pub fn sent(&mut self, at: Duration, bits: u64) {
        self.drain(at);
        self.owed += u128::from(bits);
    }

    /// Whether a picture made at `at` may be sent: what is owed is no more than the allowance of the rate.
    pub fn admits(&mut self, at: Duration) -> bool {
        self.drain(at);
        self.owed <= self.allowed()
    }

    /// The bits the allowance is, at the rate.
    fn allowed(&self) -> u128 {
        u128::from(self.bits_per_second) * self.allowance.as_nanos() / 1_000_000_000
    }

    fn drain(&mut self, at: Duration) {
        let emptied = u128::from(self.bits_per_second) * at.saturating_sub(self.at).as_nanos() / 1_000_000_000;
        self.owed = self.owed.saturating_sub(emptied);
        self.at = self.at.max(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWANCE: Duration = Duration::from_millis(300);
    const RATE: u64 = 2_400_000;
    const SIXTY: Duration = Duration::from_micros(16_667);

    fn bucket() -> Budget {
        Budget::new(ALLOWANCE, RATE, Duration::ZERO)
    }

    /// The bits a rate gives a span of time.
    fn bits(rate: u64, span: Duration) -> u64 {
        u64::try_from(u128::from(rate) * span.as_nanos() / 1_000_000_000).unwrap()
    }

    #[test]
    fn a_picture_that_sends_what_its_rate_gives_is_never_held() {
        let mut budget = bucket();
        for n in 0..600 {
            let at = SIXTY * n;
            assert!(budget.admits(at), "picture {n}");
            budget.sent(at, bits(RATE, SIXTY));
        }
    }

    #[test]
    fn a_key_frame_of_the_allowance_is_admitted_and_the_picture_after_it_waits_until_it_has_drained() {
        let mut budget = bucket();
        assert!(budget.admits(Duration::ZERO));
        budget.sent(Duration::ZERO, bits(RATE, ALLOWANCE) + 1);
        assert!(!budget.admits(Duration::ZERO), "a bit past the allowance");
        assert!(!budget.admits(Duration::from_nanos(100)), "a moment has not drained a bit");
        assert!(budget.admits(SIXTY), "a sixtieth of the rate has");
    }

    #[test]
    fn a_scene_the_encoder_makes_more_of_than_its_rate_is_held_to_the_rate_plus_the_allowance() {
        let mut budget = bucket();
        let seconds = 20;
        let (mut sent, mut held) = (0u64, 0u32);
        // Each picture the encoder makes is 2.3 times what the rate gives a picture, the most it can come down to.
        let picture = bits(RATE, SIXTY) * 23 / 10;
        for n in 0..seconds * 60 {
            let at = SIXTY * n;
            if budget.admits(at) {
                budget.sent(at, picture);
                sent += picture;
            } else {
                held += 1;
            }
        }
        let given = bits(RATE, SIXTY * (seconds * 60));
        assert!(sent <= given + bits(RATE, ALLOWANCE) + picture, "{sent} sent of {given} and the allowance");
        assert!(held > seconds * 60 / 3, "a share asked for more than its rate waits for most of its pictures: {held}");
    }

    #[test]
    fn the_rate_the_engine_changes_to_empties_the_bucket_from_then_and_sets_the_allowance() {
        let mut budget = bucket();
        budget.sent(Duration::ZERO, bits(RATE, ALLOWANCE) + 1);
        budget.set_rate(Duration::ZERO, RATE * 2);
        assert!(budget.admits(Duration::ZERO), "twice the rate allows twice the bits owed");
        budget.sent(Duration::ZERO, bits(RATE * 2, ALLOWANCE));
        assert!(!budget.admits(Duration::ZERO));
        assert!(budget.admits(Duration::from_secs(1)), "and empties twice as fast");
    }

    #[test]
    fn a_clock_that_does_not_go_back_is_all_that_is_asked_of_it() {
        let mut budget = bucket();
        budget.sent(Duration::from_secs(5), bits(RATE, ALLOWANCE) + 1);
        assert!(!budget.admits(Duration::from_secs(4)), "an earlier time drains nothing");
    }
}
