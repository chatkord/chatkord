//! How good a call's connection is, from what the engine counted. One rule here; what the three words are
//! drawn in comes from `design`'s status colours.

use std::collections::VecDeque;
use std::time::Duration;
use words::Phrase;

// Other apps' published limits, not measured on our calls: Zoom's targets bound Good,
// and Discord warns at 250 ms and over 10 % lost, where Poor starts.
const GOOD_BELOW: Duration = Duration::from_millis(150);
const GOOD_LOSS_PERCENT: u64 = 2;
const FAIR_BELOW: Duration = Duration::from_millis(250);
const FAIR_LOSS_PERCENT: u64 = 10;

/// How many looks (a second apart) loss is counted over: at about 50 audio packets a second one dropped packet is
/// 2 % of a single look, and the colour would flicker with it.
pub(crate) const LOOKS_COUNTED: usize = 5;

/// What the engine counted on the audio a call received: packets that arrived and packets that did not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Audio packets that arrived.
    pub received: u64,
    /// Audio packets expected that never arrived.
    pub lost: u64,
}

impl Counts {
    /// What was counted after `earlier`; a counter that went back (a stream closed) is not a negative.
    pub(crate) fn since(self, earlier: Counts) -> Counts {
        Counts {
            received: self.received.saturating_sub(earlier.received),
            lost: self.lost.saturating_sub(earlier.lost),
        }
    }

    /// Whether no more than `percent` of the packets expected were lost.
    fn within(self, percent: u64) -> bool {
        let total = self.received.saturating_add(self.lost);
        self.lost.saturating_mul(100) <= percent.saturating_mul(total)
    }
}

/// The last few looks at the engine's running counters, so loss is judged over [`LOOKS_COUNTED`] seconds.
#[derive(Clone, Debug, Default)]
pub(crate) struct Looks {
    looks: VecDeque<Counts>,
}

impl Looks {
    /// Takes the counters as they stand now and returns what was counted over the window. A counter that went
    /// back (a stream closed, so its share left the sums) starts the window again.
    pub(crate) fn look(&mut self, now: Counts) -> Counts {
        if self.looks.back().is_some_and(|last| now.received < last.received || now.lost < last.lost) {
            self.looks.clear();
        }
        self.looks.push_back(now);
        while self.looks.len() > LOOKS_COUNTED + 1 {
            self.looks.pop_front();
        }
        now.since(self.looks.front().copied().unwrap_or(now))
    }
}

/// The verdict on a call's connection, drawn in a status colour with its word beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// A short round trip and little loss, within the bounds a good call keeps.
    Good,
    /// Short of good, and not yet where others warn.
    Fair,
    /// A round trip or a loss where others warn.
    Poor,
}

impl Quality {
    /// The status colour the verdict is drawn in.
    pub const fn status(self) -> design::Status {
        match self {
            Self::Good => design::Status::Success,
            Self::Fair => design::Status::Warning,
            Self::Poor => design::Status::Danger,
        }
    }

    /// The word written beside it, so colour is never the only sign.
    pub const fn word(self) -> Phrase {
        match self {
            Self::Good => Phrase::QualityGood,
            Self::Fair => Phrase::QualityFair,
            Self::Poor => Phrase::QualityPoor,
        }
    }

