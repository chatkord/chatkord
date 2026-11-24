//! A call's voice on the sound server: the microphone and the speaker, each a stream PipeWire links to the device
//! of the `node.name` asked for, or to the default, which it then follows. One connection carries both, and says when
//! the server went away, so its owner can open them again.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;
use std::time::Duration;

use libspa::param::audio::AudioFormat;
use libspa_sys as spa_sys;
use pipewire_sys as pw;

use crate::audio;
use crate::conn::{Connection, Properties, Stream};
use crate::hearing::{Capture, Hearing};
use crate::library::Api;
use crate::play::fill;
use crate::session::{Error, State, Watched};

/// The latency both streams ask the graph for, in frames over the rate: one 10 ms frame at 48 kHz.
const LATENCY: &str = "480/48000";

/// A connection to the sound server for a call's microphone and speaker: the one the window's sounds play through
/// too, which lives while any of them does.
#[derive(Clone)]
pub struct Voice(Arc<Connection>);

/// How far a stream's sound is from its device, as PipeWire last measured it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    /// From the stream to the device, or from the device to the stream: the graph's delay, the resampler's and what
    /// waits in the stream's buffers.
    pub delay: Duration,
}

impl Voice {
    /// Connects to the sound server with what a stream needs loaded.
    pub fn open() -> Result<Self, Error> {
        Connection::shared().map(Self)
    }

    /// Whether PipeWire's real-time module was there to load, so the thread the streams' sound is handed over on
    /// asks to run ahead of ordinary work.
    pub fn real_time(&self) -> bool {
        self.0.real_time()
    }

    /// Whether the sound server ended this connection: every stream on it has stopped for good.
    pub fn lost(&self) -> bool {
        self.0.lost()
    }

    /// Records one channel of 16-bit samples at `rate` from the microphone whose `node.name` is `device`, or the
    /// default one. `hear` takes the samples of each quantum on PipeWire's real-time thread, so it must not block,
    /// lock or allocate.
    pub fn microphone(
        &self,
        device: Option<&str>,
        rate: u32,
        mut hear: impl FnMut(&[i16]) + Send + 'static,
    ) -> Result<Microphone, Error> {
        let properties = properties(c"Capture", c"chatkord-microphone", device);
        let capture = Capture {
            properties: &properties,
            positions: &[spa_sys::SPA_AUDIO_CHANNEL_MONO],
            linked_by_pipewire: true,
        };
        let hear = Box::new(move |bytes: &[u8]| hear(samples(bytes)));
        Ok(Microphone { hearing: Hearing::open(&self.0, &capture, rate, hear)? })
    }

    /// Plays two channels of 16-bit samples at `rate` on the speaker whose `node.name` is `device`, or the default
    /// one. `fill` is handed each quantum's interleaved samples to write, all of them, on PipeWire's real-time thread,
    /// so it must not block, lock or allocate.
    pub fn speaker(
        &self,
        device: Option<&str>,
        rate: u32,
        fill: impl FnMut(&mut [i16]) + Send + 'static,
    ) -> Result<Speaker, Error> {
        let format =
            audio::raw(AudioFormat::S16LE, rate, &[spa_sys::SPA_AUDIO_CHANNEL_FL, spa_sys::SPA_AUDIO_CHANNEL_FR])?;
        let pairs = properties(c"Playback", c"chatkord-speaker", device);
        let state = Arc::new(Watched::new(State::Unconnected));
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_stream_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_STREAM_EVENTS;
        events.process = Some(on_feed);
        events.state_changed = Some(on_feed_state);
        let held = self.0.lock();
        let stream = held.stream(
            c"chatkord-speaker",
            &Properties::Pairs(pairs.as_slice()),
            events,
            "make the speaker's stream",
            |stream| Feed {
                api: self.0.api(),
                stream,
                fill: UnsafeCell::new(Box::new(fill)),
                state: Arc::clone(&state),
            },
        )?;
        let mut params = [format.as_ptr().cast::<spa_sys::spa_pod>()];
        let flags = pw::pw_stream_flags_PW_STREAM_FLAG_AUTOCONNECT
            | pw::pw_stream_flags_PW_STREAM_FLAG_MAP_BUFFERS
            | pw::pw_stream_flags_PW_STREAM_FLAG_RT_PROCESS;
        if !stream.connect(&held, spa_sys::SPA_DIRECTION_OUTPUT, pw::PW_ID_ANY, flags, &mut params) {
            return Err(Error::Refused("connect the speaker's stream"));
        }
        Ok(Speaker { stream, state })
    }
}

/// A voice stream's node: its category, its name, and the device it is linked to. A named device is kept to: gone,
/// the stream stops rather than moving to another, so its owner decides what plays instead.
fn properties(category: &CStr, name: &CStr, device: Option<&str>) -> Vec<(&'static CStr, String)> {
    let mut pairs = vec![
        (c"media.type", "Audio".to_owned()),
        (c"media.category", category.to_string_lossy().into_owned()),
        (c"media.role", "Communication".to_owned()),
        (c"node.name", name.to_string_lossy().into_owned()),
        (c"node.description", "Chatkord".to_owned()),
        (c"node.latency", LATENCY.to_owned()),
    ];
    if let Some(device) = device {
        pairs.extend([(c"target.object", device.to_owned()), (c"node.dont-reconnect", "true".to_owned())]);
    }
    pairs
}

