//! When a watched stream's decoding moves between the graphics card and the processor: only once a trial of the other
//! path, decoding beside the one that paints, has kept up for its span, so a move never makes a stream that lags worse
//! off. A move to the processor is tried again later on the graphics card, with the same proof.

use media::Path;
use std::time::{Duration, Instant};

/// How long a graphics decoder is behind its stream, without a picture that caught up, before software is tried beside
/// it: a passing burst of work is no reason to move.
const BEHIND_FOR: Duration = Duration::from_secs(5);

/// How long a trial is judged keeping up before it takes over. A first value, not yet timed.
const TRIAL_KEEPS_UP: Duration = Duration::from_secs(3);

/// The longest a trial goes unjudged before it counts as lagging: its key frame waits for a viewer behind its stream's
/// asks (up to 8 s), then a second of pictures is judged.
const TRIAL_LONGEST: Duration = Duration::from_secs(20);

/// The wait after a software trial that lagged before the next, doubled at each up to [`SOFTWARE_AGAIN_MOST`].
const SOFTWARE_AGAIN_FIRST: Duration = Duration::from_secs(10);

/// The longest wait between two software trials.
const SOFTWARE_AGAIN_MOST: Duration = Duration::from_secs(160);

/// How long a stream decodes in software before its graphics decoder is tried again, doubled at each trial that lagged
/// and at each move back to software within [`HARDWARE_AGAIN_MOST`] of the last move to the graphics card, up to it.
const HARDWARE_AGAIN_FIRST: Duration = Duration::from_secs(30);

/// The longest wait between two graphics trials.
const HARDWARE_AGAIN_MOST: Duration = Duration::from_secs(480);

/// What a trial's pictures say so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not a second of its pictures yet.
    Unjudged,
    /// It decodes within its stream's time.
    KeepsUp,
    /// It fell behind, missed a unit, refused one or left the path it was tried on; it stays so.
    Lags,
}

/// What the decoding thread does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing.
    Stay,
    /// Open a trial on this path, from the stream's next key frame.
    Try(Path),
    /// The trial kept up: it paints from now, in the place of the decoder that painted.
    Take,
    /// The trial is dropped; the decoder that paints goes on.
    Drop,
}

/// A trial running.
#[derive(Debug)]
struct Trying {
    to: Path,
    began: Instant,
    /// Since when its verdict has been that it keeps up.
    keeps_up_since: Option<Instant>,
}

/// Where one stream's pictures are decoded, and when the other path is tried.
#[derive(Debug)]
pub(crate) struct Fallback {
    /// The path of the decoder that paints.
    painting: Path,
    /// The graphics path the stream last painted from; none for one that never did, which software never leaves.
    hardware: Option<Path>,
    /// Since when the graphics decoder has been behind, while it is.
    behind_since: Option<Instant>,
    trying: Option<Trying>,
    /// The earliest the next trial starts; none for no wait.
    not_before: Option<Instant>,
    software_again: Duration,
    hardware_again: Duration,
    /// When a trial last moved the stream back onto its graphics card.
    back_on_hardware: Option<Instant>,
}

impl Fallback {
    /// A stream whose decoder paints from `painting`.
    pub(crate) fn new(painting: Path) -> Self {
        Self {
            painting,
            hardware: matches!(painting, Path::Hardware(_)).then_some(painting),
            behind_since: None,
            trying: None,
            not_before: None,
            software_again: SOFTWARE_AGAIN_FIRST,
            hardware_again: HARDWARE_AGAIN_FIRST,
            back_on_hardware: None,
        }
    }

    /// The decoder that paints moved to `path` by itself at `now`, a broken one having handed over: a trial running is
    /// dropped, and one moved to software tries its graphics card again after its wait.
    pub(crate) fn moved(&mut self, path: Path, now: Instant) -> Step {
        self.painting = path;
        self.behind_since = None;
        match path {
            Path::Hardware(_) => self.hardware = Some(path),
            Path::Software => self.not_before = Some(now + self.hardware_again),
        }
        if self.trying.take().is_some() { Step::Drop } else { Step::Stay }
    }

