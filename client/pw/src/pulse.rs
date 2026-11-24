//! The sound server's own list of speakers and microphones (`pa_context_get_{sink,source}_info_list`, monitors left
//! out), each by the name a stream is opened on and the description a person reads.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use libloading::Library;

use crate::library::Unavailable;

/// The library every distribution ships for PulseAudio's client ABI, which PipeWire's server speaks too.
const NAME: &str = "libpulse.so.0";

/// How long a listing may take, connecting included: the module's own asks wait without a bound.
const BOUND: Duration = Duration::from_secs(2);

/// `PA_INVALID_INDEX`: a source that is no sink's monitor.
const NO_SINK: u32 = u32::MAX;

/// `pa_context_state_t`'s values that end the wait to connect.
const CONTEXT_READY: c_int = 4;
const CONTEXT_FAILED: c_int = 5;
const CONTEXT_TERMINATED: c_int = 6;
/// `pa_operation_state_t::PA_OPERATION_RUNNING`.
const OPERATION_RUNNING: c_int = 0;

/// One device as the sound server lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The server's unique name for it, which PipeWire calls `node.name`: what a device is chosen by.
    pub name: String,
    /// What a person reads, and what libwebrtc's module calls it.
    pub description: String,
}

/// The speakers and the microphones, each in the server's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The sinks.
    pub speakers: Vec<Listed>,
    /// The sources that are no sink's monitor.
    pub microphones: Vec<Listed>,
    /// The name of the sink the server plays to by default, when it has one.
    pub default_speaker: Option<String>,
    /// The name of the source it records from by default, when it has one.
    pub default_microphone: Option<String>,
}

/// Why there is no listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseError {
    /// The library is not on this device, or lacks a function.
    Unavailable(Unavailable),
    /// No sound server answered.
    Unreached,
    /// The server did not finish within the bound.
    TimedOut,
}

impl fmt::Display for PulseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "{why}"),
            Self::Unreached => write!(f, "no sound server answered {NAME}"),
            Self::TimedOut => write!(f, "the sound server did not list its devices within {BOUND:?}"),
        }
    }
}

impl std::error::Error for PulseError {}

/// `pa_sample_spec`.
#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

/// `pa_channel_map`, `PA_CHANNELS_MAX` positions.
#[repr(C)]
struct ChannelMap {
    channels: u8,
    map: [c_int; 32],
}

/// `pa_cvolume`.
#[repr(C)]
struct Volume {
    channels: u8,
    values: [u32; 32],
}

/// The leading fields of `pa_sink_info`, which have not moved since libpulse 0.9; the rest is never read.
#[repr(C)]
struct SinkInfo {
    name: *const c_char,
    index: u32,
    description: *const c_char,
}

/// The leading fields of `pa_source_info`, up to the one that tells a monitor.
#[repr(C)]
struct SourceInfo {
    name: *const c_char,
    index: u32,
    description: *const c_char,
    sample_spec: SampleSpec,
    channel_map: ChannelMap,
    owner_module: u32,
    volume: Volume,
    mute: c_int,
    monitor_of_sink: u32,
}

/// The leading fields of `pa_server_info`, up to the default devices' names; the rest is never read.
#[repr(C)]
struct ServerInfo {
    user_name: *const c_char,
    host_name: *const c_char,
    server_version: *const c_char,
    server_name: *const c_char,
    sample_spec: SampleSpec,
    default_sink_name: *const c_char,
    default_source_name: *const c_char,
}

type Opaque = c_void;
type SinkCallback = unsafe extern "C" fn(*mut Opaque, *const SinkInfo, c_int, *mut c_void);
type SourceCallback = unsafe extern "C" fn(*mut Opaque, *const SourceInfo, c_int, *mut c_void);
type ServerCallback = unsafe extern "C" fn(*mut Opaque, *const ServerInfo, *mut c_void);

/// The functions of `libpulse` a listing calls, each its C signature, resolved from our own handle.
struct Api {
    mainloop_new: unsafe extern "C" fn() -> *mut Opaque,
    mainloop_get_api: unsafe extern "C" fn(*mut Opaque) -> *mut Opaque,
    mainloop_prepare: unsafe extern "C" fn(*mut Opaque, c_int) -> c_int,
    mainloop_poll: unsafe extern "C" fn(*mut Opaque) -> c_int,
    mainloop_dispatch: unsafe extern "C" fn(*mut Opaque) -> c_int,
    mainloop_free: unsafe extern "C" fn(*mut Opaque),
    context_new: unsafe extern "C" fn(*mut Opaque, *const c_char) -> *mut Opaque,
    context_connect: unsafe extern "C" fn(*mut Opaque, *const c_char, c_int, *const c_void) -> c_int,
    context_get_state: unsafe extern "C" fn(*mut Opaque) -> c_int,
    context_disconnect: unsafe extern "C" fn(*mut Opaque),
    context_unref: unsafe extern "C" fn(*mut Opaque),
    get_sink_info_list: unsafe extern "C" fn(*mut Opaque, SinkCallback, *mut c_void) -> *mut Opaque,
    get_source_info_list: unsafe extern "C" fn(*mut Opaque, SourceCallback, *mut c_void) -> *mut Opaque,
    get_server_info: unsafe extern "C" fn(*mut Opaque, ServerCallback, *mut c_void) -> *mut Opaque,
    operation_get_state: unsafe extern "C" fn(*mut Opaque) -> c_int,
    operation_unref: unsafe extern "C" fn(*mut Opaque),
    /// Kept last so the functions above never outlive the library they point into.
    _library: Library,
}

