//! A call's microphone and speaker, opened by this client on the system's sound server by the device's own name,
//! and kept: a stream that fails is opened again, a device chosen anew takes over once it runs, and the speaker
//! follows the clock its frames arrive on by resampling, never by dropping or repeating a frame. Frames are 10 ms of
//! 16-bit samples at [`RATE`]: the microphone's one channel, the speaker's two.

use net::Backoff;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

mod kept;
#[cfg(target_os = "linux")]
#[cfg_attr(test, expect(dead_code, reason = "a unit test keeps devices of its own making"))]
mod linux;
mod realtime;
#[cfg(windows)]
#[cfg_attr(test, expect(dead_code, reason = "a unit test keeps devices of its own making"))]
mod windows;
// A unit test keeps devices of its own making, so what keeps them is tested without a sound server.
#[cfg(test)]
mod fake;
#[cfg(test)]
use self::fake as system;
#[cfg(all(target_os = "linux", not(test)))]
use self::linux as system;
#[cfg(all(windows, not(test)))]
use self::windows as system;

#[cfg(target_os = "linux")]
pub use self::linux::listing;
#[cfg(windows)]
pub(crate) use self::windows::{Apartment, communications_default, listing};
pub use kept::{Frames, Microphone, Speaker, Voiced};
#[cfg(test)]
pub(crate) use realtime::ahead;
pub use realtime::{Realtime, realtime};

/// One device the sound server lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// What it is opened by: PipeWire's `node.name`, WASAPI's endpoint id.
    pub id: String,
    /// What a person reads.
    pub label: String,
}

/// The devices the sound server lists now, and which of each is its default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The microphones, in the system's order.
    pub microphones: Vec<Listed>,
    /// The speakers, in the system's order.
    pub speakers: Vec<Listed>,
    /// The id of the default microphone, when the system says.
    pub default_microphone: Option<String>,
    /// The id of the default speaker, when the system says.
    pub default_speaker: Option<String>,
}

/// One microphone stream on the system's sound server, handing over whatever it hears.
trait Capture: Sized {
    /// Opens `device` (none: the default), each quantum of mono samples at [`RATE`] handed to `hear` from the
    /// server's own thread.
    fn open(device: Option<&str>, hear: impl FnMut(&[i16]) + Send + 'static) -> Result<Self, VoiceError>;
    /// Whether its sound flows.
    fn health(&self) -> Health;
    /// How long a sound takes from the device to being handed over.
    fn delay(&self) -> Duration;
}

/// One speaker stream on the system's sound server, asking for each quantum it plays.
trait Playback: Sized {
    /// Opens `device` (none: the default), each quantum of interleaved stereo at [`RATE`] asked of `fill` from the
    /// server's own thread.
    fn open(device: Option<&str>, fill: impl FnMut(&mut [i16]) + Send + 'static) -> Result<Self, VoiceError>;
    /// Whether its sound flows.
    fn health(&self) -> Health;
    /// How long a quantum filled now takes to reach the device.
    fn delay(&self) -> Duration;
    /// Plays at `ratio` of its nominal rate.
    fn set_rate(&self, ratio: f32);
}

/// The rate both devices run at, in samples per second per channel.
pub const RATE: u32 = 48_000;
/// The samples of one channel in a frame: 10 ms.
pub const FRAME: usize = RATE as usize / 100;
/// A frame of the microphone.
pub type Mono = [i16; FRAME];
/// A frame of the speaker, interleaved left then right.
pub type Stereo = [i16; 2 * FRAME];

/// How many microphone frames wait for their reader before the newest is dropped: 200 ms.
const MICROPHONE_KEPT: usize = 20;
/// How many speaker frames may wait to be played before the newest is dropped: 200 ms.
const SPEAKER_KEPT: usize = 20;
/// What the speaker keeps waiting beyond one quantum of the graph, so a frame late by up to this still plays on
/// time: 20 ms, in samples of one channel.
const MARGIN: usize = 2 * FRAME;

/// How often the speaker's rate is set from how full it ran.
const PACE_EVERY: Duration = Duration::from_millis(250);
/// How far past its aim the speaker may run before it is caught up by skipping, in samples of one channel: 40 ms,
/// a step the rate's half percent would take some ten seconds to work off.
const OVER: usize = RATE as usize * 40 / 1000;
/// How long the speaker must stay over [`OVER`] before it is caught up, so a burst that passes is not skipped.
const HOLD: Duration = Duration::from_millis(500);
/// How long after the catch-up began what it could not skip, no frame being quiet, is cut in one go.
const CUT_AFTER: Duration = Duration::from_secs(1);
/// The loudest a frame may peak and be skipped as silence: about -54 dBFS, under the room tone of a noise-suppressed
/// microphone and any speech or share sound, so a skipped frame is one nobody hears.
const QUIET_FRAME: u16 = 64;
/// The samples of one channel a cut is crossfaded over: 5 ms, past the ear's click and short of a syllable.
const FADE: usize = RATE as usize * 5 / 1000;
/// The waits before a failed stream is opened again, or a lost sound server reached again: doubling, the last repeated.
pub(crate) const RETRY: Backoff = Backoff::new(Duration::from_millis(250), Duration::from_secs(2));

/// Why a device did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceError {
    /// This system has no sound server this client speaks to.
    Unavailable,
    /// The sound server refused the stream.
    Stream,
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("this system has no sound server for a call"),
            Self::Stream => f.write_str("the sound server refused the call's stream"),
        }
    }
}

