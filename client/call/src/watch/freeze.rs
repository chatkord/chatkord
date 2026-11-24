//! When a watched screen froze: a gap between decoded pictures longer than WebRTC's bound for a freeze, the larger of
//! three times the recent interval and that interval and 150 ms, in which frames came and were lost or refused. A
//! still screen, whose sender repeats its last picture slowly and has it decoded, is no freeze. Time comes in.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many intervals the recent one is the mean of.
const RECENT: usize = 30;
/// What WebRTC adds to the recent interval for its bound.
const MORE_THAN: Duration = Duration::from_millis(150);

/// The freezes of one watched screen so far.
#[derive(Debug, Default)]
pub(crate) struct Freezes {
    last: Option<Instant>,
    intervals: VecDeque<Duration>,
    count: u64,
    longest: Duration,
}

impl Freezes {
    /// Takes in a picture made `at`, and whether pictures were lost or refused since the one before; the gap before
    /// it when that gap was a freeze.
    pub(crate) fn picture(&mut self, at: Instant, broken: bool) -> Option<Duration> {
        let last = self.last.replace(at)?;
        let gap = at.saturating_duration_since(last);
        if broken && self.bound().is_some_and(|bound| gap > bound) {
            self.count += 1;
            self.longest = self.longest.max(gap);
            return Some(gap);
        }
        if self.intervals.len() == RECENT {
            self.intervals.pop_front();
        }
        self.intervals.push_back(gap);
        None
    }

    /// The stream was held and is taken again: the gap until its next picture is the hold's, which is no freeze.
    pub(crate) fn resumed(&mut self) {
        self.last = None;
    }

    /// The longest gap that is not a freeze; none before an interval is known.
    fn bound(&self) -> Option<Duration> {
        let count = u32::try_from(self.intervals.len()).ok().filter(|count| *count > 0)?;
        let mean = self.intervals.iter().sum::<Duration>() / count;
        Some((mean * 3).max(mean + MORE_THAN))
    }

    /// How many freezes, and the longest.
    pub(crate) fn so_far(&self) -> (u64, Duration) {
        (self.count, self.longest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sixtieths(freezes: &mut Freezes, from: Instant, pictures: u32) -> Instant {
        let sixtieth = Duration::from_micros(16_667);
        (1..=pictures).for_each(|n| {
            freezes.picture(from + sixtieth * n, false);
        });
        from + sixtieth * pictures
    }

    #[test]
    fn a_broken_stream_longer_than_webrtcs_bound_is_one() {
        let mut freezes = Freezes::default();
        let at = sixtieths(&mut freezes, Instant::now(), 60);
        assert_eq!(freezes.picture(at + Duration::from_millis(160), true), None);
        assert_eq!(freezes.so_far().0, 0, "under 16.7 ms and 150 ms, however broken");
        let at = sixtieths(&mut freezes, at + Duration::from_millis(160), 60);
        assert_eq!(freezes.picture(at + Duration::from_millis(900), true), Some(Duration::from_millis(900)));
        assert_eq!(freezes.so_far(), (1, Duration::from_millis(900)));
    }

    #[test]
    fn a_hold_taken_again_is_no_freeze() {
        let mut freezes = Freezes::default();
        let at = sixtieths(&mut freezes, Instant::now(), 60);
        freezes.resumed();
        freezes.picture(at + Duration::from_secs(10), true);
        assert_eq!(freezes.so_far().0, 0, "the hold's 10 s, frames dropped at its end, are not the picture's fault");
        let at = sixtieths(&mut freezes, at + Duration::from_secs(10), 60);
        freezes.picture(at + Duration::from_millis(900), true);
        assert_eq!(freezes.so_far().0, 1, "and the bound still holds after it");
    }

    #[test]
    fn a_still_screen_is_no_freeze() {
        let mut freezes = Freezes::default();
        let mut at = sixtieths(&mut freezes, Instant::now(), 60);
        for _ in 0..10 {
            at += Duration::from_millis(500);
            freezes.picture(at, false);
        }
        assert_eq!(freezes.so_far().0, 0, "slow repeats, each decoded, nothing lost");
    }
}
