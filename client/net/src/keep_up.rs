//! Whether a share keeps up with what it asks of itself, judged as libwebrtc's CPU adaptation judges an encoder for
//! Chrome and the apps built on it: what was spent against what there was, over the last second. Overuse that lasts
//! asks for less; headroom that lasts asks for more back, more patiently each time a step up had to be undone. One
//! judge ([`Judge`]) has two instances: the time pictures take to encode against their intervals ([`KeepUp`]), and
//! the bits they are made of against the bits their rate gives them ([`Spend`]).

use std::collections::VecDeque;
use std::time::Duration;

/// Above this share of its interval an encoder is overused; libwebrtc's own threshold.
const OVERUSED_PERCENT: f64 = 85.0;
/// Below this share it has room for more; libwebrtc's own threshold.
const UNDERUSED_PERCENT: f64 = 42.0;
/// Above this share of the rate, the bits asked for are overuse: a tenth over is a rate controller's own error, more is
/// a scene the encoder cannot code at the rate.
pub(crate) const BITS_OVERUSED_PERCENT: f64 = 110.0;
/// Below this share of the rate, the bits asked for leave room for a step up: under half, so that a step that doubles
/// the bits a second does not at once overuse.
pub(crate) const BITS_UNDERUSED_PERCENT: f64 = 45.0;
/// How far back the load is measured.
const MEASURED_OVER: Duration = Duration::from_secs(1);
/// How long a share runs, or runs after a step, before it is judged: the encoder's first pictures and a new stream's
/// key frame are slow on their own.
pub(crate) const SETTLE: Duration = Duration::from_secs(2);
/// How long headroom must last before a step back up, at first.
pub(crate) const FIRST_WAIT_UP: Duration = Duration::from_secs(10);
/// The longest a step back up waits, however often one was undone.
const LONGEST_WAIT_UP: Duration = Duration::from_secs(80);

/// What a share should do about its load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// It does not keep up: ask less of it.
    Over,
    /// It has had room for a while: it may be asked for more.
    Under,
}

/// What one judge reads of the load so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reading {
    /// Not measured yet, or between the thresholds.
    Between,
    /// Overused.
    Over,
    /// Room that has lasted long enough for a step up.
    Roomy,
}

/// What was spent against what there was, over the last second: the judging libwebrtc's CPU adaptation does. It only
/// reads; a step the share takes is told to it by [`Judge::stepped`].
#[derive(Clone, Debug)]
pub(crate) struct Judge {
    over_percent: f64,
    under_percent: f64,
    /// Each entry recorded since the last step, within [`MEASURED_OVER`] of the newest: when it was given, what it
    /// spent, what there was for it and the time it covers.
    recent: VecDeque<(Duration, f64, f64, Duration)>,
    /// When the last step was taken, or the share began.
    stepped: Duration,
    /// Since when there has been room, while there has.
    roomy_since: Option<Duration>,
    /// How long headroom must last before the next step up.
    wait_up: Duration,
    /// When the last step up was taken, until a step down has been judged against it.
    went_up: Option<Duration>,
}

impl Judge {
    fn new(at: Duration, over_percent: f64, under_percent: f64) -> Self {
        Self {
            over_percent,
            under_percent,
            recent: VecDeque::new(),
            stepped: at,
            roomy_since: None,
            wait_up: FIRST_WAIT_UP,
            went_up: None,
        }
    }

    /// Records that what was given `at` spent `spent` of the `had` it had, over `span` of time, and reads the load.
    pub(crate) fn record(&mut self, at: Duration, spent: f64, had: f64, span: Duration) -> Reading {
        if at.saturating_sub(self.stepped) < SETTLE {
            return Reading::Between;
        }
        self.recent.push_back((at, spent, had, span));
        while self.recent.front().is_some_and(|(given, ..)| at.saturating_sub(*given) >= MEASURED_OVER) {
            self.recent.pop_front();
        }
        // A second of pictures, however few came: an encoder that holds them up sends fewer.
        let covered = self.recent.front().map_or(Duration::ZERO, |(given, ..)| at.saturating_sub(*given) + span);
        if covered < MEASURED_OVER * 9 / 10 {
            return Reading::Between;
        }
        let had: f64 = self.recent.iter().map(|(_, _, had, _)| *had).sum();
        if had <= 0.0 {
            return Reading::Between;
        }
        let spent: f64 = self.recent.iter().map(|(_, spent, ..)| *spent).sum();
        let usage = spent / had * 100.0;
        if usage > self.over_percent {
            return Reading::Over;
        }
        if usage >= self.under_percent {
            self.roomy_since = None;
            return Reading::Between;
        }
        let since = *self.roomy_since.get_or_insert(at);
        if at.saturating_sub(since) < self.wait_up { Reading::Between } else { Reading::Roomy }
    }

