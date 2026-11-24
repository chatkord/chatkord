//! One input stream on the connection the portal opened, run by PipeWire's thread loop. A lent buffer goes home
//! through the loop's recursive lock from whichever thread drops it, a callback's included, so no channel is needed.

use std::ffi::{CStr, c_char, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::os::fd::{BorrowedFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::conn::{Connection, Listening, Properties, Proxy, RegistryProxy, Stream};
use crate::dict::{Borrowed, interface};
use crate::graph::Props;
use crate::lent::Lent;
use crate::library::{Api, Unavailable};
use crate::looper::{Locked, Looper, STREAM};
use crate::methods;

/// The id of the format parameter, which [`Handler::param_changed`] is told about.
pub const FORMAT: u32 = spa::SPA_PARAM_Format;

/// Why a session did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// PipeWire is not on this device, or is too old.
    Unavailable(Unavailable),
    /// PipeWire refused the named step.
    Refused(&'static str),
    /// A parameter was not a well-formed pod.
    Pod,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "PipeWire is unavailable: {why}"),
            Self::Refused(step) => write!(f, "PipeWire refused to {step}"),
            Self::Pod => f.write_str("a PipeWire parameter was malformed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Unavailable> for Error {
    fn from(why: Unavailable) -> Self {
        Self::Unavailable(why)
    }
}

/// What a capture stream takes, as it tells the daemon: a policy (and a desktop's indicator) reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A screen or a window, through the screen cast portal.
    Screen,
    /// A camera, through the camera portal.
    Camera,
}

impl Role {
    fn properties(self) -> &'static CStr {
        match self {
            Role::Screen => c"media.type=Video media.category=Capture media.role=Screen",
            Role::Camera => c"media.type=Video media.category=Capture media.role=Camera",
        }
    }
}

/// Where a stream is in its life, as PipeWire says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Failed, or in a state this client does not know: it will deliver nothing.
    Error,
    /// Not connected to a node.
    Unconnected,
    /// Connecting to its node.
    Connecting,
    /// Connected and configured, but not processing.
    Paused,
    /// Delivering buffers to [`Handler::process`].
    Streaming,
}

impl State {
    pub(crate) fn of(raw: pw::pw_stream_state) -> Self {
        match raw {
            pw::pw_stream_state_PW_STREAM_STATE_UNCONNECTED => Self::Unconnected,
            pw::pw_stream_state_PW_STREAM_STATE_CONNECTING => Self::Connecting,
            pw::pw_stream_state_PW_STREAM_STATE_PAUSED => Self::Paused,
            pw::pw_stream_state_PW_STREAM_STATE_STREAMING => Self::Streaming,
            _ => Self::Error,
        }
    }
}

/// A stream's [`State`], set by its callback on the loop's thread and read from any.
pub(crate) struct Watched(AtomicU8);

impl Watched {
    const ORDER: [State; 5] = [State::Error, State::Unconnected, State::Connecting, State::Paused, State::Streaming];

    pub(crate) fn new(state: State) -> Self {
        let watched = Self(AtomicU8::new(0));
        watched.set(state);
        watched
    }

    pub(crate) fn set(&self, state: State) {
        let index = Self::ORDER.iter().position(|known| *known == state).unwrap_or(0);
        self.0.store(u8::try_from(index).unwrap_or(0), Ordering::Release);
    }

    pub(crate) fn get(&self) -> State {
        Self::ORDER.get(usize::from(self.0.load(Ordering::Acquire))).copied().unwrap_or(State::Error)
    }
}

/// What hears the stream. Its methods run on PipeWire's thread, with the loop's lock held.
pub trait Handler: Send + 'static {
    /// The stream's state changed.
    fn state_changed(&mut self, state: State);

    /// A parameter changed. `param` is a serialised pod, or `None` when it was cleared. Answer a [`FORMAT`] with
    /// [`StreamRef::update_params`].
    fn param_changed(&mut self, stream: &StreamRef<'_>, id: u32, param: Option<&[u8]>);

    /// Buffers are ready: [`StreamRef::dequeue`] them.
    fn process(&mut self, stream: &StreamRef<'_>);
}

/// What every handle to a session shares: the loop, and whether its stream is still there.
struct Inner {
    api: &'static Api,
    looper: Arc<Looper>,
    /// Set, under the lock, when the stream is gone: a buffer that outlives it has nowhere to go.
    closed: AtomicBool,
    /// Counts buffers PipeWire has taken back; a lent buffer of an earlier count is not ours.
    removed: AtomicU64,
    /// Counts buffers PipeWire has given the stream, so the ones it holds are this less [`Inner::removed`].
    added: AtomicU64,
}

impl Inner {
    fn lock(&self) -> Locked<'_> {
        self.looper.lock()
    }
}

