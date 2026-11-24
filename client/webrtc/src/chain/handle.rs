//! The handle of a chain: what is asked of it, and the guards that end it and a track in its mix.

use super::devices::{Listing, Microphone, MicrophoneEnd, Sound, Speaker, SpeakerEnd};
use super::mixer::{Frames, Slot, Source, Track};
use super::report::{ChainHealth, Sounding, Standing, Turn};
use super::shared::{Shared, State, Which, lock};
use super::thread::run;
use super::{RATE_HZ, STOP_BOUND, TRACK_QUEUE};
use crate::devices::DevicesRefused;
use crate::keeper::Station;
use crate::processing::Processing;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::audio_stream::native::{NativeAudioStream, NativeAudioStreamOptions};
use libwebrtc::audio_track::RtcAudioTrack;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// The chain of one engine. Its handles only ask: the keepers open and close the devices to match, and the warden
/// replaces one held in a call. Dropping the last handle stops the threads, which let go of both devices, and waits
/// for them no longer than [`STOP_BOUND`].
pub(crate) struct Chain {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    wake: Thread,
    /// Closed by the thread as it ends.
    ended: Mutex<mpsc::Receiver<()>>,
    keepers: Arc<Keepers>,
    warden: Option<JoinHandle<()>>,
}

/// Each device's keeper, as the chain's handles and its warden reach them.
struct Keepers {
    microphone: Mutex<Station<dyn Microphone>>,
    speaker: Mutex<Station<dyn Speaker>>,
}

impl Keepers {
    fn wake(&self, which: Which) {
        match which {
            Which::Microphone => lock(&self.microphone).wake(),
            Which::Speaker => lock(&self.speaker).wake(),
        }
    }
}

/// The warden's thread: gives up on a keeper held in one call past the chain's bound for a fresh one, waking as the
/// look in hand would pass it, until the chain stops.
fn ward(shared: &Shared, keepers: &Keepers) {
    while !shared.stop.load(Ordering::Acquire) && !shared.halted.load(Ordering::Acquire) {
        let now = Instant::now();
        let microphone = lock(&keepers.microphone).watch(now);
        let speaker = lock(&keepers.speaker).watch(now);
        thread::park_timeout(microphone.min(speaker));
    }
}

/// What the mix thread is handed of the devices: each end, as its keeper made it the one in use, and none as the
/// device closed.
pub(super) struct Ends {
    pub(super) microphone: mpsc::Receiver<Option<Box<dyn MicrophoneEnd>>>,
    pub(super) speaker: mpsc::Receiver<Option<Box<dyn SpeakerEnd>>>,
}

/// How a microphone's keeper hands the mix thread the end it made the one in use, and none as the device closed.
pub(super) type MicrophoneSender = mpsc::Sender<Option<Box<dyn MicrophoneEnd>>>;
/// The same for the speaker.
pub(super) type SpeakerSender = mpsc::Sender<Option<Box<dyn SpeakerEnd>>>;

impl Ends {
    /// Both ends' receivers, and the senders a keeper's station hands each end through.
    pub(super) fn channels() -> (Self, MicrophoneSender, SpeakerSender) {
        let (microphone_ends, microphone) = mpsc::channel();
        let (speaker_ends, speaker) = mpsc::channel();
        (Self { microphone, speaker }, microphone_ends, speaker_ends)
    }
}