    /// The decoder that paints made a picture at `now`, judged `behind` its stream or not. A graphics decoder behind
    /// for [`BEHIND_FOR`] has software tried beside it, and one that caught up drops it; software tries the graphics
    /// card again once its wait is over.
    pub(crate) fn painted(&mut self, now: Instant, behind: bool) -> Step {
        let waited = self.not_before.is_none_or(|at| now >= at);
        match self.painting {
            Path::Hardware(_) if !behind => {
                self.behind_since = None;
                if self.trying.take().is_some() { Step::Drop } else { Step::Stay }
            }
            Path::Hardware(_) => {
                let since = *self.behind_since.get_or_insert(now);
                if self.trying.is_none() && waited && now.duration_since(since) >= BEHIND_FOR {
                    self.start(Path::Software, now)
                } else {
                    Step::Stay
                }
            }
            Path::Software => match self.hardware {
                Some(hardware) if self.trying.is_none() && waited => self.start(hardware, now),
                Some(_) | None => Step::Stay,
            },
        }
    }

    /// The trial running says `verdict` at `now`: it takes over once it has kept up for [`TRIAL_KEEPS_UP`], and is
    /// dropped once it lags or has gone unjudged for [`TRIAL_LONGEST`].
    pub(crate) fn tried(&mut self, now: Instant, verdict: Verdict) -> Step {
        let Some(trying) = &mut self.trying else { return Step::Stay };
        match verdict {
            Verdict::Lags => self.lagged(now),
            Verdict::Unjudged if now.duration_since(trying.began) >= TRIAL_LONGEST => self.lagged(now),
            Verdict::Unjudged => Step::Stay,
            Verdict::KeepsUp => {
                let since = *trying.keeps_up_since.get_or_insert(now);
                if now.duration_since(since) < TRIAL_KEEPS_UP {
                    return Step::Stay;
                }
                let to = trying.to;
                self.trying = None;
                self.took(to, now);
                Step::Take
            }
        }
    }

    /// The trial asked for at `now` did not open or start, or ended: it counts as one that lagged.
    pub(crate) fn failed(&mut self, now: Instant) {
        self.lagged(now);
    }

    fn start(&mut self, to: Path, now: Instant) -> Step {
        self.trying = Some(Trying { to, began: now, keeps_up_since: None });
        Step::Try(to)
    }

    /// The trial lagged at `now`: it is dropped, and the next of its way waits longer than the last.
    fn lagged(&mut self, now: Instant) -> Step {
        let Some(trying) = self.trying.take() else { return Step::Stay };
        let wait = match trying.to {
            Path::Software => {
                let wait = self.software_again;
                self.software_again = (wait * 2).min(SOFTWARE_AGAIN_MOST);
                wait
            }
            Path::Hardware(_) => {
                self.hardware_again = (self.hardware_again * 2).min(HARDWARE_AGAIN_MOST);
                self.hardware_again
            }
        };
        self.not_before = Some(now + wait);
        Step::Drop
    }