/// The stream, as a handler's callbacks see it.
pub struct StreamRef<'a> {
    inner: &'a Arc<Inner>,
    stream: *mut pw::pw_stream,
}

impl StreamRef<'_> {
    /// Takes the next buffer the stream has ready.
    pub fn dequeue(&self) -> Option<Dequeued<'_>> {
        // SAFETY: called from a callback, on the loop's thread, as PipeWire requires.
        let raw = unsafe { (self.inner.api.stream_dequeue_buffer)(self.stream) };
        (!raw.is_null()).then(|| Dequeued {
            inner: self.inner,
            stream: self.stream,
            raw,
            done: false,
            _loop_thread: PhantomData,
        })
    }

    /// The buffers the stream holds now: as many as both ends settled on, once it streams.
    pub fn buffers(&self) -> u64 {
        let removed = self.inner.removed.load(Ordering::Acquire);
        self.inner.added.load(Ordering::Acquire).saturating_sub(removed)
    }

    /// Tells PipeWire the parameters we accept, each a serialised pod.
    pub fn update_params(&self, params: &[&[u8]]) -> Result<(), Error> {
        let pods = params.iter().map(|p| aligned(p)).collect::<Result<Vec<_>, _>>()?;
        let mut pointers: Vec<*const spa::spa_pod> = pods.iter().map(|p| p.as_ptr().cast()).collect();
        let count = u32::try_from(pointers.len()).map_err(|_| Error::Pod)?;
        // SAFETY: each pointer is a validated, aligned pod that outlives the call.
        let status = unsafe { (self.inner.api.stream_update_params)(self.stream, pointers.as_mut_ptr(), count) };
        if status < 0 { Err(Error::Refused("update the stream's parameters")) } else { Ok(()) }
    }
}

/// A pod copied to where C may read it: aligned to eight bytes, and as long as it says it is.
pub(crate) fn aligned(bytes: &[u8]) -> Result<Vec<u64>, Error> {
    let size = bytes.get(..4).map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]])).ok_or(Error::Pod)?;
    let total = usize::try_from(size).ok().and_then(|s| s.checked_add(size_of::<spa::spa_pod>())).ok_or(Error::Pod)?;
    if total > bytes.len() {
        return Err(Error::Pod);
    }
    let mut words = vec![0u64; bytes.len().div_ceil(8)];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks(8)) {
        let mut eight = [0u8; 8];
        eight[..chunk.len()].copy_from_slice(chunk);
        *word = u64::from_ne_bytes(eight);
    }
    Ok(words)
}

/// A buffer taken from the stream inside `process`, not yet kept. Dropped, it goes back.
pub struct Dequeued<'a> {
    inner: &'a Arc<Inner>,
    stream: *mut pw::pw_stream,
    raw: *mut pw::pw_buffer,
    done: bool,
    /// It is read where it was dequeued, on the loop's thread, and nowhere else.
    _loop_thread: PhantomData<*mut ()>,
}

/// A buffer's time from its producer, whose clock PipeWire does not name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    /// The picture's time since the producer's clock began.
    pub at: Duration,
    /// The producer's clock broke before this picture, so it does not follow from the last one.
    pub discontinuous: bool,
}

/// The picture in a buffer the daemon mapped into this process (memory, or a memory file), valid while it is dequeued.
#[derive(Clone, Copy, Debug)]
pub struct Mapped<'a> {
    /// The picture's bytes, from its first row to the end of its chunk.
    pub bytes: &'a [u8],
    /// Bytes from one row to the next, where the producer said (never for a compressed picture).
    pub stride: Option<u32>,
}

