//! A share's sound, captured each system's own way (PipeWire's mix of the applications' outputs, WASAPI's process
//! loopback), cut into chunks and handed off the capture's thread through a ring that never blocks or allocates.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use crate::voice::{Health, Retry};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use self::linux::Listen;
#[cfg(target_os = "linux")]
use self::linux::Stream;
#[cfg(target_os = "linux")]
pub use self::linux::{App, Hears, holds_us};
#[cfg(windows)]
pub(crate) use self::windows::Listen;
#[cfg(windows)]
use self::windows::Stream;
#[cfg(windows)]
pub use self::windows::{App, Hears, holds_us};

/// The rate a share's sound is sent at, in samples per second per channel.
pub const RATE: u32 = 48_000;
/// Its channels, interleaved left then right.
pub const CHANNELS: usize = 2;
/// How long one chunk of a share's sound lasts.
pub const CHUNK_LENGTH: Duration = Duration::from_millis(10);
/// The samples of one chunk: [`CHUNK_LENGTH`] of both channels, the frame the engine's audio source takes.
pub const CHUNK: usize = RATE as usize * CHUNK_LENGTH.as_millis() as usize / 1000 * CHANNELS;

/// How many chunks may wait for the sender before the newest is dropped: a tenth of a second.
const KEPT: usize = (Duration::from_millis(100).as_millis() / CHUNK_LENGTH.as_millis()) as usize;

/// The chunks the capture made, oldest first: [`CHUNK_LENGTH`] of interleaved stereo at [`RATE`] each.
struct Chunks {
    ring: rtrb::Consumer<[i16; CHUNK]>,
}

/// Yields the chunks waiting now, and ends when none does; a later call yields what came since.
impl Iterator for Chunks {
    type Item = [i16; CHUNK];

    fn next(&mut self) -> Option<Self::Item> {
        self.ring.pop().ok()
    }
}

/// Cuts what a capture hands over, a quantum of any size, into exact chunks of `N` samples, in place.
pub(crate) struct Chunker<const N: usize, F> {
    samples: [i16; N],
    filled: usize,
    hear: F,
}

impl<const N: usize, F: FnMut(&[i16; N])> Chunker<N, F> {
    pub(crate) fn new(hear: F) -> Self {
        Self { samples: [0; N], filled: 0, hear }
    }

    /// Takes interleaved little-endian samples; every chunk filled is heard at once.
    fn push(&mut self, bytes: &[u8]) {
        let (pairs, _) = bytes.as_chunks::<2>();
        self.take(pairs.iter().map(|pair| i16::from_le_bytes(*pair)));
    }

    /// Takes samples; every chunk filled is heard at once.
    pub(crate) fn push_samples(&mut self, samples: &[i16]) {
        self.take(samples.iter().copied());
    }

    fn take(&mut self, samples: impl Iterator<Item = i16>) {
        for sample in samples {
            self.samples[self.filled] = sample;
            self.filled += 1;
            if self.filled == N {
                (self.hear)(&self.samples);
                self.filled = 0;
            }
        }
    }
}

/// The loudest a chunk may peak and still be silence, about -84 dBFS. Windows' loopback hands over ±1 while nothing
/// plays (Machine A: every chunk of 10 s peaked at 1), which Opus spent about 72 kbit/s on.
const QUIET_PEAK: u16 = 2;

/// A chunk of exact zeros: what the encoder's discontinuous transmission takes for silence.
const SILENCE: [i16; CHUNK] = [0; CHUNK];

/// How far behind its clock the feed may fall before a stall is let go instead of made up in zeros: the ring's own
/// length, since what waited longer was dropped.
const LATE: Duration = Duration::from_millis(100);

/// How long the capture may give nothing before the feed sends zeros in its place: past PipeWire's largest default
/// quantum (2048 frames, 43 ms), so a capture that runs is never padded between its quanta.
const QUIET_AFTER: Duration = Duration::from_millis(50);