impl std::error::Error for VoiceError {}

/// Whether a device's sound flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// It flows.
    Running,
    /// Its stream is being linked, or waits for the device it names to be there.
    Starting,
    /// Its stream failed, or the sound server went away; it is opened again in a moment.
    Stopped,
}

/// When a failed stream is next tried, waiting longer each time it fails again.
#[derive(Debug, Default)]
pub(crate) struct Retry {
    failures: u32,
    next: Option<Instant>,
}

impl Retry {
    /// Whether a stream that is down may be tried at `now`; the first time it is seen down, it is put off.
    pub(crate) fn due(&mut self, now: Instant) -> bool {
        let next = *self.next.get_or_insert(now + RETRY.wait(1));
        if now < next {
            return false;
        }
        self.failures += 1;
        self.next = Some(now + RETRY.wait(self.failures.saturating_add(1)));
        true
    }

    /// The stream runs again: the next failure is waited on from the start.
    pub(crate) fn ran(&mut self) {
        *self = Self::default();
    }
}

/// What the speaker's real-time side tells its owner, through atomics only.
#[derive(Debug, Default)]
struct Gauge {
    /// The samples of one channel waiting to be played, summed at each quantum since the owner last read.
    level_sum: AtomicU64,
    /// The quanta summed.
    level_count: AtomicU64,
    /// The graph's last quantum, in samples of one channel.
    quantum: AtomicU32,
    /// Times the speaker ran out and played silence until it had its margin again.
    underruns: AtomicU64,
    /// Samples of one channel the owner asks the playout to drop, whole quiet frames at a time; it takes what it
    /// spends off.
    skip: AtomicU32,
    /// Samples of one channel the owner asks the playout to cut at its next frame boundary, crossfaded; it takes
    /// the ask off.
    cut: AtomicU32,
}

impl Gauge {
    /// How full the speaker ran on average since the last read, and the level it aims at, in samples of one channel;
    /// none when no quantum ran.
    fn take(&self) -> Option<(f64, f64)> {
        let count = self.level_count.swap(0, Ordering::AcqRel);
        let sum = self.level_sum.swap(0, Ordering::AcqRel);
        #[expect(clippy::cast_precision_loss, reason = "samples of a fraction of a second")]
        let level = (count > 0).then(|| sum as f64 / count as f64)?;
        #[expect(clippy::cast_precision_loss, reason = "a quantum is at most a few thousand samples")]
        let target = target(self.quantum.load(Ordering::Acquire) as usize) as f64;
        Some((level, target))
    }
}

/// The samples of one channel the speaker keeps waiting when the graph asks for `quantum` at a time.
fn target(quantum: usize) -> usize {
    quantum + MARGIN
}