impl Chain {
    /// Runs the chain for the engine `factory` makes, which it tells how late the speaker plays, so the engine holds
    /// each picture to its sound; a keeper held in one call for `give_up_after` is replaced.
    pub(crate) fn start(
        sound: Arc<dyn Sound>,
        processing: Processing,
        factory: PeerConnectionFactory,
        give_up_after: Duration,
    ) -> Result<Self, DevicesRefused> {
        let shared = Arc::new(Shared::new(sound, give_up_after));
        let running = Arc::clone(&shared);
        let (done, ended) = mpsc::channel();
        let (mut ends, microphone_ends, speaker_ends) = Ends::channels();
        let thread = thread::Builder::new()
            .name("chatkord-voice".into())
            .spawn(move || {
                let _ending = Ending { shared: &running, _done: done };
                run(&running, processing, &factory, &mut ends);
            })
            .map_err(|_| DevicesRefused::Unavailable)?;
        let wake = thread.thread().clone();
        let mut microphone = Station::new(Arc::clone(&shared), wake.clone(), microphone_ends);
        let mut speaker = Station::new(Arc::clone(&shared), wake.clone(), speaker_ends);
        let started = microphone.start().and_then(|()| speaker.start());
        let keepers = Arc::new(Keepers { microphone: Mutex::new(microphone), speaker: Mutex::new(speaker) });
        let (warded, watched) = (Arc::clone(&shared), Arc::clone(&keepers));
        let warden = thread::Builder::new().name("chatkord-voice-warden".into()).spawn(move || ward(&warded, &watched));
        let warding = warden.as_ref().map(|_| ()).map_err(|_| DevicesRefused::Unavailable);
        let chain = Self { shared, thread: Some(thread), wake, ended: Mutex::new(ended), keepers, warden: warden.ok() };
        started.and(warding)?;
        Ok(chain)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.shared.state)
    }

    /// The devices there now.
    pub(crate) fn listing(&self) -> Result<Listing, DevicesRefused> {
        self.shared.sound.listing()
    }

    /// The source a microphone track is made from.
    pub(crate) fn source(&self) -> NativeAudioSource {
        self.shared.source.clone()
    }

    /// What became of each device, as the thread answers what is asked.
    pub(crate) fn sounding(&self) -> watch::Receiver<Sounding> {
        self.shared.sounding.subscribe()
    }

    /// What the mix thread is doing now.
    pub(crate) fn doing(&self) -> Turn {
        Turn::of(self.shared.doing.load(Ordering::Relaxed))
    }

    /// Asks for `asked` as the microphone; one that runs is moved by the thread.
    pub(crate) fn choose_microphone(&self, asked: Option<&str>) {
        self.choose(Which::Microphone, asked);
    }

    /// Asks for `asked` as the speaker; one that runs is moved by the thread.
    pub(crate) fn choose_speaker(&self, asked: Option<&str>) {
        self.choose(Which::Speaker, asked);
    }

    /// Every choice is handed to the device, which knows whether it changes anything; it is said to be opening
    /// only when it names another device or the last try was refused.
    fn choose(&self, which: Which, device: Option<&str>) {
        let mut state = self.state();
        let asked = which.asked_mut(&mut state);
        let moved = asked.device.as_deref() != device;
        asked.device = device.map(str::to_owned);
        asked.asks = asked.asks.wrapping_add(1);
        if asked.holds > 0 {
            self.shared.sounding.send_if_modified(|sounding| {
                let standing = which.standing(sounding);
                let again = moved || matches!(standing, Standing::Refused(_));
                if again {
                    *standing = Standing::Opening;
                }
                again
            });
        }
        drop(state);
        self.asked(which);
    }

    /// Wakes the keeper of `which` and the mix thread, either of which may sleep until something is asked of it.
    fn asked(&self, which: Which) {
        self.keepers.wake(which);
        self.wake.unpark();
    }

    fn hold(&self, which: Which) -> Result<(), DevicesRefused> {
        let mut state = self.state();
        if state.ended {
            return Err(DevicesRefused::Unavailable);
        }
        let asked = which.asked_mut(&mut state);
        if asked.holds == 0 {
            asked.asks = asked.asks.wrapping_add(1);
            self.shared.sounding.send_modify(|sounding| *which.standing(sounding) = Standing::Opening);
        }
        asked.holds += 1;
        drop(state);
        self.asked(which);
        Ok(())
    }

    fn release(&self, which: Which) {
        let mut state = self.state();
        let asked = which.asked_mut(&mut state);
        asked.holds = asked.holds.saturating_sub(1);
        drop(state);
        self.asked(which);
    }

    /// One more holder of the microphone; the thread opens it for the first. Refused only with the thread gone.
    pub(crate) fn capture(&self) -> Result<(), DevicesRefused> {
        self.hold(Which::Microphone)
    }

    /// One holder fewer; the thread closes the microphone after the last.
    pub(crate) fn release_capture(&self) {
        self.release(Which::Microphone);
    }

    /// One more holder of the speaker; the thread opens it for the first. Refused only with the thread gone.
    pub(crate) fn play(&self) -> Result<(), DevicesRefused> {
        self.hold(Which::Speaker)
    }

    /// One holder fewer; the thread closes the speaker after the last.
    pub(crate) fn release_play(&self) {
        self.release(Which::Speaker);
    }

    fn flows(&self, which: Which) -> bool {
        let state = self.state();
        !state.ended && which.asked(&state).holds > 0 && which.flowing(&self.shared).load(Ordering::Relaxed)
    }

    /// Whether a microphone is held and open: its sound flows, or its stream is being linked.
    pub(crate) fn is_capturing(&self) -> bool {
        self.flows(Which::Microphone)
    }

    /// Whether a speaker is held and open: its sound flows, or its stream is being linked.
    pub(crate) fn is_playing(&self) -> bool {
        self.flows(Which::Speaker)
    }

    /// Processes the microphone as `processing` says from its next frame.
    pub(crate) fn process(&self, processing: Processing) {
        *lock(&self.shared.asked) = Some(processing);
        self.wake.unpark();
    }

    /// One more holder of the microphone test: while there is one the processed microphone is played back on the
    /// speaker a little late, in place of the call. The speaker plays only while someone holds it
    /// ([`Chain::play`]).
    pub(crate) fn test(&self) {
        self.state().testing.holds += 1;
    }

    /// One holder fewer; the last ends the test and forgets what waited to be played back.
    pub(crate) fn release_test(&self) {
        let mut state = self.state();
        state.testing.holds = state.testing.holds.saturating_sub(1);
        if state.testing.holds == 0 {
            state.testing.late.clear();
        }
    }

    /// How the chain is doing now.
    /// Whether the mix thread runs, read without taking the most delay a health read takes.
    pub(crate) fn alive(&self) -> bool {
        !self.state().ended
    }

    pub(crate) fn health(&self) -> ChainHealth {
        let counts = &self.shared.counts;
        let (tests, alive, tracks, speaker_holds) = {
            let state = self.state();
            (state.testing.holds, !state.ended, state.tracks.len(), state.speaker.holds)
        };
        ChainHealth {
            turns: counts.turns.load(Ordering::Relaxed),
            captured: counts.captured.load(Ordering::Relaxed),
            played: counts.played.load(Ordering::Relaxed),
            received: counts.received.load(Ordering::Relaxed),
            tracks,
            speaker_holds,
            tests,
            restarts: counts.restarts.load(Ordering::Relaxed),
            doing: self.doing(),
            microphone: self.shared.microphone.health(),
            speaker: self.shared.speaker.health(),
            microphone_dropped: counts.microphone_dropped.load(Ordering::Relaxed),
            speaker_dropped: counts.speaker_dropped.load(Ordering::Relaxed),
            speaker_underruns: counts.speaker_underruns.load(Ordering::Relaxed),
            speaker_delay_ms: counts.speaker_delay_ms.load(Ordering::Relaxed),
            speaker_delay_most_ms: counts.speaker_delay_most_ms.swap(0, Ordering::Relaxed),
            alive,
        }
    }

    /// The loudest frame's root mean square before the gate since this was last asked, in dB below full scale;
    /// none while the microphone is closed or before a frame.
    pub(crate) fn loudness(&self) -> Option<f32> {
        if !self.is_capturing() {
            return None;
        }
        lock(&self.shared.loudness).take()
    }

    /// How loud the processed microphone was since this was last asked, from 0 to 100; none while it is closed.
    pub(crate) fn level(&self) -> Option<u8> {
        if !self.is_capturing() {
            return None;
        }
        self.shared.take_level()
    }

    /// Mixes `track` (`channels` of them) at the gain `gain` holds (an `f32`'s bits) until the returned handle
    /// drops, and keeps its loudest sample before the gain in `peak` for its meter.
    pub(crate) fn hear(
        &self,
        track: RtcAudioTrack,
        channels: u32,
        gain: Arc<AtomicU32>,
        peak: Arc<AtomicU32>,
    ) -> Heard {
        let options = NativeAudioStreamOptions { queue_size_frames: Some(TRACK_QUEUE) };
        #[expect(clippy::cast_possible_wrap, reason = "48 000 and one or two channels")]
        let stream = NativeAudioStream::with_options(track, RATE_HZ as i32, channels as i32, options);
        let heard = hear_stream(&self.shared, Box::pin(stream), channels, gain, peak);
        self.wake.unpark();
        heard
    }
}

