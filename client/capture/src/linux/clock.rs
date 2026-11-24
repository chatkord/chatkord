//! A picture's time on this process's clock, spaced as the compositor stamped its buffers rather than as they
//! arrived, so the compositor's delivery jitter stays out of the stream's clock.

use std::time::{Duration, Instant};

use pw::Time;

/// How far a picture's time may lie before its arrival before the compositor's clock is taken to have parted from
/// ours, and its pictures are placed afresh from where they arrive. KWin 6.8 at 60 Hz delivered within 0.5 ms of its
/// soonest; this leaves a stall of six pictures its true time.
const LATEST: Duration = Duration::from_millis(100);

/// Places pictures by the compositor's times from an anchor: a picture whose arrival and time are both known. Nothing
/// is assumed of which clock the compositor stamps with (PipeWire names none) beyond that it runs at our clock's rate.
#[derive(Debug, Default)]
pub(super) struct Clock {
    /// The compositor's time of the anchor picture, when it arrived, and the last picture's time.
    anchor: Option<Anchor>,
}

#[derive(Debug, Clone, Copy)]
struct Anchor {
    time: Duration,
    arrived: Instant,
    last: Duration,
}

impl Clock {
    /// When the picture that arrived at `arrived`, stamped `time` by the compositor, was made. A picture arrives no
    /// sooner than it is made, so the anchor follows the picture that came the soonest after it was made; a picture
    /// with no time, out of order or after a break in the compositor's clock is placed where it arrived.
    pub(super) fn place(&mut self, time: Option<Time>, arrived: Instant) -> Instant {
        let Some(time) = time else {
            self.anchor = None;
            return arrived;
        };
        let placed = self
            .anchor
            .filter(|anchor| !time.discontinuous && time.at > anchor.last)
            .and_then(|anchor| anchor.arrived.checked_add(time.at - anchor.time))
            .filter(|placed| *placed <= arrived && arrived.duration_since(*placed) <= LATEST);
        match (placed, &mut self.anchor) {
            (Some(placed), Some(anchor)) => {
                anchor.last = time.at;
                placed
            }
            _ => {
                self.anchor = Some(Anchor { time: time.at, arrived, last: time.at });
                arrived
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_micros(16_667);

    fn at(nth: u32) -> Option<Time> {
        Some(Time { at: Duration::from_secs(1000) + FRAME * nth, discontinuous: false })
    }

    #[test]
    fn pictures_are_spaced_by_the_compositor_s_times_not_by_their_arrival() {
        let start = Instant::now();
        let mut clock = Clock::default();
        let delays = [2, 9, 3, 14, 2, 7].map(Duration::from_millis);
        let placed: Vec<Instant> =
            (0..6u32).map(|nth| clock.place(at(nth), start + FRAME * nth + delays[nth as usize])).collect();
        for (nth, pair) in placed.windows(2).enumerate() {
            assert_eq!(pair[1] - pair[0], FRAME, "picture {} keeps the compositor's spacing", nth + 1);
        }
        assert!(placed.iter().zip(0u32..).all(|(placed, nth)| *placed <= start + FRAME * nth + delays[nth as usize]));
    }

    #[test]
    fn the_anchor_moves_to_the_picture_that_came_soonest() {
        let start = Instant::now();
        let mut clock = Clock::default();
        assert_eq!(clock.place(at(0), start + Duration::from_millis(9)), start + Duration::from_millis(9));
        let soon = start + FRAME + Duration::from_millis(1);
        assert_eq!(clock.place(at(1), soon), soon, "placed later than it came, it anchors instead");
        let late = start + FRAME * 2 + Duration::from_millis(12);
        assert_eq!(clock.place(at(2), late), soon + FRAME);
    }

    #[test]
    fn a_picture_with_no_time_out_of_order_or_after_a_break_is_placed_as_it_came() {
        let start = Instant::now();
        let mut clock = Clock::default();
        clock.place(at(0), start);
        let came = start + FRAME * 3;
        assert_eq!(clock.place(None, came), came, "no time");
        clock.place(at(1), start + FRAME);
        assert_eq!(clock.place(at(1), came), came, "the same time again");
        let broke = Some(Time { discontinuous: true, ..at(2).unwrap() });
        assert_eq!(clock.place(broke, came), came, "a break in the compositor's clock");
    }

    #[test]
    fn a_clock_that_parted_from_ours_is_placed_afresh() {
        let start = Instant::now();
        let mut clock = Clock::default();
        clock.place(at(0), start);
        let came = start + FRAME + LATEST + Duration::from_millis(1);
        assert_eq!(clock.place(at(1), came), came);
        assert_eq!(clock.place(at(2), came + FRAME), came + FRAME, "and keeps the new anchor");
    }
}