    /// The trial on `to` took over at `now`. On software the graphics card is tried again after its wait, which starts
    /// over unless the card lost the stream again soon after it won it back, so a stream that swings between the two
    /// swings ever more slowly.
    fn took(&mut self, to: Path, now: Instant) {
        self.painting = to;
        self.behind_since = None;
        match to {
            Path::Software => {
                self.software_again = SOFTWARE_AGAIN_FIRST;
                self.hardware_again = match self.back_on_hardware.take() {
                    Some(back) if now.duration_since(back) < HARDWARE_AGAIN_MOST => {
                        (self.hardware_again * 2).min(HARDWARE_AGAIN_MOST)
                    }
                    Some(_) | None => HARDWARE_AGAIN_FIRST,
                };
                self.not_before = Some(now + self.hardware_again);
            }
            Path::Hardware(_) => {
                self.hardware = Some(to);
                self.back_on_hardware = Some(now);
                self.not_before = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::Backend;

    const GPU: Path = Path::Hardware(Backend::Vulkan);
    const S: Duration = Duration::from_secs(1);
    const MS: Duration = Duration::from_millis(1);

    /// A graphics decoder behind from `start` until software is tried, at `start` + [`BEHIND_FOR`].
    fn trying_software(start: Instant) -> Fallback {
        let mut fallback = Fallback::new(GPU);
        assert_eq!(fallback.painted(start, true), Step::Stay);
        assert_eq!(fallback.painted(start + BEHIND_FOR, true), Step::Try(Path::Software));
        fallback
    }

    /// A stream behind from `start` and moved to software by a trial that kept up from the start; when it moved.
    fn on_software(start: Instant) -> (Fallback, Instant) {
        let mut fallback = trying_software(start);
        assert_eq!(fallback.tried(start + BEHIND_FOR, Verdict::KeepsUp), Step::Stay);
        let moved = start + BEHIND_FOR + TRIAL_KEEPS_UP;
        assert_eq!(fallback.tried(moved, Verdict::KeepsUp), Step::Take);
        assert_eq!(fallback.painting, Path::Software);
        (fallback, moved)
    }

    #[test]
    fn hardware_behind_4_9_s_stays() {
        let start = Instant::now();
        let mut fallback = Fallback::new(GPU);
        for at in (0..=49).map(|tenths| start + tenths * 100 * MS) {
            assert_eq!(fallback.painted(at, true), Step::Stay);
        }
        assert_eq!(fallback.painting, GPU);
        assert_eq!(fallback.painted(start + 5 * S, false), Step::Stay, "caught up: the count starts over");
        assert_eq!(fallback.painted(start + 6 * S, true), Step::Stay);
        assert_eq!(fallback.painted(start + 10 * S + 900 * MS, true), Step::Stay, "4.9 s behind again");
    }

    #[test]
    fn behind_5_s_with_a_software_trial_that_lags_stays_on_hardware() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        assert_eq!(fallback.painted(start + 6 * S, true), Step::Stay, "one trial at a time");
        assert_eq!(fallback.tried(start + 6 * S, Verdict::Unjudged), Step::Stay);
        assert_eq!(fallback.tried(start + 7 * S, Verdict::Lags), Step::Drop);
        assert_eq!(fallback.painting, GPU, "the graphics decoder paints on");
        assert_eq!(fallback.tried(start + 8 * S, Verdict::KeepsUp), Step::Stay, "no trial runs");
    }

    #[test]
    fn a_trial_that_keeps_up_moves_to_software() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        let judged = start + 6 * S;
        assert_eq!(fallback.tried(judged, Verdict::KeepsUp), Step::Stay);
        assert_eq!(fallback.tried(judged + TRIAL_KEEPS_UP - MS, Verdict::KeepsUp), Step::Stay, "not yet its span");
        assert_eq!(fallback.tried(judged + TRIAL_KEEPS_UP, Verdict::KeepsUp), Step::Take);
        assert_eq!(fallback.painting, Path::Software);
    }

    #[test]
    fn a_trial_unjudged_for_its_longest_lags() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        let began = start + BEHIND_FOR;
        assert_eq!(fallback.tried(began + TRIAL_LONGEST - MS, Verdict::Unjudged), Step::Stay);
        assert_eq!(fallback.tried(began + TRIAL_LONGEST, Verdict::Unjudged), Step::Drop);
    }

