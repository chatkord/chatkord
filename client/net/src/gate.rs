//! Which rate targets are worth a new encoder: a rate change reopens it on a key frame, so a small change costs
//! more picture than it gains. Swept over a real engine's targets on loopback, three traces of 7, 7 and 89 targets
//! made 2, 2 and 6 key frames (10 % and 1 s made 3, 3 and 11; 50 % made 2, 2 and 2 but missed a 40 % drift).

use std::time::Duration;

/// A target is applied only when it differs from the running rate by at least this
/// many thousandths of it.
pub(crate) const MIN_CHANGE_PERMILLE: u64 = 200;

/// An increase waits at least this long after the last change. A decrease does not: a
/// lower target means the path is congested, and waiting makes it worse.
pub(crate) const MIN_INTERVAL_FOR_INCREASE: Duration = Duration::from_secs(2);

/// What to do with a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Open the encoder at the target.
    Apply,
    /// Keep the running rate.
    Keep,
}

/// Decides, target by target, whether the running encoder changes rate; a rate is its bits a second, the only part of
/// a target it weighs.
#[derive(Clone, Copy, Debug)]
pub struct RateGate {
    /// The bits a second the encoder runs at and when it began, as an offset on the caller's clock.
    running: (u32, Duration),
    /// Until when no target is taken either way: one made while the encoder sent nothing measured no link.
    unmeasured_until: Duration,
}

impl RateGate {
    /// A gate for an encoder that began at `running` bits a second at offset `now`.
    pub fn new(running: u32, now: Duration) -> Self {
        Self { running: (running, now), unmeasured_until: Duration::ZERO }
    }

    /// A gate for an encoder at `running` that sends again at offset `now` after a rest, when nobody watched: the
    /// engine's estimate fell while nothing was sent, as it does with no feedback, so it keeps the rate the link
    /// carried until it has sent for `MIN_INTERVAL_FOR_INCREASE`.
    pub fn woken(running: u32, now: Duration) -> Self {
        Self { running: (running, now), unmeasured_until: now + MIN_INTERVAL_FOR_INCREASE }
    }

    /// `now` is any monotonic offset, the same clock on every call. A target the gate keeps is
    /// not remembered: the engine states the next one soon, and a slow drift is compared with
    /// the running rate each time, so it is applied once it adds up.
    pub fn offer(&mut self, now: Duration, target: u32) -> Verdict {
        if now < self.unmeasured_until {
            return Verdict::Keep;
        }
        let (running, since) = self.running;
        let difference = u64::from(target.abs_diff(running));
        let enough = difference * 1000 >= u64::from(running) * MIN_CHANGE_PERMILLE;
        let increase = target > running;
        let waited = now.saturating_sub(since) >= MIN_INTERVAL_FOR_INCREASE;
        if enough && (!increase || waited) {
            self.running = (target, now);
            Verdict::Apply
        } else {
            Verdict::Keep
        }
    }
}