/// The picture in a buffer: a DMA-BUF, copied as a descriptor so it stands alone.
#[derive(Debug)]
pub struct Plane {
    /// The DMA-BUF's descriptor, a copy this value owns.
    pub fd: OwnedFd,
    /// Bytes from the start of the buffer to the first row.
    pub offset: u32,
    /// Bytes from one row to the next; never zero.
    pub stride: u32,
}

impl Dequeued<'_> {
    /// The DMA-BUF, unless the buffer is not one, the compositor marked it corrupt, it holds no picture, or it has
    /// more than one plane.
    pub fn plane(&self) -> Option<Plane> {
        // SAFETY: the buffer was dequeued on this thread and is not yet queued back.
        unsafe { plane_of(self.raw) }
    }

    /// The picture in memory, unless the buffer is a DMA-BUF, the producer marked it corrupt, it holds no picture, or
    /// its chunk lies outside its memory.
    pub fn mapped(&self) -> Option<Mapped<'_>> {
        // SAFETY: the buffer was dequeued on this thread and is not yet queued back, which `&self` holds for `'_`.
        unsafe { mapped_of(self.raw) }
    }

    /// When the producer says the picture was made, on its own clock: `None` when it wrote no time.
    pub fn time(&self) -> Option<Time> {
        // SAFETY: the buffer was dequeued on this thread and is not yet queued back.
        let header = unsafe { header_of(self.raw) }?;
        let at = Duration::from_nanos(u64::try_from(header.pts).ok().filter(|pts| *pts > 0)?);
        Some(Time { at, discontinuous: header.flags & spa::SPA_META_HEADER_FLAG_DISCONT != 0 })
    }

    /// Keeps the buffer out of the compositor's pool until the returned [`Buffer`] drops.
    pub fn lend(mut self) -> Buffer {
        self.done = true;
        Buffer {
            inner: self.inner.clone(),
            stream: StreamPtr(self.stream),
            raw: BufferPtr(self.raw),
            removed: self.inner.removed.load(Ordering::Acquire),
        }
    }
}

impl Drop for Dequeued<'_> {
    fn drop(&mut self) {
        if !self.done {
            // SAFETY: dequeued on this thread and not queued since.
            unsafe { (self.inner.api.stream_queue_buffer)(self.stream, self.raw) };
        }
    }
}

struct StreamPtr(*mut pw::pw_stream);
struct BufferPtr(*mut pw::pw_buffer);

// SAFETY: both pointers are only used under the loop's lock, and only while the stream is open and has taken back
// no buffers since this one was lent.
unsafe impl Send for StreamPtr {}
// SAFETY: as above.
unsafe impl Send for BufferPtr {}

/// A buffer the compositor lent. Dropping it, from any thread, sends it home.
pub struct Buffer {
    inner: Arc<Inner>,
    stream: StreamPtr,
    raw: BufferPtr,
    removed: u64,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let _lock = self.inner.lock();
        // A closed stream took its buffers with it; so did one that has since taken any back.
        if !self.inner.closed.load(Ordering::Acquire) && self.inner.removed.load(Ordering::Acquire) == self.removed {
            // SAFETY: the stream is open, and this buffer was dequeued from it and not queued since.
            unsafe { (self.inner.api.stream_queue_buffer)(self.stream.0, self.raw.0) };
        }
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Buffer")
    }
}

/// Where the picture is. Every pointer is checked for null before it is read.
///
/// # Safety
/// `buffer` was dequeued from a stream and not queued back.
unsafe fn plane_of(buffer: *mut pw::pw_buffer) -> Option<Plane> {
    // SAFETY: the caller's promise covers the buffer.
    let inner = unsafe { (*buffer).buffer };
    // SAFETY: as above; its `spa_buffer` is null or live with it.
    let spa_buffer = unsafe { inner.as_ref() }?;
    // One plane: the offer asks only for layouts with no auxiliary plane, so more is not a picture we can import.
    if spa_buffer.n_datas != 1 || spa_buffer.datas.is_null() {
        return None;
    }
    // SAFETY: the caller's promise covers the buffer.
    if unsafe { header_of(buffer) }.is_some_and(|header| header.flags & spa::SPA_META_HEADER_FLAG_CORRUPTED != 0) {
        return None;
    }
    // SAFETY: checked above: `datas` holds one plane.
    let data = unsafe { &*spa_buffer.datas };
    // SAFETY: a plane's chunk is null or live with its buffer.
    let chunk = unsafe { data.chunk.as_ref() }?;
    if data.type_ != spa::SPA_DATA_DmaBuf || chunk.flags & spa::SPA_CHUNK_FLAG_CORRUPTED.cast_signed() != 0 {
        return None;
    }
    let stride = u32::try_from(chunk.stride).ok().filter(|s| *s > 0)?;
    let fd = RawFd::try_from(data.fd).ok()?;
    // SAFETY: a DMA-BUF plane's descriptor is open while the buffer is dequeued; the stream owns it, so it is copied.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let own = borrowed.try_clone_to_owned().ok()?;
    Some(Plane { fd: own, offset: chunk.offset, stride })
}