/// How long a capture may stay ended, its reopening tried meanwhile, before it says so.
const STOPPED_AFTER: Duration = Duration::from_secs(2);

/// Whether a chunk is silence, which is sent as exact zeros so the encoder's DTX can rest.
fn is_quiet(chunk: &[i16; CHUNK]) -> bool {
    chunk.iter().all(|sample| sample.unsigned_abs() <= QUIET_PEAK)
}

/// The capture's side and the feed's side of one ring: the capture's side takes bytes and chunks them, and hands on
/// every chunk, a quiet one as zeros, then wakes `feed`. A full ring drops the chunk that would not fit.
fn hand_off(feed: Thread) -> (impl FnMut(&[u8]) + Send + 'static, Chunks) {
    let (mut producer, ring) = rtrb::RingBuffer::new(KEPT);
    let mut chunker = Chunker::new(move |chunk: &[i16; CHUNK]| {
        if producer.push(if is_quiet(chunk) { SILENCE } else { *chunk }).is_ok() {
            feed.unpark();
        }
    });
    (move |bytes: &[u8]| chunker.push(bytes), Chunks { ring })
}

/// The pace of the sound sent while the capture gives none: zeros every 10 ms from [`QUIET_AFTER`] after its last
/// chunk, so the engine's stream never has a gap a burst after it could be lost in. A capture that runs is handed on
/// as it comes and never padded, so its own clock, not this one, paces what is sent.
struct Feed {
    /// When the next chunk of zeros is due, if nothing comes before it.
    due: Instant,
}

impl Feed {
    fn new(now: Instant) -> Self {
        Self { due: now + QUIET_AFTER }
    }

    /// Hands `sink` every chunk waiting at `now`, or one of zeros if none waits and one is due.
    fn step(&mut self, now: Instant, chunks: &mut Chunks, sink: &mut impl FnMut(&[i16; CHUNK])) {
        let mut came = false;
        for chunk in chunks.by_ref() {
            sink(&chunk);
            came = true;
        }
        if came {
            self.due = now + QUIET_AFTER;
        } else if now >= self.due {
            sink(&SILENCE);
            self.due += CHUNK_LENGTH;
            if now.saturating_duration_since(self.due) > LATE {
                self.due = now + CHUNK_LENGTH;
            }
        }
    }
}

/// What the feed's thread and its owner share.
#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    /// Whether the capture has stayed ended past [`STOPPED_AFTER`].
    stopped: AtomicBool,
    /// How late the last chunk came after its capture, in microseconds.
    delay: AtomicU64,
    /// What to hear instead, taken at the feed's next turn.
    choice: Mutex<Option<Hears>>,
}

/// Since when a capture has been ended, as of `now`: none once it runs or waits for an application again.
fn past(health: Health, ended_at: Option<Instant>, now: Instant) -> Option<Instant> {
    match health {
        Health::Stopped => Some(ended_at.unwrap_or(now)),
        Health::Running | Health::Starting => None,
    }
}