/// The speaker's real-time side: the frames to play, cut to whatever quantum the graph asks for. Silence until the
/// margin has filled, and again after running out, so a late frame is waited for once rather than every quantum.
struct Playout {
    ring: rtrb::Consumer<Stereo>,
    frame: Stereo,
    /// How many samples of `frame` are played; a whole frame's means it is used up.
    at: usize,
    primed: bool,
    gauge: Arc<Gauge>,
    /// The start of the frame a cut dropped first, which fades out as the frame after the cut fades in; none unless
    /// a cut is waiting for that frame.
    fade: Option<[i16; 2 * FADE]>,
}

impl Playout {
    fn new(ring: rtrb::Consumer<Stereo>, gauge: Arc<Gauge>) -> Self {
        Self { ring, frame: [0; 2 * FRAME], at: 2 * FRAME, primed: false, gauge, fade: None }
    }

    /// Spends what the owner asked at a frame boundary: whole quiet frames are dropped from the front of the ring
    /// while the skip asked lasts, and a cut is made. Always leaves a frame to play.
    fn catch_up(&mut self) {
        let cut = self.gauge.cut.swap(0, Ordering::Relaxed) as usize / FRAME;
        let cut = cut.min(self.ring.slots().saturating_sub(1));
        if cut > 0 {
            self.cut(cut);
        }
        let budget = self.gauge.skip.load(Ordering::Relaxed) as usize;
        let mut spent = 0;
        while budget - spent >= FRAME && self.ring.slots() > 1 && self.ring.peek().is_ok_and(quiet) {
            let _ = self.ring.pop();
            spent += FRAME;
        }
        if spent > 0 {
            let spent = u32::try_from(spent).unwrap_or(u32::MAX);
            let _ = self
                .gauge
                .skip
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| Some(left.saturating_sub(spent)));
        }
    }

    /// Drops `frames` from the front of the ring, which holds one more than that; the start of the first dropped is
    /// kept to fade out against the frame after them, so no sample jumps.
    fn cut(&mut self, frames: usize) {
        let Ok(first) = self.ring.pop() else { return };
        let mut from = [0; 2 * FADE];
        from.copy_from_slice(&first[..2 * FADE]);
        self.fade = Some(from);
        for _ in 1..frames {
            let _ = self.ring.pop();
        }
    }

    /// The samples of one channel waiting.
    fn level(&self) -> usize {
        self.ring.slots() * FRAME + (2 * FRAME - self.at) / 2
    }

    /// Writes one quantum of interleaved stereo into `out`.
    fn fill(&mut self, out: &mut [i16]) {
        let quantum = out.len() / 2;
        self.gauge.quantum.store(u32::try_from(quantum).unwrap_or(u32::MAX), Ordering::Release);
        if !self.primed && self.level() >= target(quantum) {
            self.primed = true;
        }
        let mut written = 0;
        while self.primed && written < out.len() {
            if self.at == self.frame.len() {
                self.catch_up();
                let Ok(mut next) = self.ring.pop() else {
                    self.primed = false;
                    self.gauge.underruns.fetch_add(1, Ordering::Relaxed);
                    break;
                };
                if let Some(from) = self.fade.take() {
                    crossfade(&mut next, &from);
                }
                (self.frame, self.at) = (next, 0);
            }
            let taken = (self.frame.len() - self.at).min(out.len() - written);
            out[written..written + taken].copy_from_slice(&self.frame[self.at..self.at + taken]);
            (written, self.at) = (written + taken, self.at + taken);
        }
        out[written..].fill(0);
        self.gauge.level_sum.fetch_add(self.level() as u64, Ordering::Relaxed);
        self.gauge.level_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether a frame is quiet enough to be skipped without anyone hearing it go.
fn quiet(frame: &Stereo) -> bool {
    frame.iter().all(|sample| sample.unsigned_abs() <= QUIET_FRAME)
}

/// Fades the start of `next` in over [`FADE`] samples of each channel as `from`, what played on from where the cut
/// was made, fades out: the sound is `from` itself at the first sample and `next`'s own at the last.
fn crossfade(next: &mut Stereo, from: &[i16; 2 * FADE]) {
    let length = i32::try_from(FADE).unwrap_or(i32::MAX);
    for (at, (sample, old)) in next.iter_mut().zip(from).enumerate() {
        let new_weight = i32::try_from(at / 2).unwrap_or(length);
        let mixed = (i32::from(*old) * (length - new_weight) + i32::from(*sample) * new_weight) / length;
        *sample = i16::try_from(mixed).unwrap_or(*sample);
    }
}

/// Where the speaker is in catching up with a delay it has run up: what is asked of it, once and then again if
/// skipping the quiet was not enough.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CatchUp {
    /// Within [`OVER`] of its aim.
    #[default]
    Within,
    /// Over it since this time, not yet for [`HOLD`].
    Over(Instant),
    /// Skipping quiet frames since this time.
    Skipping(Instant),
    /// What was left was cut; the rate follows the rest.
    Cut,
}

