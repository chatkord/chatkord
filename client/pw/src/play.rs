//! Short sounds played on a speaker by the sound server: one connection made once, and one playback stream a sound,
//! which PipeWire drains before it is taken down, so a sound is whole and never waits on a timer of ours.

use std::ffi::{CStr, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use libspa::param::audio::AudioFormat;
use libspa_sys as spa_sys;
use pipewire_sys as pw;

use crate::audio;
use crate::conn::{Connection, Properties, Stream};
use crate::library::Api;
use crate::session::Error;

/// The bytes of one sample: 32-bit float, one channel.
const STRIDE: usize = size_of::<f32>();

/// A connection to the sound server that sounds play through: the one a call's voice shares, which lives while either
/// does. Dropping it ends any sound still playing.
pub struct Play {
    connection: Arc<Connection>,
}

/// What the stream's callbacks share: read on PipeWire's data thread and its loop's thread at once, so only through
/// shared references.
struct Callbacks {
    api: &'static Api,
    stream: *mut pw::pw_stream,
    samples: Arc<[f32]>,
    /// The samples handed to PipeWire so far.
    sent: AtomicUsize,
    /// A drain was asked for, once every sample was sent.
    draining: AtomicBool,
    drained: Arc<AtomicBool>,
}

/// One sound playing. Dropping it takes its stream down, cutting the sound if it has not drained.
pub struct Playing<'play> {
    _play: &'play Play,
    drained: Arc<AtomicBool>,
    _stream: Stream<Callbacks>,
}

impl Play {
    /// Connects to the sound server with what a stream needs loaded.
    pub fn open() -> Result<Self, Error> {
        Connection::shared().map(|connection| Self { connection })
    }

    /// Plays `samples`, one channel of 32-bit floats at `rate`, on the sink whose `node.name` is `speaker`, or the
    /// default sink; one no longer there leaves the session manager's choice, the default.
    pub fn start(&self, samples: Arc<[f32]>, rate: u32, speaker: Option<&str>) -> Result<Playing<'_>, Error> {
        let format = audio::raw(AudioFormat::F32LE, rate, &[spa_sys::SPA_AUDIO_CHANNEL_MONO])?;
        let mut pairs: Vec<(&CStr, String)> = vec![
            (c"media.type", "Audio".to_owned()),
            (c"media.category", "Playback".to_owned()),
            // The call's role, not a notification's: KDE mutes notifications while any app is full screen.
            (c"media.role", "Communication".to_owned()),
            (c"node.name", "chatkord-sound".to_owned()),
            (c"node.description", "Chatkord".to_owned()),
        ];
        pairs.extend(speaker.map(|speaker| (c"target.object", speaker.to_owned())));
        let drained = Arc::new(AtomicBool::new(false));
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_stream_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_STREAM_EVENTS;
        events.process = Some(on_process);
        events.drained = Some(on_drained);
        let held = self.connection.lock();
        let stream =
            held.stream(c"chatkord-sound", &Properties::Pairs(pairs.as_slice()), events, "make a sound", |stream| {
                Callbacks {
                    api: self.connection.api(),
                    stream,
                    samples,
                    sent: AtomicUsize::new(0),
                    draining: AtomicBool::new(false),
                    drained: Arc::clone(&drained),
                }
            })?;
        let mut params = [format.as_ptr().cast::<spa_sys::spa_pod>()];
        let flags = pw::pw_stream_flags_PW_STREAM_FLAG_AUTOCONNECT
            | pw::pw_stream_flags_PW_STREAM_FLAG_MAP_BUFFERS
            | pw::pw_stream_flags_PW_STREAM_FLAG_RT_PROCESS;
        if !stream.connect(&held, spa_sys::SPA_DIRECTION_OUTPUT, pw::PW_ID_ANY, flags, &mut params) {
            return Err(Error::Refused("connect a sound"));
        }
        Ok(Playing { _play: self, drained, _stream: stream })
    }
}