    /// The verdict on a round trip and the counts over the window, each grade needing both its limits; none before a
    /// round trip was measured, and with nothing counted (muted and alone) the round trip alone.
    pub fn of(round_trip: Option<Duration>, counts: Counts) -> Option<Self> {
        let round_trip = round_trip?;
        Some(if round_trip < GOOD_BELOW && counts.within(GOOD_LOSS_PERCENT) {
            Self::Good
        } else if round_trip < FAIR_BELOW && counts.within(FAIR_LOSS_PERCENT) {
            Self::Fair
        } else {
            Self::Poor
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Option<Duration> {
        Some(Duration::from_millis(millis))
    }

    fn counts(received: u64, lost: u64) -> Counts {
        Counts { received, lost }
    }

    #[test]
    fn no_round_trip_is_no_verdict() {
        assert_eq!(Quality::of(None, counts(100, 0)), None);
    }

    #[test]
    fn a_round_trip_under_a_hundred_and_fifty_is_good_and_the_limit_is_exclusive() {
        assert_eq!(Quality::of(ms(0), counts(0, 0)), Some(Quality::Good));
        assert_eq!(Quality::of(ms(149), counts(500, 0)), Some(Quality::Good));
        assert_eq!(Quality::of(ms(150), counts(500, 0)), Some(Quality::Fair));
    }

    #[test]
    fn up_to_two_percent_lost_is_good_and_up_to_ten_is_fair() {
        assert_eq!(Quality::of(ms(20), counts(98, 2)), Some(Quality::Good));
        assert_eq!(Quality::of(ms(20), counts(97, 3)), Some(Quality::Fair));
        assert_eq!(Quality::of(ms(20), counts(90, 10)), Some(Quality::Fair));
        assert_eq!(Quality::of(ms(20), counts(89, 11)), Some(Quality::Poor));
    }

    #[test]
    fn a_round_trip_of_two_hundred_and_fifty_is_poor_whatever_arrives() {
        assert_eq!(Quality::of(ms(249), counts(500, 0)), Some(Quality::Fair));
        assert_eq!(Quality::of(ms(250), counts(500, 0)), Some(Quality::Poor));
    }

    #[test]
    fn a_short_round_trip_does_not_excuse_heavy_loss() {
        assert_eq!(Quality::of(ms(20), counts(50, 50)), Some(Quality::Poor));
        assert_eq!(Quality::of(ms(200), counts(50, 50)), Some(Quality::Poor));
    }

    #[test]
    fn each_grade_has_its_own_colour_and_its_own_word() {
        let all = [Quality::Good, Quality::Fair, Quality::Poor];
        for (at, one) in all.into_iter().enumerate() {
            assert!(all[..at].iter().all(|other| other.status() != one.status() && other.word() != one.word()));
        }
        assert_eq!(Quality::Good.status(), design::Status::Success);
        assert_eq!(Quality::Poor.status(), design::Status::Danger);
    }

    #[test]
    fn counts_are_taken_since_an_earlier_look_and_a_restart_is_not_a_negative() {
        assert_eq!(counts(120, 7).since(counts(100, 7)), counts(20, 0));
        assert_eq!(counts(10, 0).since(counts(100, 7)), counts(0, 0));
    }

    /// Fifty packets a second; `burst` of the last second's were lost.
    fn second(looks: &mut Looks, total: &mut Counts, burst: u64) -> Counts {
        total.received += 50 - burst;
        total.lost += burst;
        looks.look(*total)
    }

    #[test]
    fn a_burst_of_loss_in_one_second_does_not_leave_good_over_the_window() {
        let (mut looks, mut total) = (Looks::default(), Counts::default());
        for _ in 0..4 {
            second(&mut looks, &mut total, 0);
        }
        let over = second(&mut looks, &mut total, 3);
        assert_eq!(over, counts(197, 3));
        assert_eq!(Quality::of(ms(20), over), Some(Quality::Good));
        assert_eq!(
            Quality::of(ms(20), counts(47, 3)),
            Some(Quality::Fair),
            "the burst alone, as one look would judge it"
        );
    }

    #[test]
    fn loss_leaves_the_window_after_five_looks() {
        let (mut looks, mut total) = (Looks::default(), Counts::default());
        second(&mut looks, &mut total, 0);
        assert_eq!(second(&mut looks, &mut total, 20).lost, 20);
        for _ in 0..4 {
            assert_eq!(second(&mut looks, &mut total, 0).lost, 20, "still inside the window");
        }
        assert_eq!(second(&mut looks, &mut total, 0).lost, 0, "the fifth look after it");
    }

    #[test]
    fn the_first_look_counts_nothing_and_a_counter_that_goes_back_starts_the_window_again() {
        let mut looks = Looks::default();
        assert_eq!(
            looks.look(counts(900, 40)),
            counts(0, 0),
            "what the call counted before the first look is not the window's"
        );
        assert_eq!(looks.look(counts(950, 40)), counts(50, 0));
        assert_eq!(looks.look(counts(400, 10)), counts(0, 0), "a stream closed and its share left the sums");
        assert_eq!(looks.look(counts(450, 10)), counts(50, 0));
    }
}