/// What the owner asks of the playout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Drop this many samples of one channel as quiet frames come.
    Skip(usize),
    /// Cut this many samples of one channel now, crossfaded.
    Cut(usize),
}

impl CatchUp {
    /// What to ask at `now` of a speaker that ran at `level` against its `aim`, both in samples of one channel, with
    /// `left` of an earlier skip still unspent.
    fn step(&mut self, now: Instant, level: f64, aim: f64, left: usize) -> Option<Ask> {
        #[expect(clippy::cast_precision_loss, reason = "a few thousand samples")]
        let over = level - aim > OVER as f64;
        match (*self, over) {
            (_, false) => {
                *self = Self::Within;
                None
            }
            (Self::Within, true) => {
                *self = Self::Over(now);
                None
            }
            (Self::Over(since), true) if now.saturating_duration_since(since) >= HOLD => {
                *self = Self::Skipping(now);
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "positive, a few thousand")]
                Some(Ask::Skip((level - aim) as usize))
            }
            (Self::Skipping(since), true) if left > 0 && now.saturating_duration_since(since) >= CUT_AFTER => {
                *self = Self::Cut;
                Some(Ask::Cut(left))
            }
            (Self::Over(_) | Self::Skipping(_) | Self::Cut, true) => None,
        }
    }
}

/// The speaker's rate against its device, from how full it runs: a ring fuller than its aim plays faster, an emptier
/// one slower, so it follows the clock its frames come on. A proportional term answers a step, an integral one the
/// steady drift of two crystals (some hundreds of parts per million).
#[derive(Debug, Default)]
struct Pace {
    /// The error, in seconds of sound, summed over time.
    integral: f64,
}

impl Pace {
    const PROPORTIONAL: f64 = 0.1;
    const INTEGRAL: f64 = 0.01;
    /// The most it ever bends the rate: half a percent, far past any crystal's drift.
    const BOUND: f64 = 0.005;