    #[test]
    fn a_graphics_decoder_that_catches_up_drops_its_software_trial() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        assert_eq!(fallback.painted(start + 6 * S, false), Step::Drop);
        assert_eq!(fallback.painted(start + 7 * S, true), Step::Stay, "behind afresh: its 5 s start over");
        assert_eq!(fallback.painted(start + 12 * S, true), Step::Try(Path::Software), "no wait: none lagged");
    }

    #[test]
    fn software_retries_hardware_after_its_wait() {
        let (mut fallback, moved) = on_software(Instant::now());
        assert_eq!(fallback.painted(moved + HARDWARE_AGAIN_FIRST - MS, false), Step::Stay);
        assert_eq!(fallback.painted(moved + HARDWARE_AGAIN_FIRST, false), Step::Try(GPU));
        let judged = moved + HARDWARE_AGAIN_FIRST + 2 * S;
        assert_eq!(fallback.tried(judged, Verdict::KeepsUp), Step::Stay);
        assert_eq!(fallback.tried(judged + TRIAL_KEEPS_UP, Verdict::KeepsUp), Step::Take);
        assert_eq!(fallback.painting, GPU, "back on the graphics card, which kept up");
    }

    #[test]
    fn each_trial_that_lags_waits_twice_as_long_before_the_next_up_to_its_most() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        let mut at = start + BEHIND_FOR;
        let mut waits = Vec::new();
        for _ in 0..6 {
            assert_eq!(fallback.tried(at, Verdict::Lags), Step::Drop);
            let lagged = at;
            let next = (1..).map(|step| lagged + step * S).find(|&now| fallback.painted(now, true) != Step::Stay);
            let next = next.expect("software is tried again");
            waits.push((next - lagged).as_secs());
            at = next;
        }
        assert_eq!(waits, [10, 20, 40, 80, 160, 160]);

        let (mut fallback, moved) = on_software(start);
        let mut at = moved + HARDWARE_AGAIN_FIRST;
        assert_eq!(fallback.painted(at, false), Step::Try(GPU));
        let mut waits = Vec::new();
        for _ in 0..5 {
            fallback.failed(at);
            let failed = at;
            let next = (1..).map(|step| failed + step * S).find(|&now| fallback.painted(now, false) != Step::Stay);
            let next = next.expect("the graphics card is tried again");
            waits.push((next - failed).as_secs());
            at = next;
        }
        assert_eq!(waits, [60, 120, 240, 480, 480]);
    }

    #[test]
    fn a_stream_that_swings_back_to_software_soon_after_waits_longer_each_time() {
        let (mut fallback, mut moved) = on_software(Instant::now());
        let mut waits = Vec::new();
        for _ in 0..3 {
            let next = (1..).map(|step| moved + step * S).find(|&now| fallback.painted(now, false) != Step::Stay);
            let tried = next.expect("the graphics card is tried again");
            waits.push((tried - moved).as_secs());
            assert_eq!(fallback.tried(tried, Verdict::KeepsUp), Step::Stay);
            let back = tried + TRIAL_KEEPS_UP;
            assert_eq!(fallback.tried(back, Verdict::KeepsUp), Step::Take);
            assert_eq!(fallback.painted(back + S, true), Step::Stay);
            assert_eq!(fallback.painted(back + S + BEHIND_FOR, true), Step::Try(Path::Software), "behind again");
            let judged = back + S + BEHIND_FOR;
            assert_eq!(fallback.tried(judged, Verdict::KeepsUp), Step::Stay);
            moved = judged + TRIAL_KEEPS_UP;
            assert_eq!(fallback.tried(moved, Verdict::KeepsUp), Step::Take);
        }
        assert_eq!(waits, [30, 60, 120], "each swing back waits twice the last");
    }

    #[test]
    fn a_stream_that_never_painted_on_a_graphics_card_stays_in_software() {
        let start = Instant::now();
        let mut fallback = Fallback::new(Path::Software);
        assert_eq!(fallback.painted(start + HARDWARE_AGAIN_MOST * 2, true), Step::Stay);
    }

    #[test]
    fn a_broken_graphics_decoder_handed_over_to_software_is_tried_again_after_its_wait() {
        let start = Instant::now();
        let mut fallback = trying_software(start);
        assert_eq!(fallback.moved(Path::Software, start + 6 * S), Step::Drop, "the trial goes with the move");
        assert_eq!(fallback.painted(start + 6 * S + HARDWARE_AGAIN_FIRST - MS, false), Step::Stay);
        assert_eq!(fallback.painted(start + 6 * S + HARDWARE_AGAIN_FIRST, false), Step::Try(GPU));
    }
}
