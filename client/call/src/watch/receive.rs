//! A watched screen from the engine to the window's slot: the engine's thread only queues, a thread of ours decodes.
//! Behind a lost frame nothing decodes until a key frame, so the queue drops to the next, asked of the sender.

use crate::watch::behind::Behind;
use crate::watch::fallback::{Fallback, Step, Verdict};
use crate::watch::freeze::Freezes;
use crate::watch::trial::{Feed, Trial};
use choices::Codec;
use media::{DecodeError, Decoder, Path, Picture, Shown};
use picture::{Painting, Slot};
use screens::{ShareCard, ShareState};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// Encoded frames waiting for the decoder: about 130 ms of a 60 a second stream.
pub(super) const QUEUED: usize = 8;

/// Key frames in a row the path in use may refuse, after it showed pictures, before it is taken for broken: one is a
/// frame damaged on its way, which the next mends.
const KEYS_REFUSED: u32 = 2;

struct Unit {
    data: media::Coded,
    pts: i64,
    key: bool,
}

/// The frames waiting, and whether the stream is usable: after a frame was lost nothing decodes until a key frame.
struct Backlog {
    queue: VecDeque<Unit>,
    /// Frames that never reached the decoder because a key frame was needed first or the queue was full.
    dropped: u64,
    awaiting_key: bool,
    /// The trial of another path, offered every unit as it comes, while one runs.
    trial: Option<Feed>,
}

impl Backlog {
    /// A stream starts at a key frame, the first one asked for.
    fn new() -> Self {
        Self { queue: VecDeque::new(), dropped: 0, awaiting_key: true, trial: None }
    }

    /// Whether a key frame is wanted: the stream waits for one, or the trial running waits for its first.
    fn awaits_key(&self) -> bool {
        self.awaiting_key || self.trial.as_ref().is_some_and(Feed::awaits_key)
    }

    /// Queue `unit`. True when the stream just became one that needs a key frame the sender must be asked for.
    fn push(&mut self, unit: Unit) -> bool {
        if let Some(trial) = &mut self.trial {
            trial.offer(&unit.data, unit.pts, unit.key);
        }
        if self.awaiting_key && !unit.key {
            self.dropped += 1;
            return false;
        }
        if unit.key {
            // A key frame starts the stream afresh: whatever waits behind the last one is stale.
            self.dropped += self.queue.len() as u64;
            self.queue.clear();
            self.awaiting_key = false;
        } else if self.queue.len() >= QUEUED {
            self.dropped += self.queue.len() as u64 + 1;
            self.queue.clear();
            self.awaiting_key = true;
            return true;
        }
        self.queue.push_back(unit);
        false
    }

    /// A frame could not be kept: what follows it is undecodable until a key frame. True when the stream just became
    /// one that needs a key frame the sender must be asked for.
    fn lost(&mut self) -> bool {
        if let Some(trial) = &self.trial {
            trial.lost();
        }
        self.dropped += 1;
        let asked = !self.awaiting_key;
        self.broken();
        asked
    }

    /// A frame did not decode: what follows it is undecodable too.
    fn broken(&mut self) {
        self.dropped += self.queue.len() as u64;
        self.queue.clear();
        self.awaiting_key = true;
    }

    /// The decoder is closed or opened afresh: what waits was for the one before, and the stream starts at a key frame.
    fn restarted(&mut self) {
        self.queue.clear();
        self.awaiting_key = true;
    }
}

/// What the decoding thread has done, read by the status line.
#[derive(Default)]
struct Counts {
    decoded: AtomicU64,
    nanos: AtomicU64,
    failed: AtomicU64,
    /// The pictures go to the window as surfaces on the GPU, not as copies.
    on_gpu: AtomicBool,
    /// The size of the last picture, width in the high half.
    size: AtomicU64,
    /// The freezes so far ([`Freezes`]), and the longest in milliseconds.
    freezes: AtomicU64,
    longest_freeze_ms: AtomicU64,
    /// The decoder takes longer than the stream's own time ([`Behind`]).
    behind: AtomicBool,
}

