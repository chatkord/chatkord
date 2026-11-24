//! A share's pipeline as one note, only for a second that went wrong: a freeze in the pictures it took (WebRTC's rule,
//! [`crate::watch::freeze::Freezes`]), an encode over its budget, or a picture lost. Pure: counts and the time come in.

use crate::screen::{Counted, Worst};
use proto::video::Video;
use report::{Fact, count16, count32, millis, word};
use std::time::{Duration, Instant};

/// The least time between two notes; a look sooner than this adds to the next second.
const AT_MOST_EVERY: Duration = Duration::from_secs(1);

/// The stages of the pipeline over the time between two notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Second {
    /// Pictures the system made.
    captured: u16,
    /// Pictures the capture's slot replaced before they were taken.
    dropped: u16,
    /// Pictures the pace or the share's own rate left out.
    paced: u16,
    /// Pictures lost before the encoder otherwise.
    discarded: u16,
    /// Pictures encoded.
    encodes: u16,
    /// The mean encode, in microseconds.
    encode_us: u32,
    /// The slowest encode, in microseconds.
    slowest_us: u32,
    /// Encodes over the budget.
    over_budget: u16,
    /// Units handed to libwebrtc.
    sent: u16,
    /// The widest gap between two pictures the system made, in milliseconds; a slow screen's is no fault.
    gap_ms: u16,
    /// The longest freeze in the pictures the share took, in milliseconds; zero for none.
    freeze_ms: u16,
    /// What a picture may take, in milliseconds.
    budget_ms: u16,
}

impl Second {
    /// What happened between the counts `before` and `now`, with the `worst` times in it.
    fn between(before: &Counted, now: &Counted, worst: Worst) -> Self {
        let over = |now: u64, before: u64| now.saturating_sub(before);
        let encodes = over(now.encodes, before.encodes);
        let encode_time = now.encode_time.saturating_sub(before.encode_time);
        let mean = u64::try_from(encode_time.as_micros()).unwrap_or(u64::MAX).checked_div(encodes).unwrap_or(0);
        Self {
            captured: count16(over(now.delivered, before.delivered)),
            dropped: count16(over(now.slot_dropped, before.slot_dropped)),
            paced: count16(over(now.paced, before.paced).saturating_add(over(now.held, before.held))),
            discarded: count16(over(now.discarded, before.discarded)),
            encodes: count16(encodes),
            encode_us: count32(mean),
            slowest_us: count32(u64::try_from(worst.slowest_encode.as_micros()).unwrap_or(u64::MAX)),
            over_budget: count16(over(now.over_budget, before.over_budget)),
            sent: count16(over(now.sent, before.sent)),
            gap_ms: count16(millis(worst.capture_gap)),
            freeze_ms: count16(millis(worst.freeze)),
            budget_ms: count16(millis(now.budget)),
        }
    }

    /// Whether the second went wrong: a freeze, an encode over its budget, or a picture lost. A screen that changed
    /// slowly, and a picture the pace or the rate left out, are the source and the rate chosen, not a fault.
    fn went_wrong(&self) -> bool {
        self.freeze_ms > 0 || self.over_budget > 0 || self.dropped > 0 || self.discarded > 0
    }
}

/// The looks at one video's pipeline, a second apart, and the counts at the one before.
#[derive(Debug)]
pub(crate) struct StagesLog {
    of: Video,
    last: Option<(Instant, Counted)>,
    worst: Worst,
}

/// The note about one second of a video's pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StagesLine {
    of: Video,
    second: Second,
}

impl StagesLog {
    /// The log of `of`, the share or the camera.
    pub(crate) fn new(of: Video) -> Self {
        Self { of, last: None, worst: Worst::default() }
    }