fn function<T: Copy>(library: &Library, name: &'static str) -> Result<T, Unavailable> {
    // SAFETY: `T` is the C signature of `name`; the pointer is copied out and the library is kept alive beside it for
    // the life of the process.
    unsafe { library.get::<T>(format!("{name}\0").as_bytes()) }
        .map(|symbol| *symbol)
        .map_err(|_| Unavailable::TooOld(name))
}

fn load() -> Result<Api, Unavailable> {
    // SAFETY: loading a shared library runs its initialisers; this is the system's PulseAudio client library, which is
    // built to be loaded by any program.
    let library = unsafe { Library::new(NAME) }.map_err(|_| Unavailable::NotInstalled)?;
    Ok(Api {
        mainloop_new: function(&library, "pa_mainloop_new")?,
        mainloop_get_api: function(&library, "pa_mainloop_get_api")?,
        mainloop_prepare: function(&library, "pa_mainloop_prepare")?,
        mainloop_poll: function(&library, "pa_mainloop_poll")?,
        mainloop_dispatch: function(&library, "pa_mainloop_dispatch")?,
        mainloop_free: function(&library, "pa_mainloop_free")?,
        context_new: function(&library, "pa_context_new")?,
        context_connect: function(&library, "pa_context_connect")?,
        context_get_state: function(&library, "pa_context_get_state")?,
        context_disconnect: function(&library, "pa_context_disconnect")?,
        context_unref: function(&library, "pa_context_unref")?,
        get_sink_info_list: function(&library, "pa_context_get_sink_info_list")?,
        get_source_info_list: function(&library, "pa_context_get_source_info_list")?,
        get_server_info: function(&library, "pa_context_get_server_info")?,
        operation_get_state: function(&library, "pa_operation_get_state")?,
        operation_unref: function(&library, "pa_operation_unref")?,
        _library: library,
    })
}

fn api() -> Result<&'static Api, Unavailable> {
    static API: OnceLock<Result<Api, Unavailable>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(|why| *why)
}

/// One connection to the server, its loop with it, for one listing.
struct Connection {
    api: &'static Api,
    mainloop: *mut Opaque,
    context: *mut Opaque,
    until: Instant,
}

impl Connection {
    fn open(api: &'static Api) -> Result<Self, PulseError> {
        // SAFETY: a new loop owns nothing yet.
        let mainloop = unsafe { (api.mainloop_new)() };
        if mainloop.is_null() {
            return Err(PulseError::Unreached);
        }
        let mut connection = Self { api, mainloop, context: std::ptr::null_mut(), until: Instant::now() + BOUND };
        // SAFETY: the loop is live; its vtable is the loop's own and lives as long as it does.
        let vtable = unsafe { (api.mainloop_get_api)(mainloop) };
        // SAFETY: the vtable is the loop's; the name is a C string the call copies.
        connection.context = unsafe { (api.context_new)(vtable, c"chatkord-devices".as_ptr()) };
        if connection.context.is_null() {
            return Err(PulseError::Unreached);
        }
        // SAFETY: a new context; no server named, no flags, no spawn api: the default server, never started for us.
        if unsafe { (api.context_connect)(connection.context, std::ptr::null(), 0, std::ptr::null()) } < 0 {
            return Err(PulseError::Unreached);
        }
        loop {
            // SAFETY: the context is live.
            match unsafe { (api.context_get_state)(connection.context) } {
                CONTEXT_READY => return Ok(connection),
                CONTEXT_FAILED | CONTEXT_TERMINATED => return Err(PulseError::Unreached),
                _ => connection.turn()?,
            }
        }
    }