/// The bytes of a buffer in memory. Every pointer is checked for null, and the chunk against its memory, before a byte
/// is read.
///
/// # Safety
/// `buffer` was dequeued from a stream opened with mapped buffers and not queued back; the bytes live as long as that.
unsafe fn mapped_of<'a>(buffer: *mut pw::pw_buffer) -> Option<Mapped<'a>> {
    // SAFETY: the caller's promise covers the buffer.
    let inner = unsafe { (*buffer).buffer };
    // SAFETY: as above; its `spa_buffer` is null or live with it.
    let spa_buffer = unsafe { inner.as_ref() }?;
    if spa_buffer.n_datas < 1 || spa_buffer.datas.is_null() {
        return None;
    }
    // SAFETY: the caller's promise covers the buffer.
    if unsafe { header_of(buffer) }.is_some_and(|header| header.flags & spa::SPA_META_HEADER_FLAG_CORRUPTED != 0) {
        return None;
    }
    // SAFETY: checked above: `datas` holds at least one plane.
    let data = unsafe { &*spa_buffer.datas };
    // SAFETY: a plane's chunk is null or live with its buffer.
    let chunk = unsafe { data.chunk.as_ref() }?;
    let memory = data.type_ == spa::SPA_DATA_MemPtr || data.type_ == spa::SPA_DATA_MemFd;
    if !memory || data.data.is_null() || chunk.flags & spa::SPA_CHUNK_FLAG_CORRUPTED.cast_signed() != 0 {
        return None;
    }
    // A chunk's offset wraps within the plane's memory, as SPA's ring buffers say; its size never runs past it.
    let whole = data.maxsize as usize;
    let start = (chunk.offset as usize).checked_rem(whole)?;
    let length = (chunk.size as usize).min(whole - start);
    if length == 0 {
        return None;
    }
    // SAFETY: the daemon mapped `maxsize` bytes at `data` (`MAP_BUFFERS`); `start` lies inside them, checked above.
    let first = unsafe { data.data.cast::<u8>().add(start) };
    // SAFETY: the `length` bytes from `first` lie inside that mapping, alive while the buffer is dequeued.
    let bytes = unsafe { std::slice::from_raw_parts(first, length) };
    Some(Mapped { bytes, stride: u32::try_from(chunk.stride).ok().filter(|stride| *stride > 0) })
}

/// The buffer's header, if the producer wrote one. Every pointer is checked for null before it is read.
///
/// # Safety
/// `buffer` was dequeued from a stream and not queued back; the header lives as long as that.
unsafe fn header_of<'a>(buffer: *mut pw::pw_buffer) -> Option<&'a spa::spa_meta_header> {
    // SAFETY: the caller's promise covers the buffer.
    let inner = unsafe { (*buffer).buffer };
    // SAFETY: as above; its `spa_buffer` is null or live with it.
    let spa_buffer = unsafe { inner.as_ref() }?;
    if spa_buffer.metas.is_null() {
        return None;
    }
    (0..spa_buffer.n_metas as usize).find_map(|i| {
        // SAFETY: `metas` holds `n_metas` entries, live with the buffer.
        let meta_at = unsafe { spa_buffer.metas.add(i) };
        // SAFETY: as above.
        let meta = unsafe { &*meta_at };
        let whole = !meta.data.is_null() && meta.size as usize >= size_of::<spa::spa_meta_header>();
        // SAFETY: a header meta's non-null data holds at least a `spa_meta_header`, checked by size.
        (meta.type_ == spa::SPA_META_Header && whole).then(|| unsafe { &*meta.data.cast::<spa::spa_meta_header>() })
    })
}