    /// Records that `spent` was used `at` with no picture of its own (a repeat): it counts against the pictures'
    /// time, judged at the next picture.
    pub(crate) fn spent(&mut self, at: Duration, spent: f64) {
        if at.saturating_sub(self.stepped) >= SETTLE {
            self.recent.push_back((at, spent, 0.0, Duration::ZERO));
        }
    }

    /// The share took a step, `load` the way: what was recorded before it is of another stream.
    pub(crate) fn stepped(&mut self, at: Duration, load: Load) {
        match load {
            Load::Over => {
                // A step up undone soon after waits twice as long the next time.
                if self.went_up.is_some_and(|up| at.saturating_sub(up) < self.wait_up) {
                    self.wait_up = (self.wait_up * 2).min(LONGEST_WAIT_UP);
                }
                self.went_up = None;
            }
            Load::Under => self.went_up = Some(at),
        }
        self.recent.clear();
        self.stepped = at;
        self.roomy_since = None;
    }
}

/// The encoder's load, from the time each picture took.
#[derive(Clone, Debug)]
pub struct KeepUp {
    judge: Judge,
}

impl KeepUp {
    /// A share that began at `at`.
    pub fn new(at: Duration) -> Self {
        Self { judge: Judge::new(at, OVERUSED_PERCENT, UNDERUSED_PERCENT) }
    }

    /// Records that a picture given `at` took `took` of its `interval`, and says what to do, if anything. A step
    /// asked for is taken as soon as it is said: the next is judged afresh.
    pub fn record(&mut self, at: Duration, took: Duration, interval: Duration) -> Option<Load> {
        let load = self.read(at, took, interval)?;
        self.judge.stepped(at, load);
        Some(load)
    }

    /// Records that the encoder was busy `took` at `at` with no picture of its own (a repeat): its time counts against
    /// the pictures' intervals, judged at the next picture.
    pub fn busy(&mut self, at: Duration, took: Duration) {
        self.judge.spent(at, took.as_secs_f64());
    }

    /// As [`KeepUp::record`], without the step: the share takes it, or not, and says so by [`KeepUp::stepped`].
    pub(crate) fn read(&mut self, at: Duration, took: Duration, interval: Duration) -> Option<Load> {
        if interval.is_zero() {
            return None;
        }
        match self.judge.record(at, took.as_secs_f64(), interval.as_secs_f64(), interval) {
            Reading::Over => Some(Load::Over),
            Reading::Roomy => Some(Load::Under),
            Reading::Between => None,
        }
    }

    /// The share took a step `load`ward at `at`.
    pub(crate) fn stepped(&mut self, at: Duration, load: Load) {
        self.judge.stepped(at, load);
    }
}

/// The bits a share asks for against what its rate gives over the same time: the bits its pictures are made of, and a
/// picture's share of the rate for each the budget held back. A scene the encoder cannot code at the rate asked (its
/// coarsest quantiser still makes more) is one its time does not show; a screen that changes seldom but much has the
/// time between its pictures, so it is no overuse.
#[derive(Clone, Debug)]
pub(crate) struct Spend {
    judge: Judge,
    /// When the last picture was read, or the share began or stepped.
    last: Duration,
}

impl Spend {
    /// A share that began at `at`.
    pub(crate) fn new(at: Duration) -> Self {
        Self { judge: Judge::new(at, BITS_OVERUSED_PERCENT, BITS_UNDERUSED_PERCENT), last: at }
    }

    /// Records that a picture given `at` was made of `bits` at `bits_per_second`, against the rate since the last
    /// picture; none says nothing. A key frame is not recorded: the encoder holds it to its own budget.
    #[expect(clippy::cast_precision_loss, reason = "bits a second and a picture's bits, far under 2^52")]
    pub(crate) fn read(&mut self, at: Duration, bits: u64, bits_per_second: u64) -> Option<Load> {
        let since = at.saturating_sub(std::mem::replace(&mut self.last, at)).min(MEASURED_OVER);
        let had = bits_per_second as f64 * since.as_secs_f64();
        match self.judge.record(at, bits as f64, had, since) {
            Reading::Over => Some(Load::Over),
            Reading::Roomy => Some(Load::Under),
            Reading::Between => None,
        }
    }

    /// Records that the budget held back a picture `at`, which asked for its `interval`'s share of `bits_per_second`.
    #[expect(clippy::cast_precision_loss, reason = "bits a second, far under 2^52")]
    pub(crate) fn held(&mut self, at: Duration, bits_per_second: u64, interval: Duration) {
        self.judge.spent(at, bits_per_second as f64 * interval.as_secs_f64());
    }