/// Mixes `stream` (`channels` of them) as [`Chain::hear`] mixes a track's.
pub(super) fn hear_stream(
    shared: &Arc<Shared>,
    stream: Frames,
    channels: u32,
    gain: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
) -> Heard {
    let mut state = lock(&shared.state);
    state.next_id = state.next_id.wrapping_add(1);
    let id = state.next_id;
    let slot = Arc::new(Mutex::new(Slot::default()));
    let mixed = Arc::new(AtomicU32::new(0));
    if let Some(mixer) = &mut state.mixer {
        mixer.add_source(Source { id, slot: Arc::clone(&slot), gain, mixed: Arc::clone(&mixed) });
    }
    state.tracks.push(Track { id, stream, channels, waiting: VecDeque::new(), slot, peak, ended: false });
    drop(state);
    Heard { shared: Arc::downgrade(shared), id, mixed }
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.wake.unpark();
        let deadline = Instant::now() + STOP_BOUND;
        if let Some(warden) = self.warden.take() {
            warden.thread().unpark();
            let _ = warden.join();
        }
        let (mut microphone, mut speaker) = (lock(&self.keepers.microphone), lock(&self.keepers.speaker));
        microphone.wake();
        speaker.wake();
        if let Some(thread) = self.thread.take() {
            let ended = self.ended.get_mut().unwrap_or_else(PoisonError::into_inner);
            match ended.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = thread.join();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    report::note!(
                        warn,
                        "the voice chain's thread was still running after it was stopped; it is left behind",
                        doing = Turn::of(self.shared.doing.load(Ordering::Relaxed)),
                        bound_ms = report::millis(STOP_BOUND),
                    );
                }
            }
        }
        microphone.stop(deadline);
        speaker.stop(deadline);
    }
}