/// What the C callbacks are handed back: the handler and the handles it needs.
struct Callbacks {
    handler: Box<dyn Handler>,
    inner: Arc<Inner>,
    stream: *mut pw::pw_stream,
}

unsafe extern "C" fn on_state_changed(
    data: *mut c_void,
    _old: pw::pw_stream_state,
    state: pw::pw_stream_state,
    _error: *const c_char,
) {
    // SAFETY: `data` is the `Callbacks` this stream was given, alive until the stream is destroyed.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    callbacks.handler.state_changed(State::of(state));
}

unsafe extern "C" fn on_param_changed(data: *mut c_void, id: u32, param: *const spa::spa_pod) {
    // SAFETY: as above.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    // SAFETY: a param PipeWire hands a callback is null or a pod live for that call.
    let header = unsafe { param.as_ref() };
    let bytes = header.map(|pod| {
        // SAFETY: a pod is its header and the `size` bytes after it.
        unsafe { std::slice::from_raw_parts(param.cast::<u8>(), size_of::<spa::spa_pod>() + pod.size as usize) }
    });
    let (inner, raw) = (callbacks.inner.clone(), callbacks.stream);
    callbacks.handler.param_changed(&StreamRef { inner: &inner, stream: raw }, id, bytes);
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: as above.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    let (inner, raw) = (callbacks.inner.clone(), callbacks.stream);
    callbacks.handler.process(&StreamRef { inner: &inner, stream: raw });
}

unsafe extern "C" fn on_add_buffer(data: *mut c_void, _buffer: *mut pw::pw_buffer) {
    // SAFETY: as above.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    callbacks.inner.added.fetch_add(1, Ordering::AcqRel);
}

unsafe extern "C" fn on_remove_buffer(data: *mut c_void, _buffer: *mut pw::pw_buffer) {
    // SAFETY: as above.
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    callbacks.inner.removed.fetch_add(1, Ordering::AcqRel);
}

/// What the source's node calls itself, once it has said. A global's announcement holds a few of its properties only,
/// so the node is bound and its own info read.
struct Naming {
    connection: Arc<Connection>,
    registry: Option<RegistryProxy>,
    node: u32,
    bound: Option<Proxy<Told>>,
    events: Lent<pw::pw_node_events>,
}

/// What the bound node has said of itself.
struct Told {
    name: Mutex<Option<String>>,
}

unsafe extern "C" fn on_source_global(
    data: *mut c_void,
    id: u32,
    _permissions: u32,
    _kind: *const c_char,
    _version: u32,
    _props: *const spa::spa_dict,
) {
    // SAFETY: `data` is the session's lent naming; registry callbacks run one at a time under the loop's lock.
    let naming = unsafe { &mut *data.cast::<Naming>() };
    if id != naming.node || naming.bound.is_some() {
        return;
    }
    let Some(registry) = &naming.registry else { return };
    let events = naming.events.as_ptr();
    let held = naming.connection.lock();
    let told = Told { name: Mutex::new(None) };
    naming.bound = held.bind(registry, id, interface::NODE, pw::PW_VERSION_NODE, told, |proxy, hook, data| {
        // SAFETY: the proxy was just made and the loop is locked; the hook, the events and the node's data are lent and
        // outlive it.
        unsafe { methods::listen_node(proxy, hook, events, data) }
    });
}

unsafe extern "C" fn on_source_info(data: *mut c_void, info: *const pw::pw_node_info) {
    // SAFETY: `data` is the bound node's lent data, alive until its proxy is taken down.
    let told = unsafe { &*data.cast::<Told>() };
    // SAFETY: `info` is PipeWire's for this call, or null.
    let Some(info) = (unsafe { info.as_ref() }) else { return };
    if info.change_mask & u64::from(pw::PW_NODE_CHANGE_MASK_PROPS) != 0 {
        // SAFETY: the node's dictionary, PipeWire's for this call.
        let props = unsafe { Borrowed::new(info.props) };
        *told.name.lock().unwrap_or_else(PoisonError::into_inner) = props.get("media.name").map(str::to_owned);
    }
}