    /// The share took a step `load`ward at `at`.
    pub(crate) fn stepped(&mut self, at: Duration, load: Load) {
        self.judge.stepped(at, load);
        self.last = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIXTY: Duration = Duration::from_micros(16_667);

    /// Feeds pictures `took` long at 60 a second from `from` for `seconds`, returning each step and when it came.
    fn feed(keep_up: &mut KeepUp, from: Duration, seconds: u32, took: Duration) -> Vec<(Duration, Load)> {
        (0..seconds * 60)
            .filter_map(|n| {
                let at = from + SIXTY * n;
                keep_up.record(at, took, SIXTY).map(|load| (at, load))
            })
            .collect()
    }

    #[test]
    fn an_encoder_that_keeps_up_is_left_alone() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        assert!(feed(&mut keep_up, Duration::ZERO, 60, Duration::from_millis(10)).is_empty(), "60 % of its interval");
    }

    #[test]
    fn one_that_does_not_is_asked_for_less_once_the_share_has_settled_and_again_only_after_the_next_settles() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        let steps = feed(&mut keep_up, Duration::ZERO, 7, Duration::from_millis(25));
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert!(steps.iter().all(|(_, load)| *load == Load::Over));
        assert!(steps[0].0 >= SETTLE && steps[0].0 < SETTLE + MEASURED_OVER, "a second measured, {steps:?}");
        assert!(steps[1].0 - steps[0].0 >= SETTLE, "a step's new stream is not judged before it settles");
    }

    #[test]
    fn an_encoder_so_slow_its_pictures_come_late_is_overused() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        let late = Duration::from_millis(50);
        let steps: Vec<_> =
            (0..100).filter_map(|n| keep_up.record(late * n, Duration::from_millis(40), SIXTY)).collect();
        assert_eq!(steps.first(), Some(&Load::Over), "20 a second, each taking more than its sixtieth");
    }

    #[test]
    fn a_slow_picture_now_and_then_is_not_overuse() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        let steps: Vec<_> = (0..60 * 60)
            .filter_map(|n| {
                let took = if n % 30 == 0 { Duration::from_millis(60) } else { Duration::from_millis(8) };
                keep_up.record(SIXTY * n, took, SIXTY)
            })
            .collect();
        assert!(!steps.contains(&Load::Over), "a key frame twice a second, four intervals long, among quick pictures");
    }

    #[test]
    fn room_that_lasts_asks_for_more_and_a_step_up_undone_waits_twice_as_long_the_next_time() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        let roomy = Duration::from_millis(3);
        let first = feed(&mut keep_up, Duration::ZERO, 20, roomy);
        assert_eq!(first.iter().map(|(_, load)| *load).collect::<Vec<_>>(), [Load::Under]);
        let up = first[0].0;
        assert!(up >= SETTLE + FIRST_WAIT_UP && up < SETTLE + MEASURED_OVER + FIRST_WAIT_UP, "{up:?}");
        // Too much at the new step: undone. Each feed carries on from where the last one ended.
        let over = feed(&mut keep_up, Duration::from_secs(20), 4, Duration::from_millis(25));
        assert_eq!(over.first().map(|(_, load)| *load), Some(Load::Over));
        let down = over[0].0;
        assert_eq!(over.len(), 2, "on down again: {over:?}");
        let again = feed(&mut keep_up, Duration::from_secs(24), 40, roomy);
        let next_up = again.first().expect("room again, in time").0;
        assert!(next_up - down >= SETTLE + FIRST_WAIT_UP * 2, "waited {:?}", next_up - down);
    }

    #[test]
    fn a_repeat_between_pictures_counts_against_their_intervals() {
        let thirtieth = SIXTY * 2;
        let took = Duration::from_millis(15);
        let run = |repeats: bool| {
            let mut keep_up = KeepUp::new(Duration::ZERO);
            (0..30 * 6)
                .filter_map(|n| {
                    let at = thirtieth * n;
                    let load = keep_up.record(at, took, thirtieth);
                    if repeats {
                        keep_up.busy(at + SIXTY, took);
                    }
                    load
                })
                .collect::<Vec<_>>()
        };
        assert!(run(false).is_empty(), "45 % of a thirtieth alone");
        assert_eq!(run(true).first(), Some(&Load::Over), "90 % with a repeat in each interval");
    }

    #[test]
    fn an_empty_interval_is_no_measurement() {
        let mut keep_up = KeepUp::new(Duration::ZERO);
        assert_eq!(keep_up.record(SETTLE * 2, Duration::from_secs(1), Duration::ZERO), None);
    }
}