struct Shared {
    backlog: Mutex<Backlog>,
    ready: Condvar,
    counts: Counts,
    /// The freezes so far, kept here so a decoder opened again goes on from them.
    freezes: Mutex<Freezes>,
    key_frame: Notify,
    /// Told once, at the first picture decoded: the window shows a card from then on.
    first_picture: Notify,
    /// Told when a held stream's decoder opened again, or did not: its stream may be started at the server.
    reopened: Notify,
    /// The path decoding moved to on the decode thread, once it moved from the one it opened on.
    moved: Mutex<Option<Path>>,
    /// The path a broken one handed over to, until it is told.
    handed_over: Mutex<Option<Path>>,
    /// The stream was held and is taken again, until the decode thread hears of it.
    resumed: AtomicBool,
    /// A held stream's decoder did not open again, until the call is told.
    unopened: AtomicBool,
}

impl Shared {
    fn backlog(&self) -> MutexGuard<'_, Backlog> {
        held(&self.backlog)
    }
}

/// The value a lock holds, whether or not a thread panicked holding it.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What a status line says of the decoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    /// The codec the frames are in.
    pub codec: Codec,
    /// Where the frames are decoded.
    pub path: Path,
    /// Whether the pictures reach the window as surfaces on the GPU, not as copies.
    pub on_gpu: bool,
    /// Pictures decoded so far.
    pub decoded: u64,
    /// Time spent decoding them.
    pub seconds: f64,
    /// Frames that never reached the decoder: a key frame was needed first, or the queue was full.
    pub dropped: u64,
    /// Frames the decoder refused.
    pub failed: u64,
    /// The size of the last picture put, once there is one.
    pub size: Option<(u32, u32)>,
    /// How often the picture froze: a long gap in which frames were lost or refused.
    pub freezes: u64,
    /// The longest freeze.
    pub longest_freeze: Duration,
    /// The decoder takes longer than the stream's own time: this computer cannot keep up with it.
    pub behind: bool,
}

/// Where a watched screen's frames wait for its decoder, fed from the engine's thread; it takes them before the
/// decoder opens, since the codec is known only once the screen is taken.
#[derive(Clone)]
pub struct Intake {
    shared: Arc<Shared>,
}

impl Intake {
    /// An empty queue, which asks for a key frame first.
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            backlog: Mutex::new(Backlog::new()),
            ready: Condvar::new(),
            counts: Counts::default(),
            freezes: Mutex::new(Freezes::default()),
            key_frame: Notify::new(),
            first_picture: Notify::new(),
            reopened: Notify::new(),
            moved: Mutex::new(None),
            handed_over: Mutex::new(None),
            resumed: AtomicBool::new(false),
            unopened: AtomicBool::new(false),
        });
        // The first thing a stream needs is a key frame, and the sender may not send one for seconds.
        shared.key_frame.notify_one();
        Self { shared }
    }

    /// Queue an encoded frame; called on the engine's thread, so it never waits on the decoder.
    pub fn feed(&self, data: &[u8], render_time_us: i64, key: bool) {
        // The one copy a received frame takes: the engine's buffer is lent only for this call.
        let asked = match media::Coded::copied(data) {
            Ok(data) => self.shared.backlog().push(Unit { data, pts: render_time_us, key }),
            Err(_) => self.shared.backlog().lost(),
        };
        self.shared.ready.notify_one();
        if asked {
            self.shared.key_frame.notify_one();
        }
    }
}

impl Default for Intake {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a stream's decoding did not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No decoder opened for the stream.
    Decoder(DecodeError),
    /// The thread that decodes could not be started.
    Thread,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decoder(why) => why.fmt(f),
            Self::Thread => f.write_str("the thread that decodes could not be started"),
        }
    }
}

impl std::error::Error for StartError {}

/// Starts the thread that decodes, running `body`.
type Spawn = fn(Box<dyn FnOnce() + Send>) -> std::io::Result<JoinHandle<()>>;

/// A watched screen's decoder, decoding what its [`Intake`] queued; dropping it stops the decoding. A stream held at
/// the server has no decoder: [`Pipeline::hold`] closes it, and [`Pipeline::resume`] opens one again.
pub struct Pipeline {
    shared: Arc<Shared>,
    codec: Codec,
    path: Path,
    /// Where a decoder opened again looks, as the first one did.
    sources: media::Sources,
    slot: Slot,
    run: Mutex<Run>,
}