    /// Runs the loop once, waiting no later than the bound.
    fn turn(&self) -> Result<(), PulseError> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(PulseError::TimedOut);
        }
        let micros = c_int::try_from(left.as_micros()).unwrap_or(c_int::MAX);
        // SAFETY: the loop is live and run only from this thread.
        let turned = unsafe { (self.api.mainloop_prepare)(self.mainloop, micros) } >= 0
            // SAFETY: as above, after a prepare.
            && unsafe { (self.api.mainloop_poll)(self.mainloop) } >= 0
            // SAFETY: as above, after a poll.
            && unsafe { (self.api.mainloop_dispatch)(self.mainloop) } >= 0;
        turned.then_some(()).ok_or(PulseError::Unreached)
    }

    /// Runs the loop until `operation` is done, then lets it go.
    fn finish(&self, operation: *mut Opaque) -> Result<(), PulseError> {
        if operation.is_null() {
            return Err(PulseError::Unreached);
        }
        let mut done = Ok(());
        // SAFETY: the operation is live until unreferenced below.
        while unsafe { (self.api.operation_get_state)(operation) } == OPERATION_RUNNING {
            if let Err(why) = self.turn() {
                done = Err(why);
                break;
            }
        }
        // SAFETY: the reference the list call handed us, released once.
        unsafe { (self.api.operation_unref)(operation) };
        done
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if !self.context.is_null() {
            // SAFETY: a context this connection made; disconnecting first ends any operation still pending.
            unsafe { (self.api.context_disconnect)(self.context) };
            // SAFETY: the one reference `context_new` handed us, released once.
            unsafe { (self.api.context_unref)(self.context) };
        }
        // SAFETY: the loop this connection made; nothing runs on it now.
        unsafe { (self.api.mainloop_free)(self.mainloop) };
    }
}

/// A C string the server handed us, as text; none for a null.
///
/// # Safety
/// `text` is null or a NUL-terminated string that lives for the call.
unsafe fn text(text: *const c_char) -> Option<String> {
    // SAFETY: the caller's.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
}

unsafe extern "C" fn on_sink(_: *mut Opaque, info: *const SinkInfo, eol: c_int, data: *mut c_void) {
    if eol != 0 || info.is_null() {
        return;
    }
    // SAFETY: `info` is the server's, valid for the callback.
    let info = unsafe { &*info };
    // SAFETY: the server's strings live for the callback.
    let (Some(name), Some(description)) = (unsafe { text(info.name) }, unsafe { text(info.description) }) else {
        return;
    };
    // SAFETY: `data` is the `Vec` the listing lent for this operation, borrowed by nothing else while it runs.
    unsafe { &mut *data.cast::<Vec<Listed>>() }.push(Listed { name, description });
}

unsafe extern "C" fn on_source(_: *mut Opaque, info: *const SourceInfo, eol: c_int, data: *mut c_void) {
    if eol != 0 || info.is_null() {
        return;
    }
    // SAFETY: `info` is the server's, valid for the callback.
    let info = unsafe { &*info };
    if info.monitor_of_sink != NO_SINK {
        return;
    }
    // SAFETY: the server's strings live for the callback.
    let (Some(name), Some(description)) = (unsafe { text(info.name) }, unsafe { text(info.description) }) else {
        return;
    };
    // SAFETY: `data` is the `Vec` the listing lent for this operation, borrowed by nothing else while it runs.
    unsafe { &mut *data.cast::<Vec<Listed>>() }.push(Listed { name, description });
}

unsafe extern "C" fn on_server(_: *mut Opaque, info: *const ServerInfo, data: *mut c_void) {
    // SAFETY: `info` is the server's, valid for the callback, or null.
    let Some(info) = (unsafe { info.as_ref() }) else { return };
    // SAFETY: `data` is the listing lent for this operation, borrowed by nothing else while it runs.
    let listing = unsafe { &mut *data.cast::<Listing>() };
    // SAFETY: the server's strings live for the callback.
    listing.default_speaker = unsafe { text(info.default_sink_name) };
    // SAFETY: as above.
    listing.default_microphone = unsafe { text(info.default_source_name) };
}

/// The server's speakers and microphones now, and which of each is its default. It blocks for up to two seconds: run it on the blocking pool.
pub fn devices() -> Result<Listing, PulseError> {
    let connection = Connection::open(api().map_err(PulseError::Unavailable)?)?;
    let api = connection.api;
    let mut listing = Listing::default();
    let speakers = (&raw mut listing.speakers).cast::<c_void>();
    // SAFETY: the context is ready; `speakers` outlives the operation, which `finish` runs to its end.
    connection.finish(unsafe { (api.get_sink_info_list)(connection.context, on_sink, speakers) })?;
    let microphones = (&raw mut listing.microphones).cast::<c_void>();
    // SAFETY: as above, for `microphones`.
    connection.finish(unsafe { (api.get_source_info_list)(connection.context, on_source, microphones) })?;
    let whole = (&raw mut listing).cast::<c_void>();
    // SAFETY: as above, for the whole listing, which nothing else borrows while the operation runs.
    connection.finish(unsafe { (api.get_server_info)(connection.context, on_server, whole) })?;
    Ok(listing)
}
