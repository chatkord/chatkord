//! The pictures an encoder was given whose units have not come out: when each was captured, so a unit carries its
//! picture's time, and how many there are, so an encoder that takes pictures and hands nothing back is told apart.

use std::collections::VecDeque;
use std::time::Instant;

/// The most pictures an encoder is let hold with no unit out: more than any encoder here holds (AMF's default depth,
/// 16), so a unit always finds its own and an encoder holding this many has stalled.
pub(super) const IN_FLIGHT: usize = 32;

/// An encoder holds [`IN_FLIGHT`] pictures and handed no unit out for any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stalled;

/// Each picture sent whose unit has not come out, by its pts, oldest first.
#[derive(Debug)]
pub(super) struct InFlight {
    pictures: VecDeque<(i64, Instant)>,
}

impl InFlight {
    pub(super) fn new() -> Self {
        Self { pictures: VecDeque::with_capacity(IN_FLIGHT) }
    }

    /// The picture at `pts`, captured at `captured`, was sent; refused, unrecorded, when the encoder already holds
    /// [`IN_FLIGHT`] with no unit out.
    pub(super) fn sent(&mut self, pts: i64, captured: Instant) -> Result<(), Stalled> {
        if self.pictures.len() >= IN_FLIGHT {
            return Err(Stalled);
        }
        self.pictures.push_back((pts, captured));
        Ok(())
    }

    /// When the picture of the unit at `pts` was captured; pictures sent before it, whose units an encoder may drop
    /// (a filler, a flush, a replaced encoder), are let go with it. A unit no picture is kept for is stamped `now`.
    pub(super) fn captured_of(&mut self, pts: i64, now: Instant) -> Instant {
        let mut found = None;
        while let Some(&(sent, captured)) = self.pictures.front() {
            if sent > pts {
                break;
            }
            self.pictures.pop_front();
            found = (sent == pts).then_some(captured).or(found);
        }
        found.unwrap_or(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_encoder_that_holds_every_picture_it_may_and_hands_none_out_has_stalled() {
        let (mut held, start) = (InFlight::new(), Instant::now());
        let most = i64::try_from(IN_FLIGHT).unwrap();
        for pts in 0..most {
            assert_eq!(held.sent(pts, start), Ok(()), "up to the most any encoder holds");
        }
        assert_eq!(held.sent(most, start), Err(Stalled));
        held.captured_of(0, start);
        assert_eq!(held.sent(most, start), Ok(()), "one unit out and it takes pictures again");
    }

    #[test]
    fn a_unit_carries_its_pictures_capture_and_lets_go_of_those_dropped_before_it() {
        let (mut held, start) = (InFlight::new(), Instant::now());
        let at = |ms| start + Duration::from_millis(ms);
        for pts in 0..4 {
            held.sent(pts, at(pts.unsigned_abs() * 10)).unwrap();
        }
        assert_eq!(held.captured_of(2, at(99)), at(20), "units 0 and 1 were dropped");
        assert_eq!(held.captured_of(2, at(99)), at(99), "no picture is kept for a unit seen again");
        assert_eq!(held.captured_of(3, at(99)), at(30));
        for pts in 4..4 + i64::try_from(IN_FLIGHT).unwrap() {
            assert_eq!(held.sent(pts, start), Ok(()), "every one let go no longer counts as held");
        }
    }
}