/// The feed's thread: opens the capture and says whether it did, then, until stopped, reopens it when it ended, hands
/// every chunk to `sink` and fills what the capture did not give with zeros. It runs ahead of ordinary work, as a
/// call's voice does, and the capture's own thread only writes the ring and wakes it.
fn serve(
    first: Hears,
    mut sink: impl FnMut(&[i16; CHUNK]),
    shared: &Shared,
    opened: &mpsc::SyncSender<Result<(), SoundError>>,
) {
    let _ahead = crate::voice::realtime();
    let (hear, mut chunks) = hand_off(std::thread::current());
    let mut hears = first;
    let mut stream = match Stream::open(&hears, hear) {
        Ok(stream) => {
            let _ = opened.send(Ok(()));
            Some(stream)
        }
        Err(why) => {
            let _ = opened.send(Err(why));
            return;
        }
    };
    let (mut retry, mut ended_at) = (Retry::default(), None);
    let mut feed = Feed::new(Instant::now());
    while !shared.stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let chosen = shared.choice.try_lock().ok().and_then(|mut choice| choice.take());
        let health = stream.as_ref().map_or(Health::Stopped, Stream::health);
        let reopen = match (chosen, health) {
            (Some(chosen), _) => {
                hears = chosen;
                true
            }
            (None, Health::Stopped) => retry.due(now),
            (None, Health::Running) => {
                retry.ran();
                false
            }
            (None, Health::Starting) => false,
        };
        if reopen {
            // Dropped first: a Windows loopback ends with its thread, which the new one must not outlive.
            drop(stream.take());
            let (hear, fresh) = hand_off(std::thread::current());
            stream = Stream::open(&hears, hear).ok();
            chunks = fresh;
        }
        ended_at = past(stream.as_ref().map_or(Health::Stopped, Stream::health), ended_at, now);
        shared.stopped.store(ended_at.is_some_and(|at| now.duration_since(at) >= STOPPED_AFTER), Ordering::Relaxed);
        let late = stream.as_ref().map_or(Duration::ZERO, Stream::delay);
        shared.delay.store(u64::try_from(late.as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
        feed.step(now, &mut chunks, &mut sink);
        std::thread::park_timeout(feed.due.saturating_duration_since(Instant::now()));
    }
}

/// The share's sound being heard and handed to a sink, kept: a capture the sound server ended (a restart, a device
/// gone) is opened again after a growing wait, as a call's microphone is. The sink gets each chunk the capture
/// makes, and zeros every 10 ms while it makes none, from a real-time thread of this capture's own. Dropping it stops the capture and the thread.
pub struct Capture {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Hears what `hears` names, each chunk handed to `sink` on the feed's thread, which must not block. Refused,
    /// nothing is heard.
    pub fn open(hears: Hears, sink: impl FnMut(&[i16; CHUNK]) + Send + 'static) -> Result<Self, SoundError> {
        let shared = Arc::new(Shared::default());
        let (opened, said) = mpsc::sync_channel(1);
        let own = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("chatkord-share-feed".into())
            .spawn(move || serve(hears, sink, &own, &opened))
            .map_err(|_| SoundError::Stream)?;
        match said.recv() {
            Ok(Ok(())) => Ok(Self { shared, thread: Some(thread) }),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => Err(SoundError::Stream),
        }
    }

    /// Hears what `hears` names from now on instead.
    pub fn choose(&self, hears: Hears) {
        *self.shared.choice.lock().unwrap_or_else(PoisonError::into_inner) = Some(hears);
        self.wake();
    }

    /// Whether the capture has stayed ended past its wait, its reopening tried meanwhile.
    pub fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Relaxed)
    }

    /// How late its sound reaches this client after it was captured, by the system's own clock: a chunk pushed now
    /// plays that much after a picture stamped at its capture. Zero with no capture.
    pub fn delay(&self) -> Duration {
        Duration::from_micros(self.shared.delay.load(Ordering::Relaxed))
    }

    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Why a share's sound did not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// This system cannot capture it: no PipeWire on Linux, a Windows older than its process loopback.
    Unavailable,
    /// The capture stream would not open.
    Stream,
}

impl fmt::Display for SoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("this system has no capture for a share's sound"),
            Self::Stream => f.write_str("the share's capture stream did not open"),
        }
    }
}