/// The thread that decodes while a decoder is open.
struct Run {
    thread: Option<JoinHandle<()>>,
    /// Ends that thread; each decoder has its own, so one opened again is never stopped by the one it follows.
    stop: Arc<AtomicBool>,
    open: bool,
}

impl Pipeline {
    /// Opens `codec`'s decoder on `painting`, the GPU the window paints with, so its pictures are painted where they
    /// are; the decoder tries it first, then the system's own device, then software. It decodes what `intake` queued.
    pub fn start(intake: &Intake, codec: Codec, painting: &Painting, slot: Slot) -> Result<Self, StartError> {
        Self::start_with(intake, codec, painting.sources(), slot)
    }

    /// As [`Pipeline::start`], the decoder looking where `sources` say, each in turn and software last.
    pub fn start_with(intake: &Intake, codec: Codec, sources: media::Sources, slot: Slot) -> Result<Self, StartError> {
        Self::start_on(intake, codec, sources, slot, |body| {
            std::thread::Builder::new().name("decode".to_string()).spawn(body)
        })
    }

    fn start_on(
        intake: &Intake,
        codec: Codec,
        sources: media::Sources,
        slot: Slot,
        spawn: Spawn,
    ) -> Result<Self, StartError> {
        let decoder = Decoder::open(crate::codec::encoded(codec), &sources).map_err(StartError::Decoder)?;
        let path = decoder.path();
        let shared = Arc::clone(&intake.shared);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, slot, stop) = (Arc::clone(&shared), slot.clone(), Arc::clone(&stop));
            let sources = sources.clone();
            spawn(Box::new(move || decode(decoder, codec, &sources, &slot, &shared, &stop)))
                .map_err(|_| StartError::Thread)?
        };
        let run = Mutex::new(Run { thread: Some(thread), stop, open: true });
        Ok(Self { shared, codec, path, sources, slot, run })
    }

    /// Closes the decoder, so a stream held at the server holds nothing on the GPU: the thread frees it as it ends.
    /// What waited for it is dropped, and the stream waits for a key frame, which nothing asks for until it resumes.
    pub fn hold(&self) {
        let mut run = held(&self.run);
        if !std::mem::take(&mut run.open) {
            return;
        }
        run.stop.store(true, Ordering::Relaxed);
        self.shared.ready.notify_all();
        self.shared.backlog().restarted();
    }

    /// Whether a decoder is open: false from [`Pipeline::hold`] until [`Pipeline::resume`].
    pub fn is_open(&self) -> bool {
        held(&self.run).open
    }

    /// Whether the thread that decoded has ended and freed its decoder, after [`Pipeline::hold`].
    pub fn is_freed(&self) -> bool {
        let run = held(&self.run);
        !run.open && run.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Its stream was held and is to be taken again: a decoder opens, told by [`Pipeline::reopened`], the gap the
    /// hold leaves is no freeze, and the stream waits for a key frame, which the server's start of it asks of the
    /// sender, so this wants none of its own. The decoder opens on the thread that decodes, which first waits for the
    /// one before it to end, so nothing here blocks.
    pub fn resume(&self) {
        self.shared.resumed.store(true, Ordering::Relaxed);
        let mut run = held(&self.run);
        let alive = run.open && run.thread.as_ref().is_some_and(|thread| !thread.is_finished());
        run.open = true;
        if alive {
            self.shared.reopened.notify_one();
            return;
        }
        self.shared.backlog().restarted();
        let stop = Arc::new(AtomicBool::new(false));
        let (codec, sources, slot) = (self.codec, self.sources.clone(), self.slot.clone());
        let (shared, flag, previous) = (Arc::clone(&self.shared), Arc::clone(&stop), run.thread.take());
        let began = std::thread::Builder::new().name("decode".to_string()).spawn(move || {
            if let Some(previous) = previous {
                let _ = previous.join();
            }
            reopen(codec, &sources, &slot, &shared, &flag);
        });
        match began {
            Ok(thread) => {
                run.thread = Some(thread);
                run.stop = stop;
            }
            Err(_) => {
                run.open = false;
                self.shared.counts.failed.fetch_add(1, Ordering::Relaxed);
                self.shared.unopened.store(true, Ordering::Relaxed);
                self.shared.reopened.notify_one();
                report::note!(warn, "the thread of a held stream's decoder did not start again");
            }
        }
    }

    /// Whether the stream waits for a key frame: before its first, after a frame was lost or did not decode, and while
    /// a trial of another path waits for its first.
    pub fn awaiting_key(&self) -> bool {
        self.shared.backlog().awaits_key()
    }

    /// Whether this computer cannot keep up with the stream: see [`Report::behind`].
    pub fn behind(&self) -> bool {
        self.shared.counts.behind.load(Ordering::Relaxed)
    }

    /// Resolves when the sender should be asked for a key frame.
    pub async fn wants_key_frame(&self) {
        self.shared.key_frame.notified().await;
    }

    /// Resolves once its first picture was decoded.
    pub async fn first_picture(&self) {
        self.shared.first_picture.notified().await;
    }

    /// Resolves once the decoder [`Pipeline::resume`] opens has opened, or did not.
    pub async fn reopened(&self) {
        self.shared.reopened.notified().await;
    }

    /// The path a decoder that broke after showing pictures handed over to, once: a person is told it once.
    pub fn handed_over(&self) -> Option<Path> {
        held(&self.shared.handed_over).take()
    }

    /// Whether a decoder did not open again for the stream taken after a hold, once: it stays dark until it resumes.
    pub fn unopened(&self) -> bool {
        self.shared.unopened.swap(false, Ordering::Relaxed)
    }

    /// What the decoding has done so far.
    pub fn report(&self) -> Report {
        let counts = &self.shared.counts;
        Report {
            codec: self.codec,
            path: held(&self.shared.moved).unwrap_or(self.path),
            on_gpu: counts.on_gpu.load(Ordering::Relaxed),
            decoded: counts.decoded.load(Ordering::Relaxed),
            seconds: counts.nanos.load(Ordering::Relaxed) as f64 / 1e9,
            dropped: self.shared.backlog().dropped,
            failed: counts.failed.load(Ordering::Relaxed),
            size: {
                let packed = counts.size.load(Ordering::Relaxed);
                let [w0, w1, w2, w3, h0, h1, h2, h3] = packed.to_be_bytes();
                (packed != 0).then_some((u32::from_be_bytes([w0, w1, w2, w3]), u32::from_be_bytes([h0, h1, h2, h3])))
            },
            freezes: counts.freezes.load(Ordering::Relaxed),
            longest_freeze: Duration::from_millis(counts.longest_freeze_ms.load(Ordering::Relaxed)),
            behind: counts.behind.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let run = self.run.get_mut().unwrap_or_else(|poisoned| poisoned.into_inner());
        run.stop.store(true, Ordering::Relaxed);
        self.shared.ready.notify_all();
        if let Some(thread) = run.thread.take() {
            let _ = thread.join();
        }
    }
}

fn next(shared: &Shared, stop: &AtomicBool) -> Option<Unit> {
    let mut backlog = shared.backlog();
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(unit) = backlog.queue.pop_front() {
            return Some(unit);
        }
        backlog = shared.ready.wait(backlog).unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

/// Opens the decoder of a stream taken again after a hold, on the thread that decodes, tells [`Pipeline::reopened`],
/// and decodes with it; one that does not open is counted among the frames refused and told once, and the stream stays
/// dark, for the next resume to try again.
fn reopen(codec: Codec, sources: &media::Sources, slot: &Slot, shared: &Shared, stop: &AtomicBool) {
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let opened = Decoder::open(crate::codec::encoded(codec), sources);
    shared.reopened.notify_one();
    match opened {
        Ok(decoder) => decode(decoder, codec, sources, slot, shared, stop),
        Err(_) => {
            shared.counts.failed.fetch_add(1, Ordering::Relaxed);
            shared.unopened.store(true, Ordering::Relaxed);
            report::note!(warn, "a held stream's decoder did not open again");
        }
    }
}

/// Opens a trial of `codec` on `to`, this stream's alone, offered the stream's units from now and decoding as the
/// window takes pictures; the sender is asked for the key frame it starts from. None where it did not open or start.
fn try_on(to: Path, codec: Codec, sources: &media::Sources, slot: &Slot, shared: &Shared) -> Option<Trial> {
    let decoder = Decoder::open_on(crate::codec::encoded(codec), to, sources).ok()?;
    let (trial, feed) = Trial::start(decoder, slot.takes_imports())?;
    shared.backlog().trial = Some(feed);
    shared.key_frame.notify_one();
    Some(trial)
}

/// Ends a trial, which lets go of its decoder on its own thread.
fn drop_trial(trial: &mut Option<Trial>, shared: &Shared) {
    shared.backlog().trial = None;
    *trial = None;
}

/// Puts the decoder of `trial`, which kept up, in the place of `decoder`. What waits in the queue was offered to the
/// trial too, which decodes it before it gives its decoder back, so the stream goes on with no key frame. False where
/// it lagged at the last unit or its thread failed, `decoder` staying, and asking for a key frame if it was let go.
fn take_over(trial: Trial, decoder: &mut Decoder, shared: &Shared) -> bool {
    {
        let mut backlog = shared.backlog();
        backlog.trial = None;
        // Read under the queue's lock, which every offer takes: no unit can have missed the trial after this.
        if trial.verdict() != Verdict::KeepsUp {
            return false;
        }
        backlog.queue.clear();
        backlog.awaiting_key = false;
    }
    match trial.finish() {
        Some(taken) => {
            *decoder = taken;
            true
        }
        None => {
            shared.backlog().broken();
            shared.key_frame.notify_one();
            false
        }
    }
}

fn decode(
    mut decoder: Decoder,
    codec: Codec,
    sources: &media::Sources,
    slot: &Slot,
    shared: &Shared,
    stop: &AtomicBool,
) {
    let mut path = decoder.path();
    // Published at once, so the card names the path a decoder opened again took.
    *held(&shared.moved) = Some(path);
    let mut refused_keys = 0;
    let mut freezes = std::mem::take(&mut *held(&shared.freezes));
    let mut behind = Behind::default();
    shared.counts.behind.store(false, Ordering::Relaxed);
    let (mut fallback, mut trial) = (Fallback::new(path), None::<Trial>);
    let lost = || shared.backlog().dropped + shared.counts.failed.load(Ordering::Relaxed);
    let mut lost_at_last = lost();
    // A watched screen's first picture, each step logged once: its time is read from the log's own stamps.
    let (mut first_in, mut first_out) = (true, true);
    while let Some(unit) = next(shared, stop) {
        if std::mem::take(&mut first_in) {
            report::note!(info, "watched screen: first unit in", bytes = unit.data.len(), key_frame = unit.key);
        }
        let started = Instant::now();
        let decoded = decode_one(&mut decoder, slot, &unit, &shared.counts);
        if decoded.as_ref().is_ok_and(|pictures| *pictures > 0) && std::mem::take(&mut first_out) {
            report::note!(info, "watched screen: first picture decoded", path = decoder.path());
            shared.first_picture.notify_one();
        }
        if decoded.is_err() {
            refused_keys += u32::from(unit.key);
            // Handed over, the stream starts again at the next key frame, on the next path.
            if refused_keys >= KEYS_REFUSED
                && let Some(next) = decoder.hand_over()
            {
                refused_keys = 0;
                *held(&shared.handed_over) = Some(next);
            }
        }
        // Where it decodes is published before what it did, so whoever reads the counts reads the path with them.
        if decoder.path() != path {
            path = decoder.path();
            *held(&shared.moved) = Some(path);
            if fallback.moved(path, Instant::now()) == Step::Drop {
                drop_trial(&mut trial, shared);
            }
        }
        match decoded {
            Ok(pictures) => {
                if pictures > 0 {
                    refused_keys = 0;
                    let late = behind.decoded(unit.pts, started.elapsed());
                    shared.counts.behind.store(late, Ordering::Relaxed);
                    let at = Instant::now();
                    let mut step = fallback.painted(at, late);
                    if let (Step::Stay, Some(running)) = (step, &trial) {
                        step = fallback.tried(at, running.verdict());
                    }
                    match step {
                        Step::Stay => {}
                        Step::Try(to) => {
                            trial = try_on(to, codec, sources, slot, shared);
                            if trial.is_none() {
                                fallback.failed(at);
                            }
                        }
                        Step::Drop => drop_trial(&mut trial, shared),
                        Step::Take => {
                            let from = decoder.path();
                            if trial.take().is_some_and(|running| take_over(running, &mut decoder, shared)) {
                                (behind, refused_keys, path) = (Behind::default(), 0, decoder.path());
                                *held(&shared.moved) = Some(path);
                                if path == Path::Software {
                                    report::note!(
                                        warn,
                                        "a watched screen's graphics decoder could not keep up; software did",
                                        from = from
                                    );
                                    *held(&shared.handed_over) = Some(path);
                                } else {
                                    report::note!(
                                        info,
                                        "a watched screen decodes on its graphics card again",
                                        on = path
                                    );
                                }
                            } else {
                                fallback.moved(from, at);
                            }
                        }
                    }
                    let now = lost();
                    if shared.resumed.swap(false, Ordering::Relaxed) {
                        freezes.resumed();
                    }
                    freezes.picture(Instant::now(), now > lost_at_last);
                    lost_at_last = now;
                    let (count, longest) = freezes.so_far();
                    shared.counts.freezes.store(count, Ordering::Relaxed);
                    let longest = u64::try_from(longest.as_millis()).unwrap_or(u64::MAX);
                    shared.counts.longest_freeze_ms.store(longest, Ordering::Relaxed);
                }
                shared.counts.on_gpu.store(path.leaves_on_gpu() && slot.takes_imports(), Ordering::Relaxed);
                shared
                    .counts
                    .nanos
                    .fetch_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
                shared.counts.decoded.fetch_add(pictures, Ordering::Relaxed);
            }
            Err(_) => {
                shared.backlog().broken();
                shared.key_frame.notify_one();
                shared.counts.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    // A trial lets go of its decoder before this thread ends, so a held stream holds nothing on the GPU.
    shared.backlog().trial = None;
    if let Some(running) = trial {
        let _ = running.finish();
    }
    *held(&shared.freezes) = freezes;
}

fn pack(width: u32, height: u32) -> u64 {
    u64::from(width) << 32 | u64::from(height)
}

impl Report {
    /// The card of the share being watched: `rate` the pictures a second over the last interval, `dropped` those the
    /// viewer had not taken when a newer came. None until a picture was put, since the card is about its size (so a
    /// route lost before the first picture is not told).
    pub fn card(&self, who: &terms::MemberId, rate: Option<u32>, dropped: u64, state: ShareState) -> Option<ShareCard> {
        let (width, height) = self.size?;
        Some(ShareCard {
            who: who.clone(),
            width,
            height,
            frame_rate: rate,
            dropped,
            codec: self.codec.label().to_string(),
            decoder: crate::codec::route(self.path),
            refused: self.failed,
            freezes: (self.freezes, self.longest_freeze),
            state,
        })
    }
}

/// Decode one unit into the slot, as surfaces while the window takes them and the path leaves them on the GPU, and in
/// memory, by reference, otherwise. Pictures put.
fn decode_one(decoder: &mut Decoder, slot: &Slot, unit: &Unit, counts: &Counts) -> Result<u64, DecodeError> {
    if !slot.takes_imports() {
        let pictures = decoder.decode_in_memory(&unit.data, unit.pts)?;
        let count = pictures.len() as u64;
        pictures.into_iter().for_each(|picture| put_in_memory(slot, picture, counts));
        return Ok(count);
    }
    let shown = decoder.decode_shown(&unit.data, unit.pts)?;
    let count = shown.len() as u64;
    for picture in shown {
        match picture {
            Shown::OnGpu(surface) => {
                counts.size.store(pack(surface.width(), surface.height()), Ordering::Relaxed);
                #[cfg(target_os = "linux")]
                slot.put_surface(surface);
                #[cfg(windows)]
                slot.put_d3d12(surface);
            }
            Shown::InMemory(picture) => put_in_memory(slot, picture, counts),
        }
    }
    Ok(count)
}

fn put_in_memory(slot: &Slot, picture: Picture, counts: &Counts) {
    let (width, height) = picture.size();
    counts.size.store(pack(width, height), Ordering::Relaxed);
    slot.put_picture(picture);
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::Backend;
    use screens::Route;

    fn ada() -> terms::MemberId {
        terms::MemberId::parse("Ada").expect("a member id")
    }

    fn unit(key: bool) -> Unit {
        Unit { data: media::Coded::copied(&[0]).expect("a unit"), pts: 0, key }
    }

    #[test]
    fn a_decode_thread_that_does_not_start_is_an_error_not_a_stream_nobody_decodes() {
        let refused: Spawn = |_| Err(std::io::ErrorKind::OutOfMemory.into());
        let started =
            Pipeline::start_on(&Intake::new(), Codec::H264, Painting::default().sources(), Slot::new(), refused);
        assert_eq!(started.err(), Some(StartError::Thread));
    }

    #[test]
    fn a_held_stream_closes_its_decoder_and_a_resumed_one_opens_again_awaiting_the_key_frame_the_resume_asked() {
        let intake = Intake::new();
        let pipeline =
            Pipeline::start_with(&intake, Codec::H264, media::Sources::default(), Slot::new()).expect("software opens");
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("a runtime");
        let asked =
            |within| runtime.block_on(async { tokio::time::timeout(within, pipeline.wants_key_frame()).await.is_ok() });
        assert!(asked(Duration::from_secs(1)), "a stream starts by asking for a key frame");
        assert!(pipeline.is_open() && !pipeline.is_freed());

        pipeline.hold();
        assert!(!pipeline.is_open());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pipeline.is_freed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(pipeline.is_freed(), "the thread that decoded ended, and freed its decoder with it");
        assert!(pipeline.awaiting_key(), "what a held stream shows next is a key frame");
        assert!(!asked(Duration::from_millis(100)), "a held stream asks for nothing");

        pipeline.resume();
        assert!(pipeline.is_open() && !pipeline.is_freed(), "a decoder opens again");
        let reopened = runtime
            .block_on(async { tokio::time::timeout(Duration::from_secs(10), pipeline.reopened()).await.is_ok() });
        assert!(reopened, "and says it opened, for the stream to be started at the server");
        assert!(pipeline.awaiting_key(), "it waits for a key frame");
        assert!(!asked(Duration::from_millis(300)), "which the server's start asks of the sender, not this");
        let again = pipeline.report().path;
        assert_eq!(again, Path::Software, "it opened where the first one did");
    }

    #[test]
    fn a_stream_resumed_while_its_decoder_is_open_changes_nothing() {
        let intake = Intake::new();
        let pipeline =
            Pipeline::start_with(&intake, Codec::H264, media::Sources::default(), Slot::new()).expect("software opens");
        pipeline.resume();
        assert!(pipeline.is_open() && !pipeline.is_freed());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("a runtime");
        let reopened =
            runtime.block_on(async { tokio::time::timeout(Duration::from_secs(1), pipeline.reopened()).await.is_ok() });
        assert!(reopened, "its decoder is open already, so it may be started at once");
        pipeline.hold();
        pipeline.hold();
        assert!(!pipeline.is_open(), "a second hold finds nothing to close");
    }

    fn report(path: Path, size: Option<(u32, u32)>) -> Report {
        Report {
            codec: Codec::H264,
            path,
            on_gpu: false,
            decoded: 0,
            seconds: 0.0,
            dropped: 0,
            failed: 0,
            size,
            freezes: 0,
            longest_freeze: Duration::ZERO,
            behind: false,
        }
    }

    #[test]
    fn the_card_names_the_codec_the_screen_arrived_in() {
        let card = |codec| Report { codec, ..report(Path::Software, Some((1280, 720))) };
        let named = |codec| card(codec).card(&ada(), None, 0, ShareState::Live).map(|card| card.codec);
        assert_eq!((named(Codec::H264), named(Codec::Av1)), (Some("H.264".into()), Some("AV1".into())));
    }

    #[test]
    fn the_card_names_the_decode_path_and_flags_a_software_one() {
        let hardware = report(Path::Hardware(Backend::VaApi), Some((1920, 1080)))
            .card(&ada(), Some(60), 4, ShareState::Live)
            .unwrap();
        assert_eq!(hardware.dropped, 4, "the pictures this viewer did not take travel to the card");
        assert_eq!(hardware.decoder, Route::Hardware { via: "VA-API".into() });
        assert!(!ShareCard::software(&hardware.decoder));
        assert_eq!(hardware.size_and_rate(&words::Words::english()), "1920 \u{d7} 1080 \u{b7} 60 fps");
        let vulkan =
            report(Path::Hardware(Backend::Vulkan), Some((1280, 720))).card(&ada(), None, 0, ShareState::Live).unwrap();
        assert_eq!(vulkan.decoder, Route::Hardware { via: "Vulkan".into() });
        let software = report(Path::Software, Some((1280, 720))).card(&ada(), None, 0, ShareState::Live).unwrap();
        assert!(ShareCard::software(&software.decoder), "a fallback is flagged");
    }

    #[test]
    fn there_is_no_card_before_a_picture() {
        assert_eq!(report(Path::Software, None).card(&ada(), None, 0, ShareState::Live), None);
    }

    #[test]
    fn a_size_survives_being_packed() {
        assert_eq!(pack(3840, 2160), (3840 << 32) | 2160);
    }

    #[test]
    fn nothing_is_queued_before_the_first_key_frame() {
        let mut backlog = Backlog::new();
        assert!(!backlog.push(unit(false)), "the key frame was already asked for");
        assert_eq!((backlog.queue.len(), backlog.dropped), (0, 1));
        assert!(!backlog.push(unit(true)));
        assert!(!backlog.push(unit(false)));
        assert_eq!(backlog.queue.len(), 2);
    }

    #[test]
    fn a_stream_awaits_a_key_frame_before_its_first_and_again_after_a_break() {
        let mut backlog = Backlog::new();
        assert!(backlog.awaiting_key, "nothing yet");
        backlog.push(unit(true));
        assert!(!backlog.awaiting_key, "a key frame came");
        backlog.broken();
        assert!(backlog.awaiting_key, "a frame did not decode");
        backlog.push(unit(false));
        assert!(backlog.awaiting_key, "a frame that is not a key frame mends nothing");
        backlog.push(unit(true));
        assert!(!backlog.awaiting_key);
    }

    #[test]
    fn a_full_queue_is_emptied_and_the_sender_asked_for_a_key_frame_once() {
        let mut backlog = Backlog::new();
        backlog.push(unit(true));
        for _ in 1..QUEUED {
            assert!(!backlog.push(unit(false)));
        }
        assert!(backlog.push(unit(false)), "the frame that did not fit is the one that asks");
        assert_eq!((backlog.queue.len(), backlog.dropped), (0, QUEUED as u64 + 1));
        assert!(!backlog.push(unit(false)), "asked already; frames wait for the key frame in silence");
        assert!(!backlog.push(unit(true)));
        assert_eq!(backlog.queue.len(), 1);
    }

    #[test]
    fn a_key_frame_replaces_what_waited_behind_the_last_one() {
        let mut backlog = Backlog::new();
        backlog.push(unit(true));
        backlog.push(unit(false));
        backlog.push(unit(true));
        assert_eq!((backlog.queue.len(), backlog.dropped), (1, 2));
    }

    #[test]
    fn a_frame_that_did_not_decode_drops_what_follows_it_up_to_the_next_key_frame() {
        let mut backlog = Backlog::new();
        backlog.push(unit(true));
        backlog.push(unit(false));
        backlog.broken();
        assert_eq!(backlog.queue.len(), 0);
        assert!(!backlog.push(unit(false)));
        assert_eq!(backlog.queue.len(), 0);
        backlog.push(unit(true));
        assert_eq!(backlog.queue.len(), 1);
    }
}