    /// Takes in a look at `at`, with what the stages `counted` and the `worst` times since the last look. The first
    /// look only starts the count. A second after the last note, the line about it, when it went wrong.
    pub(crate) fn look(&mut self, counted: Counted, worst: Worst, at: Instant) -> Option<StagesLine> {
        self.worst = self.worst.max(worst);
        let Some((then, before)) = self.last else {
            self.last = Some((at, counted));
            self.worst = Worst::default();
            return None;
        };
        if at.saturating_duration_since(then) < AT_MOST_EVERY {
            return None;
        }
        self.last = Some((at, counted));
        let second = Second::between(&before, &counted, std::mem::take(&mut self.worst));
        second.went_wrong().then_some(StagesLine { of: self.of, second })
    }
}

impl StagesLine {
    /// Writes the note to the log.
    pub(crate) fn log(&self) {
        let (of, second) = (
            match self.of {
                Video::Screen => Fact::Word(word!("share")),
                Video::Camera => Fact::Word(word!("camera")),
            },
            &self.second,
        );
        report::note!(
            warn,
            "video stages went wrong",
            of = of,
            captured = second.captured,
            dropped = second.dropped,
            paced = second.paced,
            discarded = second.discarded,
            encodes = second.encodes,
            encode_us = second.encode_us,
            slowest_us = second.slowest_us,
            over_budget = second.over_budget,
            sent = second.sent,
            gap_ms = second.gap_ms,
            freeze_ms = second.freeze_ms,
            budget_ms = second.budget_ms,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use report::{MAX_LINE, Ring};
    use tracing_subscriber::layer::SubscriberExt as _;

    const MS: Duration = Duration::from_millis(1);
    const BUDGET: Duration = Duration::from_millis(16);

    /// Counts that say a healthy second more than `before`: sixty pictures made, encoded and sent, none lost.
    fn steady(before: Counted) -> Counted {
        Counted {
            delivered: before.delivered + 60,
            encodes: before.encodes + 60,
            encode_time: before.encode_time + 60 * 5 * MS,
            sent: before.sent + 60,
            budget: BUDGET,
            ..before
        }
    }

    fn quiet() -> Worst {
        Worst { slowest_encode: 7 * MS, capture_gap: BUDGET, freeze: Duration::ZERO }
    }

    /// A log that has had its first look, which only starts the count.
    fn started(at: Instant) -> (StagesLog, Counted) {
        let mut log = StagesLog::new(Video::Screen);
        let counted = Counted { budget: BUDGET, ..Counted::default() };
        assert_eq!(log.look(counted, Worst::default(), at), None, "the first look only starts the count");
        (log, counted)
    }

    #[test]
    fn a_healthy_second_writes_no_note() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        assert_eq!(log.look(steady(counted), quiet(), start + Duration::from_secs(1)), None);
    }

    #[test]
    fn a_freeze_is_a_second_gone_wrong_and_a_slow_screens_gap_alone_is_not() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        let slow = Worst { capture_gap: 900 * MS, ..quiet() };
        assert_eq!(
            log.look(steady(counted), slow, start + Duration::from_secs(1)),
            None,
            "a still screen made no picture"
        );
        let frozen = Worst { capture_gap: 400 * MS, freeze: 400 * MS, ..quiet() };
        let line = log.look(steady(steady(counted)), frozen, start + Duration::from_secs(2)).expect("a freeze");
        assert_eq!((line.second.freeze_ms, line.second.gap_ms, line.second.budget_ms), (400, 400, 16));
        assert_eq!((line.second.captured, line.second.encodes, line.second.sent), (60, 60, 60));
        assert_eq!(line.second.encode_us, 5_000, "the mean over the second's encodes");
        assert_eq!(line.second.slowest_us, 7_000);
    }