impl Playing<'_> {
    /// Whether every sample has played out of the speaker.
    pub fn drained(&self) -> bool {
        self.drained.load(Ordering::Acquire)
    }
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: `data` is the lent callbacks, alive until the stream is destroyed; read only through shared references.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    let api = callbacks.api;
    // SAFETY: called from the stream's `process`, as PipeWire requires.
    let buffer = unsafe { (api.stream_dequeue_buffer)(callbacks.stream) };
    if buffer.is_null() {
        return;
    }
    let sent = callbacks.sent.load(Ordering::Relaxed);
    let left = callbacks.samples.get(sent..).unwrap_or_default();
    // SAFETY: the buffer was just dequeued and is not queued back until after this returns.
    let written = unsafe {
        fill(buffer, STRIDE, |bytes| {
            let (into, _) = bytes.as_chunks_mut::<STRIDE>();
            for (into, sample) in into.iter_mut().zip(left) {
                *into = sample.to_le_bytes();
            }
            into.len().min(left.len())
        })
    };
    if written == 0 {
        // SAFETY: dequeued above and not queued since.
        unsafe { (api.stream_return_buffer)(callbacks.stream, buffer) };
        if !callbacks.draining.swap(true, Ordering::Relaxed) {
            // SAFETY: the stream is live; a flush is real-time safe and calls `drained` once it has played out.
            unsafe { (api.stream_flush)(callbacks.stream, true) };
        }
        return;
    }
    callbacks.sent.store(sent + written, Ordering::Relaxed);
    // SAFETY: dequeued above and not queued since.
    unsafe { (api.stream_queue_buffer)(callbacks.stream, buffer) };
}

unsafe extern "C" fn on_drained(data: *mut c_void) {
    // SAFETY: as in `on_process`.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    callbacks.drained.store(true, Ordering::Release);
}

/// Hands `write` the bytes of the first plane, as many whole frames of `frame_bytes` as the buffer asks for and holds;
/// `write` says how many frames it wrote, which the buffer then carries. Says how many frames that was.
///
/// # Safety
/// `buffer` was dequeued and not queued back.
pub(crate) unsafe fn fill(
    buffer: *mut pw::pw_buffer,
    frame_bytes: usize,
    write: impl FnOnce(&mut [u8]) -> usize,
) -> usize {
    // SAFETY: as the caller says, `buffer` is a live dequeued buffer.
    let buffer = unsafe { &mut *buffer };
    let (inner, requested) = (buffer.buffer, buffer.requested);
    // SAFETY: as above; its `spa_buffer` is null or live with it.
    let Some(spa_buffer) = (unsafe { inner.as_ref() }) else { return 0 };
    if spa_buffer.n_datas == 0 || spa_buffer.datas.is_null() || frame_bytes == 0 {
        return 0;
    }
    // SAFETY: checked above: `datas` holds at least one plane.
    let plane = unsafe { &mut *spa_buffer.datas };
    if plane.data.is_null() || plane.chunk.is_null() {
        return 0;
    }
    let room = usize::try_from(plane.maxsize).unwrap_or(0) / frame_bytes;
    // A stream that does not say what it wants (`requested` 0) takes what fits.
    let asked = usize::try_from(requested).ok().filter(|asked| *asked > 0).unwrap_or(room);
    let frames = room.min(asked);
    // SAFETY: the plane is mapped for `maxsize` bytes, and `frames` whole frames fit in it.
    let bytes = unsafe { std::slice::from_raw_parts_mut(plane.data.cast::<u8>(), frames * frame_bytes) };
    let written = write(bytes).min(frames);
    // SAFETY: checked non-null above; the chunk is the plane's, live with the buffer.
    let chunk = unsafe { &mut *plane.chunk };
    chunk.offset = 0;
    chunk.stride = i32::try_from(frame_bytes).unwrap_or(0);
    chunk.size = u32::try_from(written * frame_bytes).unwrap_or(0);
    buffer.size = written as u64;
    written
}
