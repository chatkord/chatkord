//! When a connection of the call whose route is gone asks the server to start its ICE again: at once once the engine
//! gave the route up, or once it stayed down a while, and again while it stays off, spread so the clients of one
//! server that lost it together do not ask together. The server ends the place itself if the route never returns.

use net::{Backoff, Jitter};
use std::time::{Duration, Instant};
use webrtc::Reach;

/// How long a route may be down, the engine still trying it, before its ICE is started again.
const DOWN_FOR: Duration = Duration::from_secs(5);

/// The wait after the `tries`th try of what stays refused or off, a restart here or a take on the lane: 1, 2 and 4
/// seconds, then 8.
pub(super) const BACKOFF: Backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(8));

/// One connection's restarts while its route is off.
#[derive(Debug)]
pub(super) struct Mending {
    jitter: Jitter,
    /// Since when the route has been off; none while it is up.
    since: Option<Instant>,
    /// Restarts asked since it went off.
    tries: u32,
    /// The earliest the next may be asked.
    next: Option<Instant>,
}

impl Mending {
    pub(super) fn new(jitter: Jitter) -> Self {
        Self { jitter, since: None, tries: 0, next: None }
    }

    /// Whether to ask for a restart at `now`, the route being `reach`; a yes is counted as asked. A connection
    /// not up yet has no route to mend.
    pub(super) fn due(&mut self, reach: Reach, now: Instant) -> bool {
        if matches!(reach, Reach::Up | Reach::Starting) {
            *self = Self::new(self.jitter);
            return false;
        }
        let since = *self.since.get_or_insert(now);
        let ready = reach == Reach::Lost || now.saturating_duration_since(since) >= DOWN_FOR;
        if !ready || self.next.is_some_and(|next| now < next) {
            return false;
        }
        self.tries = self.tries.saturating_add(1);
        self.next = Some(now + BACKOFF.delay(self.tries, self.jitter));
        true
    }

    /// Whether a restart was asked and the route is not back yet.
    pub(super) fn mending(&self) -> bool {
        self.tries > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No spread: `from_bits` maps the middle of its range to 1.0.
    fn even() -> Jitter {
        Jitter::from_bits(1 << 63)
    }

    fn at(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    #[test]
    fn a_route_down_is_restarted_once_it_stays_down_and_one_given_up_at_once() {
        let start = Instant::now();
        let mut down = Mending::new(even());
        assert!(!down.due(Reach::Down, start));
        assert!(!down.due(Reach::Down, at(start, 4.9)), "the engine is still trying it");
        assert!(down.due(Reach::Down, at(start, 5.0)));
        assert!(down.mending());
        let mut lost = Mending::new(even());
        assert!(lost.due(Reach::Lost, start), "a route the engine gave up is restarted at once");
    }

    #[test]
    fn a_route_that_stays_off_is_restarted_after_one_two_four_then_every_eight_seconds() {
        let start = Instant::now();
        let mut mending = Mending::new(even());
        let mut asked = Vec::new();
        for tenth in 0..=400 {
            let now = at(start, f64::from(tenth) / 10.0);
            if mending.due(Reach::Lost, now) {
                asked.push(now.duration_since(start).as_secs_f64().round());
            }
        }
        assert_eq!(asked, [0.0, 1.0, 3.0, 7.0, 15.0, 23.0, 31.0, 39.0]);
    }

    #[test]
    fn a_connection_not_up_yet_is_never_restarted() {
        let start = Instant::now();
        let mut starting = Mending::new(even());
        for second in [0.0, 5.0, 60.0] {
            assert!(!starting.due(Reach::Starting, at(start, second)), "at {second} s");
        }
        assert!(!starting.mending());
    }

    #[test]
    fn a_route_back_up_ends_the_mending_and_a_new_loss_starts_over() {
        let start = Instant::now();
        let mut mending = Mending::new(even());
        assert!(mending.due(Reach::Lost, start));
        assert!(!mending.due(Reach::Up, at(start, 0.5)));
        assert!(!mending.mending());
        assert!(mending.due(Reach::Lost, at(start, 0.6)), "a new loss waits for nothing that came before");
    }
}