    #[test]
    fn an_encode_over_its_budget_a_slot_drop_or_a_discard_each_make_a_second_wrong_and_the_pace_does_not() {
        let start = Instant::now();
        for (wrong, why) in [
            (Counted { over_budget: 1, ..Counted::default() }, "an encode over its budget"),
            (Counted { slot_dropped: 1, ..Counted::default() }, "a picture the slot dropped"),
            (Counted { discarded: 1, ..Counted::default() }, "a picture lost before the encoder"),
        ] {
            let (mut log, counted) = started(start);
            let now = Counted {
                over_budget: wrong.over_budget,
                slot_dropped: wrong.slot_dropped,
                discarded: wrong.discarded,
                ..steady(counted)
            };
            assert!(log.look(now, quiet(), start + Duration::from_secs(1)).is_some(), "{why}");
        }
        let (mut log, counted) = started(start);
        let paced = Counted { paced: 30, ..steady(counted) };
        assert_eq!(log.look(paced, quiet(), start + Duration::from_secs(1)), None, "the rate chosen is no fault");
        let (mut log, counted) = started(start);
        let held = Counted { held: 30, ..steady(counted) };
        assert_eq!(log.look(held, quiet(), start + Duration::from_secs(1)), None, "nor the budget that holds to it");
    }

    #[test]
    fn a_screen_that_made_no_picture_has_no_gap_to_say_and_no_note() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        assert_eq!(log.look(counted, Worst::default(), start + Duration::from_secs(1)), None);
    }

    #[test]
    fn counts_are_differenced_so_a_note_says_only_its_own_second() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        let first = Counted { slot_dropped: 4, ..steady(counted) };
        let line = log.look(first, quiet(), start + Duration::from_secs(1)).expect("drops");
        assert_eq!((line.second.captured, line.second.dropped), (60, 4));
        let second = Counted { slot_dropped: 5, ..steady(first) };
        let line = log.look(second, quiet(), start + Duration::from_secs(2)).expect("one more drop");
        assert_eq!((line.second.captured, line.second.dropped), (60, 1), "the drops before it are not said again");
    }

    #[test]
    fn a_look_sooner_than_a_second_after_the_last_is_counted_in_the_next() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        let frozen = Worst { capture_gap: 300 * MS, freeze: 300 * MS, ..quiet() };
        assert_eq!(log.look(steady(counted), frozen, start + Duration::from_millis(400)), None, "too soon");
        let line = log
            .look(steady(steady(counted)), quiet(), start + Duration::from_secs(1))
            .expect("the freeze seen in the look before counts");
        assert_eq!((line.second.captured, line.second.freeze_ms), (120, 300));
    }

    #[test]
    fn counts_that_fell_with_a_new_capture_read_as_nothing_and_none_is_divided_by_zero() {
        let start = Instant::now();
        let (mut log, counted) = started(start);
        let big = Counted { delivered: 1000, slot_dropped: 3, ..steady(counted) };
        log.look(big, quiet(), start + Duration::from_secs(1));
        let smaller = Counted { delivered: 10, slot_dropped: 0, encodes: big.encodes, ..big };
        assert_eq!(log.look(smaller, quiet(), start + Duration::from_secs(2)), None, "no drops, no encodes, no gap");
    }

    #[test]
    fn a_note_is_written_whole_and_fits_the_reports_line_whatever_the_counts() {
        let worst_case = Second {
            captured: u16::MAX,
            dropped: u16::MAX,
            paced: u16::MAX,
            discarded: u16::MAX,
            encodes: u16::MAX,
            encode_us: u32::MAX,
            slowest_us: u32::MAX,
            over_budget: u16::MAX,
            sent: u16::MAX,
            gap_ms: u16::MAX,
            freeze_ms: u16::MAX,
            budget_ms: u16::MAX,
        };
        let ring = Ring::new(std::time::SystemTime::now);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(ring.layer()), || {
            StagesLine { of: Video::Camera, second: worst_case }.log();
        });
        let lines = ring.lines();
        let [line] = &lines[..] else { panic!("one note: {lines:?}") };
        assert!(line.as_str().len() <= MAX_LINE, "{} bytes: {line}", line.as_str().len());
        assert!(line.as_str().contains("freeze_ms=65535"), "{line}");
    }
}