    /// The rate to play at, from how full the speaker ran over the last `elapsed` against its aim, both in samples of
    /// one channel.
    fn rate(&mut self, level: f64, target: f64, elapsed: Duration) -> f64 {
        let error = (level - target) / f64::from(RATE);
        // Bounded so a long run out of reach (no frames, a device stalled) does not wind it up.
        let reach = Self::BOUND / Self::INTEGRAL;
        self.integral = (self.integral + error * elapsed.as_secs_f64()).clamp(-reach, reach);
        1.0 + (Self::PROPORTIONAL * error + Self::INTEGRAL * self.integral).clamp(-Self::BOUND, Self::BOUND)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playout(frames: usize) -> (rtrb::Producer<Stereo>, Playout, Arc<Gauge>) {
        let (producer, consumer) = rtrb::RingBuffer::new(frames);
        let gauge = Arc::new(Gauge::default());
        (producer, Playout::new(consumer, Arc::clone(&gauge)), gauge)
    }

    fn numbered(n: i16) -> Stereo {
        [n; 2 * FRAME]
    }

    #[test]
    fn the_speaker_waits_for_its_margin_then_plays_every_sample_in_order_across_quanta() {
        let (mut producer, mut playout, gauge) = playout(8);
        let mut out = vec![1_i16; 2 * 256];
        producer.push(numbered(1)).unwrap();
        playout.fill(&mut out);
        assert!(out.iter().all(|sample| *sample == 0), "silence until the margin and one quantum wait");
        for n in 2..=4 {
            producer.push(numbered(n)).unwrap();
        }
        let mut played = Vec::new();
        for _ in 0..10 {
            playout.fill(&mut out);
            played.extend_from_slice(&out);
        }
        let expected: Vec<i16> = (1..=4).flat_map(|n| numbered(n).to_vec()).collect();
        assert_eq!(&played[..expected.len()], &expected[..], "each frame whole, in order, cut at any quantum");
        assert!(played[expected.len()..].iter().all(|sample| *sample == 0), "then silence");
        assert_eq!(gauge.underruns.load(Ordering::Relaxed), 1, "running out is told once");
        assert_eq!(gauge.quantum.load(Ordering::Relaxed), 256);
    }

    #[test]
    fn the_speaker_reports_how_full_it_ran_and_what_it_aims_at() {
        let (mut producer, mut playout, gauge) = playout(8);
        for n in 0..4 {
            producer.push(numbered(n)).unwrap();
        }
        let mut out = vec![0_i16; 2 * FRAME];
        playout.fill(&mut out);
        let (level, aim) = gauge.take().expect("a quantum ran");
        assert!((level - 3.0 * FRAME as f64).abs() < 1.0, "three frames wait after one is played: {level}");
        assert!((aim - (FRAME + MARGIN) as f64).abs() < 1.0);
        assert_eq!(gauge.take(), None, "read once");
    }

    /// The speaker's ring under a sender whose clock runs `drift` apart from the device's, for `seconds`: how full it
    /// ends, against its aim, in samples, and the rate it settled on.
    fn followed(drift: f64, seconds: u32) -> (f64, f64) {
        let mut pace = Pace::default();
        let (aim, mut level, mut rate) = (f64::from(3 * 480_u32), 1440.0, 1.0);
        let step = PACE_EVERY.as_secs_f64();
        let steps = u128::from(seconds) * 1000 / PACE_EVERY.as_millis();
        for _ in 0..steps {
            level += f64::from(RATE) * step * ((1.0 + drift) - rate);
            rate = pace.rate(level, aim, PACE_EVERY);
        }
        (level - aim, rate)
    }

    #[test]
    fn the_rate_follows_a_sender_whose_clock_runs_apart_and_the_level_stays_near_its_aim() {
        for drift in [-200e-6, -20e-6, 0.0, 20e-6, 200e-6] {
            let (off, rate) = followed(drift, 120);
            assert!(off.abs() < 48.0, "within 1 ms of its aim at {drift}: {off} samples");
            assert!((rate - 1.0 - drift).abs() < 5e-6, "the rate is the sender's at {drift}: {rate}");
        }
    }

    /// A speaker that ran `excess` samples over its aim from time zero with a sender in step, looked at every
    /// [`PACE_EVERY`]: when it is back within its aim, and what was asked of the playout. A quiet stream lets the
    /// skip be spent at once; a loud one leaves it to the cut.
    fn caught_up(excess: f64, quiet: bool) -> (Option<Duration>, Vec<Ask>) {
        let (aim, mut level) = (1440.0, 1440.0 + excess);
        let (mut catch, mut left, mut asked) = (CatchUp::default(), 0_usize, Vec::new());
        let start = Instant::now();
        for step in 1..=40_u32 {
            let now = start + PACE_EVERY * step;
            if let Some(ask) = catch.step(now, level, aim, left) {
                asked.push(ask);
                match ask {
                    Ask::Skip(samples) if quiet => level -= samples as f64,
                    Ask::Skip(samples) => left = samples,
                    Ask::Cut(samples) => {
                        level -= samples as f64;
                        left = 0;
                    }
                }
            }
            if level - aim <= 48.0 {
                return (Some(PACE_EVERY * step), asked);
            }
        }
        (None, asked)
    }

    #[test]
    fn a_ring_40_ms_past_its_aim_for_half_a_second_is_back_within_a_second() {
        // 45 ms over: more than a step the rate's bend would work off in seconds.
        let over = f64::from(RATE) * 0.045;
        let (back, asked) = caught_up(over, true);
        let back = back.expect("it is caught up");
        assert!(back <= HOLD + Duration::from_secs(1), "back at {back:?}, after the half second it must stay over");
        assert!(back >= HOLD, "not before it has stayed over for the half second: {back:?}");
        assert!(matches!(asked[..], [Ask::Skip(_)]), "quiet frames were enough: {asked:?}");
        // 38 ms over is within what the rate follows: nothing is asked.
        assert_eq!(caught_up(f64::from(RATE) * 0.038, true), (None, Vec::new()));
        // A stream with no quiet frame is cut a second after the skip began.
        let (back, asked) = caught_up(over, false);
        assert!(matches!(asked[..], [Ask::Skip(_), Ask::Cut(_)]), "skipped, then cut: {asked:?}");
        assert!(back.expect("cut") <= HOLD + CUT_AFTER + PACE_EVERY, "cut a second after the skip began: {back:?}");
    }

    #[test]
    fn a_quiet_frame_is_skipped_before_a_loud_one() {
        let (mut producer, mut playout, gauge) = playout(8);
        let mut loud = numbered(1000);
        loud[3] = 20_000;
        for frame in [numbered(5), loud, numbered(9), numbered(11)] {
            producer.push(frame).unwrap();
        }
        gauge.skip.store(u32::try_from(2 * FRAME).unwrap(), Ordering::Relaxed);
        let mut out = vec![0_i16; 2 * FRAME];
        playout.fill(&mut out);
        assert_eq!(out[0], 1000, "the quiet frame was skipped, and the loud one, which is not, stops the skip");
        let left = gauge.skip.load(Ordering::Relaxed) as usize;
        assert_eq!(left, FRAME, "one frame of the two asked was spent, the other waits for a quiet one");
    }

    #[test]
    fn a_cut_crossfades_without_a_jump_in_the_sound() {
        let (mut producer, mut playout, gauge) = playout(16);
        // A ramp of 4 a sample: what a cut drops is 4 frames of it, a step of 7 680 where the two ends are joined.
        let ramp = |frame: usize| -> Stereo {
            let mut samples = [0; 2 * FRAME];
            for (at, pair) in samples.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *pair = [i16::try_from((frame * FRAME + at) * 4).unwrap(); 2];
            }
            samples
        };
        for frame in 0..12 {
            producer.push(ramp(frame)).unwrap();
        }
        let mut played = Vec::new();
        let mut out = vec![0_i16; 2 * FRAME];
        for round in 0..6 {
            if round == 2 {
                gauge.cut.store(u32::try_from(4 * FRAME).unwrap(), Ordering::Relaxed);
            }
            playout.fill(&mut out);
            played.extend(out.iter().step_by(2).copied());
        }
        let steps: Vec<i32> = played.windows(2).map(|pair| i32::from(pair[1]) - i32::from(pair[0])).collect();
        let (jump, rises) =
            (steps.iter().map(|step| step.abs()).max().unwrap(), steps.iter().filter(|s| **s != 0).count());
        assert!(rises > 4 * FRAME, "the sound played on past the cut");
        assert!(jump < 100, "the cut is a slope over 5 ms, not a step of thousands: {jump}");
        assert!(jump > 4, "and it did cut: nothing else bends the ramp");
        assert_eq!(gauge.cut.load(Ordering::Relaxed), 0, "the ask is taken");
    }

    #[test]
    fn the_rate_never_bends_past_its_bound() {
        let mut pace = Pace::default();
        for _ in 0..1_000 {
            let rate = pace.rate(48_000.0, 0.0, PACE_EVERY);
            assert!((rate - 1.0).abs() <= Pace::BOUND + f64::EPSILON, "{rate}");
        }
    }

    #[test]
    fn a_failed_stream_is_tried_again_later_each_time_and_at_once_after_it_ran() {
        let start = Instant::now();
        let mut retry = Retry::default();
        assert!(!retry.due(start), "put off when first seen down");
        assert!(retry.due(start + RETRY.wait(1)));
        assert!(!retry.due(start + RETRY.wait(1) + RETRY.wait(2) / 2));
        assert!(retry.due(start + RETRY.wait(1) + RETRY.wait(2)));
        retry.ran();
        assert!(!retry.due(start + Duration::from_secs(60)), "the wait starts over");
    }
}