/// Says the thread is gone as it ends, however it ends: a stop leaves both devices closed, anything else leaves
/// them refused, and whoever waits for the thread is let go.
struct Ending<'a> {
    shared: &'a Shared,
    _done: mpsc::Sender<()>,
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        let left = if self.shared.stop.load(Ordering::Acquire) {
            Standing::Closed
        } else {
            Standing::Refused(DevicesRefused::Unavailable)
        };
        let mut state = lock(&self.shared.state);
        state.ended = true;
        self.shared.halted.store(true, Ordering::Release);
        self.shared.capturing.store(false, Ordering::Relaxed);
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.sounding.send_replace(Sounding { microphone: left, speaker: left });
    }
}

/// A received track in the mix; dropping it takes the track out.
pub(crate) struct Heard {
    shared: std::sync::Weak<Shared>,
    id: i32,
    #[cfg_attr(not(any(test, feature = "testing")), expect(dead_code, reason = "read by a test's measure of the mix"))]
    mixed: Arc<AtomicU32>,
}

impl Heard {
    /// The loudest sample mixed since this was last asked, after the track's gain.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn mixed(&self) -> u32 {
        self.mixed.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Heard {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            let mut state = lock(&shared.state);
            state.tracks.retain(|track| track.id != self.id);
            if let Some(mixer) = &mut state.mixer {
                mixer.remove_source(self.id);
            }
        }
    }
}
