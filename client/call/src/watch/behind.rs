//! Whether a watched stream's decoder keeps up: the time it spent decoding its latest pictures against the time those
//! pictures span on the stream's own clock, so a burst the network delivers at once is no sign of a slow computer.

use std::collections::VecDeque;
use std::time::Duration;

/// The pictures it is judged over: a second of a stream at 60 a second.
const PICTURES: usize = 60;

/// A decoder judged behind is caught up only once it decodes in nine tenths of the stream's time, so one that runs at
/// about the stream's rate does not set and clear its card's word each second.
const CAUGHT_UP_PERMILLE: u128 = 900;

/// The latest pictures' times on the stream's clock and how long each took to decode, and the judgement so far.
#[derive(Debug, Default)]
pub(crate) struct Behind {
    recent: VecDeque<(i64, Duration)>,
    behind: bool,
    judged: bool,
}

impl Behind {
    /// Takes in a picture whose time on the stream's clock is `at_us`, decoded in `took`; whether the decoder is
    /// behind the stream now. Pictures whose times do not move on leave the judgement as it was.
    pub(crate) fn decoded(&mut self, at_us: i64, took: Duration) -> bool {
        if self.recent.len() == PICTURES {
            self.recent.pop_front();
        }
        self.recent.push_back((at_us, took));
        let (Some((first, _)), Some((last, _))) = (self.recent.front(), self.recent.back()) else { return self.behind };
        let spanned = u128::try_from(last.saturating_sub(*first)).unwrap_or(0) * 1_000;
        if self.recent.len() < PICTURES || spanned == 0 {
            return self.behind;
        }
        // The first picture's decode came before the span began.
        let busy: u128 = self.recent.iter().skip(1).map(|(_, took)| took.as_nanos()).sum();
        self.judged = true;
        self.behind = if self.behind { busy * 1_000 > spanned * CAUGHT_UP_PERMILLE } else { busy > spanned };
        self.behind
    }

    /// Whether it has judged the decoder: it has taken a second of pictures, and they moved on the stream's clock.
    pub(crate) fn judged(&self) -> bool {
        self.judged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream at 60 a second whose every picture takes `took` to decode, `pictures` of them from `from`.
    fn feed(behind: &mut Behind, from: u32, pictures: u32, took: Duration) -> bool {
        (from..from + pictures).map(|n| behind.decoded(i64::from(n) * 16_667, took)).last().unwrap_or(false)
    }

    #[test]
    fn a_decoder_slower_than_the_stream_is_behind_and_a_faster_one_is_not() {
        let mut slow = Behind::default();
        assert!(!feed(&mut slow, 0, 59, Duration::from_micros(16_900)), "not judged before a second of pictures");
        assert!(!slow.judged());
        assert!(feed(&mut slow, 59, 1, Duration::from_micros(16_900)), "16.9 ms a picture of a 16.7 ms stream");
        let mut fast = Behind::default();
        assert!(!feed(&mut fast, 0, 120, Duration::from_micros(15_000)), "15 ms a picture keeps up");
        assert!(fast.judged());
    }

    #[test]
    fn a_behind_decoder_is_caught_up_only_well_under_the_stream_s_time() {
        let mut behind = Behind::default();
        assert!(feed(&mut behind, 0, 60, Duration::from_micros(17_000)));
        assert!(feed(&mut behind, 60, 60, Duration::from_micros(16_000)), "96 % of the stream's time: still behind");
        assert!(!feed(&mut behind, 120, 60, Duration::from_micros(14_000)), "84 %: caught up");
        assert!(!feed(&mut behind, 180, 60, Duration::from_micros(16_000)), "and 96 % does not set it again");
    }

    #[test]
    fn pictures_whose_times_do_not_move_judge_nothing() {
        let mut behind = Behind::default();
        let stuck = (0..120).map(|_| behind.decoded(0, Duration::from_millis(50))).last();
        assert_eq!(stuck, Some(false), "no span to judge against");
        assert!(!behind.judged());
    }
}