/// The samples of a quantum's bytes: little-endian 16-bit, as the stream asked, on a little-endian machine.
fn samples(bytes: &[u8]) -> &[i16] {
    // SAFETY: every bit pattern is an `i16`; what does not fall on an `i16`'s alignment is left out, never read.
    let (_, samples, _) = unsafe { bytes.align_to::<i16>() };
    samples
}

/// The same, to be written.
fn samples_mut(bytes: &mut [u8]) -> &mut [i16] {
    // SAFETY: as `samples`.
    let (_, samples, _) = unsafe { bytes.align_to_mut::<i16>() };
    samples
}

#[cfg(not(target_endian = "little"))]
compile_error!("the voice's streams are little-endian 16-bit samples, read in place");

/// How the stream's sound stands to its device, from `time` as PipeWire last measured it.
pub(crate) fn timing(time: &pw::pw_time, rate: u32) -> Timing {
    let (num, denom) = (u64::from(time.rate.num), u64::from(time.rate.denom));
    let graph = u64::try_from(time.delay).unwrap_or(0).saturating_mul(num).saturating_mul(1_000_000_000);
    let graph = graph.checked_div(denom).unwrap_or(0);
    let waiting = time.buffered.saturating_add(time.queued).saturating_mul(1_000_000_000) / u64::from(rate.max(1));
    Timing { delay: Duration::from_nanos(graph.saturating_add(waiting)) }
}

/// The microphone's stream. Dropping it lets go of the device.
pub struct Microphone {
    hearing: Hearing,
}

impl Microphone {
    /// Where the stream is in its life, as PipeWire last said.
    pub fn state(&self) -> State {
        self.hearing.state()
    }

    /// How long the microphone's sound took to reach the stream, at `rate`.
    pub fn timing(&self, rate: u32) -> Option<Timing> {
        self.hearing.timing(rate)
    }
}

/// What writes each quantum the speaker plays, on PipeWire's real-time thread.
type Fill = Box<dyn FnMut(&mut [i16]) + Send>;

/// What the speaker's callbacks share, read through shared references from the loop's thread and the data thread
/// at once: `fill` is only ever touched on the data thread.
struct Feed {
    api: &'static Api,
    stream: *mut pw::pw_stream,
    fill: UnsafeCell<Fill>,
    state: Arc<Watched>,
}

// SAFETY: PipeWire's loop and data threads read these through shared references; the stream pointer is only passed
// back to PipeWire from its own callbacks, and `fill` is touched by the data thread alone.
unsafe impl Send for Feed {}
// SAFETY: as for `Send`: no field is written through a shared reference but `fill`, on one thread.
unsafe impl Sync for Feed {}

/// The speaker's stream. Dropping it lets go of the device.
pub struct Speaker {
    stream: Stream<Feed>,
    state: Arc<Watched>,
}

impl Speaker {
    /// Where the stream is in its life, as PipeWire last said.
    pub fn state(&self) -> State {
        self.state.get()
    }

    /// How long the speaker's sound takes from the stream to the device, at `rate`.
    pub fn timing(&self, rate: u32) -> Option<Timing> {
        self.stream.time().map(|time| timing(&time, rate))
    }

    /// Plays the stream at `ratio` times its rate against the device, by resampling: how a clock that runs apart
    /// from the device's is followed. It needs the stream to be streaming.
    pub fn set_rate(&self, ratio: f32) -> Result<(), Error> {
        if self.stream.set_rate(ratio) { Ok(()) } else { Err(Error::Refused("set the speaker's rate")) }
    }
}

unsafe extern "C" fn on_feed_state(
    data: *mut c_void,
    _old: pw::pw_stream_state,
    state: pw::pw_stream_state,
    _error: *const c_char,
) {
    // SAFETY: `data` is the lent feed, alive until the stream is destroyed, read only through shared references.
    let feed = unsafe { &*data.cast::<Feed>() };
    feed.state.set(State::of(state));
}

unsafe extern "C" fn on_feed(data: *mut c_void) {
    // SAFETY: as in `on_feed_state`.
    let feed = unsafe { &*data.cast::<Feed>() };
    // SAFETY: called from the stream's `process`, as PipeWire requires.
    let buffer = unsafe { (feed.api.stream_dequeue_buffer)(feed.stream) };
    if buffer.is_null() {
        return;
    }
    // SAFETY: `fill` is touched only here, on the stream's data thread, one call at a time.
    let write = unsafe { &mut *feed.fill.get() };
    // SAFETY: the buffer was just dequeued and is not queued back until after this returns; two 16-bit channels.
    unsafe {
        fill(buffer, 2 * size_of::<i16>(), |bytes| {
            let samples = samples_mut(bytes);
            write(samples);
            samples.len() / 2
        })
    };
    // SAFETY: dequeued above and not queued since.
    unsafe { (feed.api.stream_queue_buffer)(feed.stream, buffer) };
}