/// A running capture stream. Dropping it stops the loop and takes the stream down.
pub struct Session {
    inner: Arc<Inner>,
    /// Taken down in `Drop`, under the lock, before anything it stands on.
    stream: Option<Stream<Callbacks>>,
    /// The registry, heard only for the source's name; before the naming it reaches.
    registry_listening: Option<Listening<pw::pw_registry_events>>,
    naming: Lent<Naming>,
    /// Last, so the connection goes after everything made on it.
    _connection: Arc<Connection>,
}

// SAFETY: every use of the lent naming is under the loop's lock, or in `Drop` after the listening that reaches it is
// gone.
unsafe impl Send for Session {}

impl Session {
    /// Opens `node` on `remote`, the portal's connection to PipeWire, as `role`, and offers `offers` (serialised
    /// format pods, the first preferred) with buffers mapped. Everything after this arrives at `handler`.
    pub fn open(
        remote: OwnedFd,
        node: u32,
        role: Role,
        offers: &[&[u8]],
        handler: impl Handler,
    ) -> Result<Self, Error> {
        let offers = offers.iter().map(|offer| aligned(offer)).collect::<Result<Vec<_>, _>>()?;
        let connection = Connection::open_fd(c"chatkord-capture", &STREAM, remote, "connect to the screen")?;
        let inner = Arc::new(Inner {
            api: connection.api(),
            looper: connection.looper(),
            closed: AtomicBool::new(false),
            removed: AtomicU64::new(0),
            added: AtomicU64::new(0),
        });
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_stream_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_STREAM_EVENTS;
        events.state_changed = Some(on_state_changed);
        events.param_changed = Some(on_param_changed);
        events.process = Some(on_process);
        events.add_buffer = Some(on_add_buffer);
        events.remove_buffer = Some(on_remove_buffer);
        // SAFETY: as for the stream's events.
        let mut registry_events: pw::pw_registry_events = unsafe { std::mem::zeroed() };
        registry_events.version = pw::PW_VERSION_REGISTRY_EVENTS;
        registry_events.global = Some(on_source_global);
        // SAFETY: as above.
        let mut node_events: pw::pw_node_events = unsafe { std::mem::zeroed() };
        node_events.version = pw::PW_VERSION_NODE_EVENTS;
        node_events.info = Some(on_source_info);

        let mut session = Self {
            inner: Arc::clone(&inner),
            stream: None,
            naming: Lent::new(Naming {
                connection: Arc::clone(&connection),
                registry: None,
                node,
                bound: None,
                events: Lent::new(node_events),
            }),
            registry_listening: None,
            _connection: Arc::clone(&connection),
        };
        // From here, dropping `session` takes down whatever has been made.
        let held = connection.lock();
        // The source's name is only told, never needed: a registry that will not listen leaves it unknown. Taken
        // first, it is the connection's id 2, as the portal's own registry was: that registry's events still queued on
        // the socket the portal hands over then decode as globals, where another id would refuse each one.
        if let Ok(registry) = held.registry() {
            let data = session.naming.as_ptr().cast::<c_void>();
            // SAFETY: the naming is lent and outlives the listening, which `Session` drops first.
            session.registry_listening = unsafe { held.listen_registry(&registry, registry_events, data) }.ok();
            // SAFETY: no registry callback reads the naming before the loop's lock is let go, which this holds.
            unsafe { (*session.naming.as_ptr()).registry = Some(registry) };
        }
        let stream = held.stream(
            c"chatkord-capture",
            &Properties::Text(role.properties()),
            events,
            "make a stream",
            |stream| Callbacks { handler: Box::new(handler), inner: Arc::clone(&inner), stream },
        )?;
        let mut params: Vec<*const spa::spa_pod> = offers.iter().map(|offer| offer.as_ptr().cast()).collect();
        let flags = pw::pw_stream_flags_PW_STREAM_FLAG_AUTOCONNECT | pw::pw_stream_flags_PW_STREAM_FLAG_MAP_BUFFERS;
        if !stream.connect(&held, spa::SPA_DIRECTION_INPUT, node, flags, &mut params) {
            return Err(Error::Refused("connect the stream"));
        }
        session.stream = Some(stream);
        drop(held);
        Ok(session)
    }

