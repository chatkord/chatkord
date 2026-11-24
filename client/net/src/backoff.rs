//! How long to wait before a try that was refused again: doubling from a first wait to a longest, and spread, so the
//! clients of one server that lost it together do not try together. One schedule shape; each retrying owner names its
//! own two numbers.

use std::time::Duration;

/// A factor in [0.8, 1.2] that spreads the clients of one server so they do not all try at once.
/// The host draws the bits; this crate owns the range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jitter(f64);

impl Jitter {
    /// No spread: the middle of the range.
    pub const NONE: Jitter = Jitter(1.0);

    /// The factor for 64 random bits.
    pub fn from_bits(bits: u64) -> Jitter {
        // The top 53 bits are exactly a fraction in [0, 1).
        let unit = (bits >> 11) as f64 / (1_u64 << 53) as f64;
        Jitter(0.8 + 0.4 * unit)
    }

    /// `wait` spread by this factor.
    pub fn scale(self, wait: Duration) -> Duration {
        wait.mul_f64(self.0)
    }
}

/// A wait that doubles at each try, from `first` and no further than `longest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    first: Duration,
    longest: Duration,
}

impl Backoff {
    /// A schedule that waits `first` after the first try, twice that after the second, and so on up to `longest`.
    pub const fn new(first: Duration, longest: Duration) -> Backoff {
        Backoff { first, longest }
    }

    /// The wait after the `tries`th try, the first being 1: before any spread.
    pub fn wait(self, tries: u32) -> Duration {
        let doublings = tries.saturating_sub(1).min(u32::BITS - 1);
        self.first.saturating_mul(1 << doublings).min(self.longest)
    }

    /// The wait after the `tries`th try, spread by `jitter`.
    pub fn delay(self, tries: u32, jitter: Jitter) -> Duration {
        jitter.scale(self.wait(tries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn the_wait_doubles_from_the_first_and_stops_at_the_longest() {
        let reconnect = Backoff::new(SECOND, 30 * SECOND);
        let waits: Vec<u64> = (1..=8).map(|tries| reconnect.wait(tries).as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30]);
        let mend = Backoff::new(SECOND, 8 * SECOND);
        assert_eq!((1..=5).map(|tries| mend.wait(tries).as_secs()).collect::<Vec<_>>(), [1, 2, 4, 8, 8]);
    }

    #[test]
    fn a_count_past_every_doubling_is_the_longest_not_an_overflow() {
        let long = Backoff::new(SECOND, 64 * SECOND);
        assert_eq!(long.wait(0), SECOND);
        assert_eq!(long.wait(u32::MAX), 64 * SECOND);
    }

    #[test]
    fn jitter_stays_within_a_fifth_either_way() {
        let wait = Backoff::new(SECOND, 30 * SECOND);
        assert_eq!(wait.delay(6, Jitter::from_bits(0)), Duration::from_secs(24));
        let most = wait.delay(6, Jitter::from_bits(u64::MAX));
        assert!(most <= Duration::from_secs(36) && most > Duration::from_millis(35_990), "{most:?}");
        assert_eq!(Jitter::from_bits(1 << 63), Jitter::NONE);
    }
}