impl std::error::Error for SoundError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: impl IntoIterator<Item = i16>) -> Vec<u8> {
        samples.into_iter().flat_map(i16::to_le_bytes).collect()
    }

    #[test]
    fn quanta_of_any_size_come_out_as_exact_chunks_in_order() {
        let mut heard = Vec::new();
        let all: Vec<i16> = (0..i16::try_from(CHUNK * 3 + 17).expect("a few thousand samples")).collect();
        {
            let mut chunker = Chunker::new(|chunk: &[i16; CHUNK]| heard.push(chunk.to_vec()));
            for quantum in bytes(all.iter().copied()).chunks(1024 * 4 - 6) {
                chunker.push(quantum);
            }
        }
        assert_eq!(heard.len(), 3);
        assert_eq!(heard.concat(), all[..CHUNK * 3]);
    }

    #[test]
    fn a_chunk_is_ten_milliseconds_of_both_channels() {
        assert_eq!(CHUNK, 960);
    }

    #[test]
    fn chunks_come_out_in_order_and_a_full_ring_drops_the_newest() {
        let (mut hear, mut chunks) = hand_off(std::thread::current());
        // Marks well above silence, so each one is handed on as it is.
        let first = 1000;
        let after = first + i16::try_from(KEPT).expect("small");
        for mark in first..after + 3 {
            hear(&bytes([mark; CHUNK]));
        }
        let taken: Vec<i16> = chunks.by_ref().map(|chunk| chunk[0]).collect();
        let kept: Vec<i16> = (first..after).collect();
        assert_eq!(taken, kept, "the first {KEPT} in order, the rest dropped");

        hear(&bytes([after; CHUNK]));
        assert_eq!(chunks.next().map(|chunk| chunk[0]), Some(after), "room again once taken");
        assert_eq!(chunks.next(), None);
    }

    #[test]
    fn silence_and_windows_dither_are_sent_as_zeros() {
        let (mut hear, mut chunks) = hand_off(std::thread::current());
        // What Windows hands over while nothing plays: ±1 here and there.
        hear(&bytes((0..CHUNK).map(|i| [0, 1, 0, -1][i % 4])));
        hear(&bytes([0; CHUNK]));
        assert_eq!(chunks.next(), Some(SILENCE), "dither is exact zeros, and handed on");
        assert_eq!(chunks.next(), Some(SILENCE), "silence is handed on too");
        assert_eq!(chunks.next(), None);
        // A tone of amplitude 100, about -50 dBFS: quiet, and still sound.
        #[expect(clippy::cast_possible_truncation, reason = "an amplitude of 100")]
        let tone: Vec<i16> =
            (0..CHUNK).map(|i| (100.0 * (std::f64::consts::TAU * 1234.0 * i as f64 / 96_000.0).sin()) as i16).collect();
        hear(&bytes(tone.iter().copied()));
        assert_eq!(chunks.next().map(|chunk| chunk.to_vec()), Some(tone), "a quiet tone is sent whole");
        // One sample past the bound is sound.
        hear(&bytes((0..CHUNK).map(|i| if i == 7 { 3 } else { 0 })));
        assert_ne!(chunks.next(), Some(SILENCE), "a chunk louder than silence anywhere in it is sent as it is");
    }

    /// A feed started at `start` with a ring it can be given chunks through.
    fn fed(start: Instant) -> (Feed, impl FnMut(&[u8]), Chunks) {
        let (hear, chunks) = hand_off(std::thread::current());
        (Feed::new(start), hear, chunks)
    }

    /// What `feed` hands on at `at`.
    fn stepped(feed: &mut Feed, at: Instant, chunks: &mut Chunks) -> Vec<[i16; CHUNK]> {
        let mut sent = Vec::new();
        feed.step(at, chunks, &mut |chunk: &[i16; CHUNK]| sent.push(*chunk));
        sent
    }

    #[test]
    fn a_capture_silent_past_its_bound_is_padded_with_one_zero_chunk_each_ten_milliseconds() {
        let start = Instant::now();
        let (mut feed, _hear, mut chunks) = fed(start);
        let before = stepped(&mut feed, start + QUIET_AFTER - Duration::from_millis(1), &mut chunks);
        assert!(before.is_empty(), "before the bound nothing is made up");
        assert_eq!(stepped(&mut feed, start + QUIET_AFTER, &mut chunks), [SILENCE], "one zero chunk at the bound");
        assert!(stepped(&mut feed, start + QUIET_AFTER, &mut chunks).is_empty(), "and none again in the same instant");
        let early = start + QUIET_AFTER + CHUNK_LENGTH - Duration::from_millis(1);
        assert!(stepped(&mut feed, early, &mut chunks).is_empty(), "not before the next ten milliseconds");
        assert_eq!(stepped(&mut feed, start + QUIET_AFTER + CHUNK_LENGTH, &mut chunks), [SILENCE], "then the next");
    }

    #[test]
    fn a_capture_that_runs_between_its_quanta_is_never_padded() {
        let start = Instant::now();
        let (mut feed, mut hear, mut chunks) = fed(start);
        let mut sent = Vec::new();
        // A quantum of 2048 frames every 43 ms: four or five chunks at once, then nothing for the rest of it.
        let quantum = Duration::from_micros(42_667);
        for round in 1..=30_u32 {
            hear(&bytes(std::iter::repeat_n(500, 2048 * CHANNELS)));
            let at = start + quantum * round;
            sent.extend(stepped(&mut feed, at, &mut chunks));
            sent.extend(stepped(&mut feed, at + quantum - Duration::from_millis(1), &mut chunks));
        }
        assert!(!sent.is_empty() && sent.iter().all(|chunk| chunk[0] == 500), "only the capture's own chunks");
        let after = stepped(&mut feed, start + quantum * 30 + QUIET_AFTER, &mut chunks);
        assert_eq!(after, [SILENCE], "zeros once it gave nothing for the bound");
    }

    #[test]
    fn a_stall_is_let_go_and_not_made_up_in_zeros() {
        let start = Instant::now();
        let (mut feed, _hear, mut chunks) = fed(start);
        let mut count = 0;
        let mut sink = |_: &[i16; CHUNK]| count += 1;
        let resumed = start + Duration::from_secs(60);
        feed.step(resumed, &mut chunks, &mut sink);
        feed.step(resumed, &mut chunks, &mut sink);
        assert_eq!(count, 1, "a minute gone is one chunk, not six thousand");
    }

    #[test]
    fn a_capture_is_told_ended_from_when_it_first_ended_until_it_runs_or_waits_again() {
        let start = Instant::now();
        let later = start + STOPPED_AFTER;
        assert_eq!(past(Health::Stopped, None, start), Some(start), "ended now");
        assert_eq!(past(Health::Stopped, Some(start), later), Some(start), "still since then");
        assert_eq!(past(Health::Running, Some(start), later), None, "back");
        assert_eq!(past(Health::Starting, Some(start), later), None, "waiting for an application is not ended");
    }

    mod lab {
        use super::*;

        /// The feed's thread must run ahead of ordinary work and never on a runtime's worker, whatever thread asked
        /// for the capture.
        #[tokio::test]
        async fn the_feed_runs_real_time_off_any_tokio_worker() {
            #[cfg(target_os = "linux")]
            let hears = Hears::Apps(std::collections::HashSet::new());
            #[cfg(windows)]
            let hears = Hears::AllButOurs;
            let (told, heard) = mpsc::channel();
            let opened = Capture::open(hears, move |_| {
                let _ = told.send((crate::voice::ahead(), tokio::runtime::Handle::try_current().is_ok()));
            });
            let Ok(capture) = opened else { return tier::skipped("the system has no capture of a share's sound") };
            // Nothing plays, so the first chunk is the feed's own zeros: the ones the deadline makes.
            let (ahead, on_a_runtime) = heard.recv_timeout(Duration::from_secs(3)).expect("the feed hands on a chunk");
            drop(capture);
            assert!(!on_a_runtime, "the feed is a thread of its own, not a task of a runtime's");
            if !ahead {
                tier::skipped("this session is granted no real-time priority (no portal or RTKit, or a remote one)");
            }
        }
    }
}