    /// What the source's node calls itself (`media.name`), once the daemon has said: KWin names a window's stream
    /// after the window's app.
    pub fn source_name(&self) -> Option<String> {
        let _lock = self.inner.lock();
        // SAFETY: the naming is written only by the registry's callbacks, which run under this same lock.
        let naming = unsafe { &*self.naming.as_ptr() };
        let told = naming.bound.as_ref()?.data();
        told.name.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _lock = self.inner.lock();
        self.stream = None;
        self.registry_listening = None;
        // SAFETY: the naming is lent, and written only by callbacks that run under this same lock.
        let naming = unsafe { &mut *self.naming.as_ptr() };
        naming.bound = None;
        naming.registry = None;
        self.inner.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pod_is_copied_whole_and_aligned() {
        let mut pod = 16u32.to_ne_bytes().to_vec();
        pod.extend_from_slice(&[7; 20]);
        let words = aligned(&pod).unwrap();
        assert_eq!(words.len(), 3);
        assert_eq!(words.as_ptr() as usize % 8, 0);
    }

    #[test]
    fn a_pod_shorter_than_it_says_is_refused() {
        let mut pod = 64u32.to_ne_bytes().to_vec();
        pod.extend_from_slice(&[0; 20]);
        assert_eq!(aligned(&pod), Err(Error::Pod));
        assert_eq!(aligned(&[1, 2]), Err(Error::Pod));
        assert_eq!(aligned(&u32::MAX.to_ne_bytes()), Err(Error::Pod));
    }

    #[test]
    fn every_state_has_a_meaning() {
        assert_eq!(State::of(pw::pw_stream_state_PW_STREAM_STATE_STREAMING), State::Streaming);
        assert_eq!(State::of(pw::pw_stream_state_PW_STREAM_STATE_ERROR), State::Error);
        assert_eq!(State::of(pw::pw_stream_state_PW_STREAM_STATE_UNCONNECTED), State::Unconnected);
    }

    mod lab {
        use super::super::*;

        struct Deaf;

        impl Handler for Deaf {
            fn state_changed(&mut self, _: State) {}
            fn param_changed(&mut self, _: &StreamRef<'_>, _: u32, _: Option<&[u8]>) {}
            fn process(&mut self, _: &StreamRef<'_>) {}
        }

        /// The portal hands over the connection its own registry was id 2 on, with that registry's events maybe
        /// still queued: ours takes the id, so they decode as globals and not as another proxy's refused messages.
        #[test]
        fn a_session_s_registry_is_its_connection_s_first_proxy() {
            let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else { return tier::skipped("no runtime directory") };
            let path = std::path::Path::new(&dir).join("pipewire-0");
            let Ok(socket) = std::os::unix::net::UnixStream::connect(path) else {
                return tier::skipped("no PipeWire daemon here");
            };
            // A node no daemon has, so nothing is linked to the stream.
            let nowhere = u32::MAX - 1;
            let session = Session::open(OwnedFd::from(socket), nowhere, Role::Screen, &[], Deaf).expect("a session");
            // SAFETY: the library is the one the session's own calls were loaded from.
            let library = unsafe { libloading::Library::new("libpipewire-0.3.so.0") }.unwrap();
            // SAFETY: `pw_proxy_get_id` is declared so in `pipewire/proxy.h`.
            let id_of: libloading::Symbol<'_, unsafe extern "C" fn(*mut pw::pw_proxy) -> u32> =
                unsafe { library.get(b"pw_proxy_get_id\0") }.unwrap();
            let _lock = session.inner.lock();
            // SAFETY: the naming is written only under the loop's lock, which is held.
            let registry = unsafe { (*session.naming.as_ptr()).registry.as_ref() }.expect("a registry");
            // SAFETY: the registry is a live proxy of the session, whose loop is locked.
            let id = unsafe { id_of(registry.raw().cast()) };
            assert_eq!(id, 2, "the registry is the first proxy after the core (0) and the client (1)");
        }
    }
}
