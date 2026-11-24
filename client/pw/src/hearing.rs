//! A capture stream: a share's sound, a node of this client's own that the chosen applications' ports are linked
//! into, or a call's microphone, linked by PipeWire to the device it names. Read on PipeWire's real-time thread and
//! handed on as it comes.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;

use libspa::param::audio::AudioFormat;
use libspa_sys as spa_sys;
use pipewire_sys as pw;

use crate::audio;
use crate::conn::{Connection, Properties, Stream};
use crate::library::Api;
use crate::session::{Error, State, Watched};
use crate::voice::{Timing, timing};

/// What hears the bytes of each quantum, interleaved little-endian 16-bit samples, on PipeWire's real-time thread.
pub(crate) type Hear = Box<dyn FnMut(&[u8]) + Send>;

/// What a capture stream is.
pub(crate) struct Capture<'a> {
    /// The stream's node's properties.
    pub(crate) properties: &'a [(&'a CStr, String)],
    /// One channel for each position, as `SPA_AUDIO_CHANNEL_*` names them.
    pub(crate) positions: &'a [u32],
    /// PipeWire links it to its target, or the default; otherwise it is linked to nothing until links are made.
    pub(crate) linked_by_pipewire: bool,
}

/// What the callbacks are handed back, read through shared references from the loop's thread and the data thread
/// at once: `hear` is only ever touched on the data thread.
struct Callbacks {
    api: &'static Api,
    stream: *mut pw::pw_stream,
    hear: UnsafeCell<Hear>,
    state: Arc<Watched>,
}

// SAFETY: PipeWire's loop and data threads read these through shared references; the stream pointer is only passed
// back to PipeWire from its own callbacks, and `hear` is touched by the data thread alone.
unsafe impl Send for Callbacks {}
// SAFETY: as for `Send`: no field is written through a shared reference but `hear`, on one thread.
unsafe impl Sync for Callbacks {}

/// The capture stream. Dropping it takes the stream down under its connection's lock.
pub(crate) struct Hearing {
    stream: Stream<Callbacks>,
    state: Arc<Watched>,
}

impl Hearing {
    /// Opens the stream `capture` says on `connection`, in 16-bit samples at `rate`.
    pub(crate) fn open(
        connection: &Arc<Connection>,
        capture: &Capture<'_>,
        rate: u32,
        hear: Hear,
    ) -> Result<Self, Error> {
        // PipeWire converts any device's or application's sound into this.
        let format = audio::raw(AudioFormat::S16LE, rate, capture.positions)?;
        let state = Arc::new(Watched::new(State::Unconnected));
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_stream_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_STREAM_EVENTS;
        events.process = Some(on_process);
        events.state_changed = Some(on_state_changed);
        let held = connection.lock();
        let stream = held.stream(
            c"chatkord-capture",
            &Properties::Pairs(capture.properties),
            events,
            "make a capture stream",
            |stream| Callbacks {
                api: connection.api(),
                stream,
                hear: UnsafeCell::new(hear),
                state: Arc::clone(&state),
            },
        )?;
        let mut params = [format.as_ptr().cast::<spa_sys::spa_pod>()];
        let mut flags = pw::pw_stream_flags_PW_STREAM_FLAG_MAP_BUFFERS | pw::pw_stream_flags_PW_STREAM_FLAG_RT_PROCESS;
        if capture.linked_by_pipewire {
            flags |= pw::pw_stream_flags_PW_STREAM_FLAG_AUTOCONNECT;
        }
        if !stream.connect(&held, spa_sys::SPA_DIRECTION_INPUT, pw::PW_ID_ANY, flags, &mut params) {
            return Err(Error::Refused("connect a capture stream"));
        }
        Ok(Self { stream, state })
    }

    /// Where the stream is in its life, as PipeWire last said.
    pub(crate) fn state(&self) -> State {
        self.state.get()
    }

    /// How the stream's sound stands to its device, at `rate`.
    pub(crate) fn timing(&self, rate: u32) -> Option<Timing> {
        self.stream.time().map(|time| timing(&time, rate))
    }
}

unsafe extern "C" fn on_state_changed(
    data: *mut c_void,
    _old: pw::pw_stream_state,
    state: pw::pw_stream_state,
    _error: *const c_char,
) {
    // SAFETY: `data` is the lent callbacks, alive until the stream is destroyed, read only through shared references.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    callbacks.state.set(State::of(state));
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: as in `on_state_changed`.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    // SAFETY: called from the stream's `process`, as PipeWire requires.
    let buffer = unsafe { (callbacks.api.stream_dequeue_buffer)(callbacks.stream) };
    if buffer.is_null() {
        return;
    }
    // SAFETY: the buffer was just dequeued; every pointer is checked before it is read, and the bytes are within the
    // mapped size PipeWire gave.
    if let Some(bytes) = unsafe { mapped(buffer) } {
        // SAFETY: `hear` is touched only here, on the stream's data thread, one call at a time.
        (unsafe { &mut *callbacks.hear.get() })(bytes);
    }
    // SAFETY: dequeued above and not queued since.
    unsafe { (callbacks.api.stream_queue_buffer)(callbacks.stream, buffer) };
}

/// The bytes a buffer's first plane holds.
///
/// # Safety
/// `buffer` was dequeued and not queued back; the returned bytes are read before it is.
unsafe fn mapped<'a>(buffer: *mut pw::pw_buffer) -> Option<&'a [u8]> {
    // SAFETY: as the caller says, `buffer` is a live dequeued buffer.
    let inner = unsafe { (*buffer).buffer };
    // SAFETY: as above; its `spa_buffer` is null or live with it.
    let spa_buffer = unsafe { inner.as_ref() }?;
    if spa_buffer.n_datas == 0 || spa_buffer.datas.is_null() {
        return None;
    }
    // SAFETY: checked above: `datas` holds at least one plane.
    let data = unsafe { &*spa_buffer.datas };
    // SAFETY: a plane's chunk is null or live with its buffer.
    let chunk = unsafe { data.chunk.as_ref() }?;
    if data.data.is_null() {
        return None;
    }
    let start = chunk.offset.min(data.maxsize);
    let length = usize::try_from(chunk.size.min(data.maxsize - start)).ok()?;
    let start = usize::try_from(start).ok()?;
    // SAFETY: `start` is at most `maxsize`, the mapped size PipeWire gave.
    let first = unsafe { data.data.cast::<u8>().add(start) };
    // SAFETY: `length` bytes from `start` stay within `maxsize`, mapped until the buffer is queued back.
    Some(unsafe { std::slice::from_raw_parts(first, length) })
}
