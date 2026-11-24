//! A connection to the sound server and everything made on it: the thread loop, the context and the core, and the
//! streams, proxies and listeners that must go before them. This and the loop, library and hook modules are the only
//! ones that hold a raw PipeWire pointer for longer than a call; each owner here frees what it made under the loop's
//! lock, in the order PipeWire needs.

use std::ffi::{CStr, c_int, c_void};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::dict::Owned;
use crate::lent::Lent;
use crate::library::{Api, api};
use crate::looper::{self, Looper, REAL_TIME, STREAM};
use crate::methods;
use crate::session::Error;

/// Linux's `EPIPE`: the core's error when the server closed the connection.
const EPIPE: c_int = 32;

/// Whether a core error ends the connection, as PipeWire's own clients read it; the rest (a stream's node gone, an
/// ask about it refused) are that stream's, which says so in its state.
pub(crate) fn ended(id: u32, res: c_int) -> bool {
    id == pw::PW_ID_CORE && res == -EPIPE
}

/// A connection to the sound server: its loop's thread, its context with `modules` loaded and its core. Dropping the
/// last holder disconnects the core, destroys the context, then stops and destroys the loop.
pub(crate) struct Connection {
    /// Set by the core's error listener when the server ended the connection.
    lost: Lent<AtomicBool>,
    context: *mut pw::pw_context,
    core: *mut pw::pw_core,
    real_time: bool,
    listening: Option<Listening<pw::pw_core_events>>,
    /// Last, so the loop is destroyed after everything made on it.
    looper: Arc<Looper>,
}

// SAFETY: the context and core are only used under the loop's lock, or in `Drop` once no stream, proxy or listener
// made on them is left; `lost` is an atomic.
unsafe impl Send for Connection {}
// SAFETY: as above.
unsafe impl Sync for Connection {}

impl Connection {
    /// A connection of its own to the daemon, with `modules` loaded, and the real-time module offered when
    /// `real_time`: a stream whose callback carries a call's sound runs ahead of ordinary work only through it.
    pub(crate) fn open(name: &CStr, modules: &[&CStr], real_time: bool) -> Result<Arc<Self>, Error> {
        Self::connect(name, modules, None, real_time, "reach the sound server")
    }

    /// A connection over `remote`, the descriptor a portal handed over, with `modules` loaded; `refused` names the step
    /// a connection PipeWire does not make fails at.
    pub(crate) fn open_fd(
        name: &CStr,
        modules: &[&CStr],
        remote: OwnedFd,
        refused: &'static str,
    ) -> Result<Arc<Self>, Error> {
        Self::connect(name, modules, Some(remote), false, refused)
    }

    /// The connection a call's sound shares: the voice's microphone and speaker and the window's chimes. Made when
    /// none lives or the one that does was ended by the server, and dropped with its last holder.
    pub(crate) fn shared() -> Result<Arc<Self>, Error> {
        static SHARED: Mutex<Weak<Connection>> = Mutex::new(Weak::new());
        let mut held = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(live) = held.upgrade().filter(|live| !live.lost()) {
            return Ok(live);
        }
        let made = Self::connect(c"chatkord-sound", &STREAM, None, true, "reach the sound server")?;
        *held = Arc::downgrade(&made);
        Ok(made)
    }

    fn connect(
        name: &CStr,
        modules: &[&CStr],
        remote: Option<OwnedFd>,
        real_time: bool,
        refused: &'static str,
    ) -> Result<Arc<Self>, Error> {
        let api = api()?;
        let looper = Arc::new(Looper::new(api, name)?);
        let mut connection = Self {
            lost: Lent::new(AtomicBool::new(false)),
            context: std::ptr::null_mut(),
            core: std::ptr::null_mut(),
            real_time: false,
            listening: None,
            looper,
        };
        connection.looper.start()?;
        let lock = connection.looper.lock();
        connection.context = connection.looper.context(&lock, modules)?;
        if real_time {
            connection.real_time = connection.looper.offer(&lock, connection.context, REAL_TIME);
        }
        connection.core = match remote {
            // SAFETY: the context was just made on the locked loop; no properties, and no user data.
            None => unsafe { (api.context_connect)(connection.context, std::ptr::null_mut(), 0) },
            Some(remote) => {
                let fd = remote.into_raw_fd();
                // SAFETY: the context was just made on the locked loop; the descriptor is ours.
                let core = unsafe { (api.context_connect_fd)(connection.context, fd, std::ptr::null_mut(), 0) };
                if core.is_null() {
                    // PipeWire closes the descriptor only with a connection it made, and a connection it fails to
                    // make leaves it open (read in 1.6.10's `pw_context_connect_fd`), so it is still ours to close.
                    // SAFETY: `fd` came from `into_raw_fd` above and nothing else owns it now.
                    drop(unsafe { OwnedFd::from_raw_fd(fd) });
                }
                core
            }
        };
        if connection.core.is_null() {
            return Err(Error::Refused(refused));
        }
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_core_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_CORE_EVENTS;
        events.error = Some(on_core_error);
        let data = connection.lost.as_ptr().cast::<c_void>();
        // SAFETY: the core is connected and the loop locked; `lost` is lent for as long as the connection, and the
        // listener is removed with the core.
        connection.listening = Some(unsafe { listen_core(&connection.looper, connection.core, events, data) }?);
        drop(lock);
        Ok(Arc::new(connection))
    }

    /// Takes the loop's lock until the guard drops; the lock is recursive, so a callback may take it again.
    pub(crate) fn lock(self: &Arc<Self>) -> Locked<'_> {
        Locked { connection: self, _held: self.looper.lock() }
    }

    /// Whether the server ended this connection: everything made on it has stopped for good.
    pub(crate) fn lost(&self) -> bool {
        // SAFETY: `lost` is lent for as long as the connection, and only ever read through a shared reference.
        unsafe { &*self.lost.as_ptr() }.load(Ordering::Acquire)
    }

    /// Whether PipeWire's real-time module was there to load, so the thread a stream's sound is handed over on asks to
    /// run ahead of ordinary work.
    pub(crate) fn real_time(&self) -> bool {
        self.real_time
    }

    pub(crate) fn api(&self) -> &'static Api {
        self.looper.api
    }

    /// The loop's lock, for what outlives the connection's holders and must still be taken under it.
    pub(crate) fn looper(&self) -> Arc<Looper> {
        Arc::clone(&self.looper)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        {
            let _lock = self.looper.lock();
            // The hook is unlinked from the core's list before disconnecting frees that list (ASan, PipeWire 1.6).
            drop(self.listening.take());
            if !self.core.is_null() {
                // SAFETY: a core this connection made; everything made on it went with its owner, which held this,
                // and its listener is gone, so `lost` is not written after this.
                unsafe { (self.looper.api.core_disconnect)(self.core) };
            }
            if !self.context.is_null() {
                // SAFETY: a context this connection made and has not freed.
                unsafe { (self.looper.api.context_destroy)(self.context) };
            }
        }
        self.looper.stop();
    }
}

unsafe extern "C" fn on_core_error(
    data: *mut c_void,
    id: u32,
    _seq: c_int,
    res: c_int,
    _message: *const std::ffi::c_char,
) {
    if ended(id, res) {
        // SAFETY: `data` is the lent `lost`, alive until the core is disconnected, which removes this listener.
        unsafe { &*data.cast::<AtomicBool>() }.store(true, Ordering::Release);
    }
}

/// Adds `events` as a listener of `core`.
///
/// # Safety
/// `core` is connected, its loop (`looper`) is locked, and `data` outlives the returned listening.
unsafe fn listen_core(
    looper: &Arc<Looper>,
    core: *mut pw::pw_core,
    events: pw::pw_core_events,
    data: *mut c_void,
) -> Result<Listening<pw::pw_core_events>, Error> {
    let listening = Listening::new(looper, events);
    // SAFETY: as the caller says; the hook and the events are lent and outlive the core's listener list entry, which
    // `Listening` takes out first.
    listening.listener.add(|hook| unsafe { methods::listen_core(core, hook, listening.events.as_ptr(), data) })?;
    Ok(listening)
}

/// The loop's lock, held until this drops, and the connection it is the lock of: what streams, proxies and listeners
/// are made through.
pub(crate) struct Locked<'c> {
    connection: &'c Arc<Connection>,
    _held: looper::Locked<'c>,
}

/// Where a stream's node properties come from.
pub(crate) enum Properties<'a> {
    /// `key=value` pairs.
    Pairs(&'a [(&'a CStr, String)]),
    /// A whitespace-separated `key=value` list.
    Text(&'a CStr),
}

impl Locked<'_> {
    /// A stream named `name` on this connection, its node made from `properties`, listening with `events`. `callbacks`
    /// builds what they are handed from the stream, so none runs before it knows it; `refused` names the step that
    /// failed.
    pub(crate) fn stream<T>(
        &self,
        name: &CStr,
        properties: &Properties<'_>,
        events: pw::pw_stream_events,
        refused: &'static str,
        callbacks: impl FnOnce(*mut pw::pw_stream) -> T,
    ) -> Result<Stream<T>, Error> {
        let api = self.connection.api();
        let props = match properties {
            Properties::Pairs(pairs) => {
                let owned = Owned::new(pairs)?;
                let raw = owned.raw();
                // SAFETY: the dictionary is live for this call, which copies it.
                unsafe { (api.properties_new_dict)(&raw const raw) }
            }
            // SAFETY: the properties are a NUL-terminated `key=value` list.
            Properties::Text(text) => unsafe { (api.properties_new_string)(text.as_ptr()) },
        };
        if props.is_null() {
            return Err(Error::Refused(refused));
        }
        // SAFETY: `pw_stream_new` takes the properties; the core is connected and this holds its loop's lock.
        let raw = unsafe { (api.stream_new)(self.connection.core, name.as_ptr(), props) };
        if raw.is_null() {
            return Err(Error::Refused(refused));
        }
        let stream = Stream {
            raw,
            callbacks: Lent::new(callbacks(raw)),
            events: Lent::new(events),
            listener: Listener::new(&self.connection.looper),
            connection: Arc::clone(self.connection),
        };
        let data = stream.callbacks.as_ptr().cast::<c_void>();
        // SAFETY: the stream was just made and the loop is locked; the hook, the events and the callbacks are lent and
        // outlive the stream.
        stream.listener.add(|hook| unsafe { (api.stream_add_listener)(raw, hook, stream.events.as_ptr(), data) });
        Ok(stream)
    }

    /// The daemon's registry on this connection. Taken first, it is the connection's first proxy.
    pub(crate) fn registry(&self) -> Result<RegistryProxy, Error> {
        // SAFETY: the core is connected, and this holds its loop's lock.
        let raw = unsafe { methods::registry_of(self.connection.core) };
        if raw.is_null() {
            return Err(Error::Refused("ask for the registry"));
        }
        Ok(RegistryProxy { raw, connection: Arc::clone(self.connection) })
    }

    /// Hears the registry with `events`.
    ///
    /// # Safety
    /// `data` outlives the returned listening.
    pub(crate) unsafe fn listen_registry(
        &self,
        registry: &RegistryProxy,
        events: pw::pw_registry_events,
        data: *mut c_void,
    ) -> Result<Listening<pw::pw_registry_events>, Error> {
        let listening = Listening::new(&self.connection.looper, events);
        let events_at = listening.events.as_ptr();
        listening.listener.add(|hook| {
            // SAFETY: the registry is live, the loop is locked, and the hook and the events are lent and outlive its
            // listener entry, which `Listening` takes out first; `data` as the caller says.
            unsafe { methods::listen_registry(registry.raw, hook, events_at, data) }
        })?;
        Ok(listening)
    }

    /// Hears the core with `events`.
    ///
    /// # Safety
    /// `data` outlives the returned listening.
    pub(crate) unsafe fn listen_core(
        &self,
        events: pw::pw_core_events,
        data: *mut c_void,
    ) -> Result<Listening<pw::pw_core_events>, Error> {
        // SAFETY: the core is connected and this holds its loop's lock; `data` as the caller says.
        unsafe { listen_core(&self.connection.looper, self.connection.core, events, data) }
    }

    /// Binds the global `id` as an object of `kind` at `version`, holding `data` for `listen` to hand its callbacks.
    /// `listen` adds the proxy's listener with the hook and the data it is given, and says whether the proxy took it.
    pub(crate) fn bind<T>(
        &self,
        registry: &RegistryProxy,
        id: u32,
        kind: &CStr,
        version: u32,
        data: T,
        listen: impl FnOnce(*mut pw::pw_proxy, *mut spa::spa_hook, *mut c_void) -> Result<(), Error>,
    ) -> Option<Proxy<T>> {
        // SAFETY: the registry is live and this holds its loop's lock.
        let raw = unsafe { methods::bind(registry.raw, id, kind, version) };
        self.attach(raw, data, listen)
    }

    /// Asks the daemon's `factory` for an object of `kind` at `version`, made with `props`; the rest as
    /// [`Locked::bind`]. `None` when the ask could not even be sent.
    pub(crate) fn create<T>(
        &self,
        factory: &CStr,
        kind: &CStr,
        version: u32,
        props: &Owned,
        data: T,
        listen: impl FnOnce(*mut pw::pw_proxy, *mut spa::spa_hook, *mut c_void) -> Result<(), Error>,
    ) -> Option<Proxy<T>> {
        let raw_props = props.raw();
        // SAFETY: the core is connected and this holds its loop's lock; the factory copies the dictionary.
        let raw = unsafe { methods::create_object(self.connection.core, factory, kind, version, &raw_props) };
        self.attach(raw, data, listen)
    }

    fn attach<T>(
        &self,
        raw: *mut pw::pw_proxy,
        data: T,
        listen: impl FnOnce(*mut pw::pw_proxy, *mut spa::spa_hook, *mut c_void) -> Result<(), Error>,
    ) -> Option<Proxy<T>> {
        if raw.is_null() {
            return None;
        }
        let proxy = Proxy {
            raw,
            data: Lent::new(data),
            listener: Listener::new(&self.connection.looper),
            connection: Arc::clone(self.connection),
        };
        // `proxy` is dropped on a refusal, which takes it down as any other.
        proxy.listener.add(|hook| listen(raw, hook, proxy.data.as_ptr().cast())).ok()?;
        Some(proxy)
    }

    /// Asks the daemon to answer `seq` with the core's `done` once it has answered everything asked before it; the
    /// sequence number `done` will carry, or an error's negative number.
    pub(crate) fn sync(&self, seq: c_int) -> c_int {
        // SAFETY: the core is connected and this holds its loop's lock.
        unsafe { methods::sync(self.connection.core, seq) }
    }
}

/// A hook PipeWire keeps in a list of an object's listeners, at an address that never moves, and taken out of that list
/// when this drops, as `spa_hook_remove` would: an object PipeWire destroyed has done so already, which the hook's own
/// `removed` callback records, so taking it out twice is harmless.
pub(crate) struct Listener {
    hook: Lent<spa::spa_hook>,
    looper: Arc<Looper>,
}

impl Listener {
    fn new(looper: &Arc<Looper>) -> Self {
        // SAFETY: plain data; zeroed is a hook in no list.
        let hook: spa::spa_hook = unsafe { std::mem::zeroed() };
        Self { hook: Lent::new(hook), looper: Arc::clone(looper) }
    }

    /// Hands the hook to `add`, an `add_listener` made under the loop's lock, then has a removal by PipeWire's own
    /// list clean leave it unlinked: adding zeroes the hook (`spa_hook_list_append`), so this is set after.
    fn add<R>(&self, add: impl FnOnce(*mut spa::spa_hook) -> R) -> R {
        let hook = self.hook.as_ptr();
        let added = add(hook);
        // SAFETY: the lent hook, live as long as `self`; the loop is locked, so no list walks it meanwhile.
        unsafe { (*hook).removed = Some(unlinked) };
        added
    }

    /// Takes the hook out of its list if it is still in one.
    fn detach(&self) {
        let _lock = self.looper.lock();
        // SAFETY: the hook is zeroed or added by `add`; one PipeWire took out, or this did before, is unlinked; the
        // loop is locked.
        unsafe { crate::hook::remove(self.hook.as_ptr()) };
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.detach();
    }
}

/// A hook's `removed`: whoever took it out of its list, it is in none now.
unsafe extern "C" fn unlinked(hook: *mut spa::spa_hook) {
    // SAFETY: `hook` is the lent one this was set on, live while it is removed.
    unsafe { (*hook).link = spa::spa_list { prev: std::ptr::null_mut(), next: std::ptr::null_mut() } };
}

/// An event table and the listener that has PipeWire call it, kept together so the table outlives the listener.
pub(crate) struct Listening<E> {
    /// First, so the hook is taken out before the table goes.
    listener: Listener,
    events: Lent<E>,
}

impl<E> Listening<E> {
    fn new(looper: &Arc<Looper>, events: E) -> Self {
        Self { listener: Listener::new(looper), events: Lent::new(events) }
    }
}

/// A stream on a connection, with the event table and callbacks PipeWire holds a pointer into. Dropping it
/// disconnects and destroys the stream under the loop's lock, which waits for its data thread.
pub(crate) struct Stream<T> {
    raw: *mut pw::pw_stream,
    callbacks: Lent<T>,
    events: Lent<pw::pw_stream_events>,
    listener: Listener,
    /// Last, so the connection outlives the stream and everything PipeWire held of it.
    connection: Arc<Connection>,
}

// SAFETY: the stream is only used under its connection's lock, or read for its time, which is real-time safe; the
// callbacks are lent and touched by PipeWire's threads alone.
unsafe impl<T: Send> Send for Stream<T> {}

impl<T> Stream<T> {
    /// Connects the stream in `direction` to `target` with `flags`, offering `params` (pods the caller keeps alive for
    /// the call); whether PipeWire took it.
    pub(crate) fn connect(
        &self,
        _held: &Locked<'_>,
        direction: spa::spa_direction,
        target: u32,
        flags: pw::pw_stream_flags,
        params: &mut [*const spa::spa_pod],
    ) -> bool {
        let Ok(count) = u32::try_from(params.len()) else { return false };
        // SAFETY: the stream is live and the loop is locked, which `_held` shows; `params` holds `count` pods, which
        // the caller keeps alive.
        let status = unsafe {
            (self.connection.api().stream_connect)(self.raw, direction, target, flags, params.as_mut_ptr(), count)
        };
        status >= 0
    }

    /// How the stream's sound stands to its device now; real-time safe.
    pub(crate) fn time(&self) -> Option<pw::pw_time> {
        // SAFETY: a zeroed `pw_time` is plain data the call fills.
        let mut time: pw::pw_time = unsafe { std::mem::zeroed() };
        // SAFETY: the stream is live while `self` is; reading its time is real-time safe from any thread.
        let status =
            unsafe { (self.connection.api().stream_get_time_n)(self.raw, &raw mut time, size_of::<pw::pw_time>()) };
        (status >= 0).then_some(time)
    }

    /// Sets the stream's rate control to `ratio`; whether PipeWire took it. The stream must be streaming.
    pub(crate) fn set_rate(&self, ratio: f32) -> bool {
        let mut values = [ratio];
        let _lock = self.connection.lock();
        // SAFETY: the stream is live and its loop is locked, where PipeWire takes a control; one control, then the id
        // 0 that ends the list.
        let status = unsafe {
            (self.connection.api().stream_set_control)(self.raw, spa::SPA_PROP_rate, 1, values.as_mut_ptr(), 0_u32)
        };
        status >= 0
    }
}

impl<T> Drop for Stream<T> {
    fn drop(&mut self) {
        let api = self.connection.api();
        let _lock = self.connection.looper.lock();
        // SAFETY: the loop is locked and the stream not yet destroyed.
        unsafe { (api.stream_disconnect)(self.raw) };
        self.listener.detach();
        // SAFETY: as above; its listener is out, and destroying it waits for its data thread, so no callback reads
        // what is lent after this.
        unsafe { (api.stream_destroy)(self.raw) };
    }
}

/// The daemon's registry, a proxy on a connection. Dropping it destroys the proxy under the loop's lock.
pub(crate) struct RegistryProxy {
    raw: *mut pw::pw_registry,
    /// Held so the connection outlives the proxy.
    connection: Arc<Connection>,
}

// SAFETY: the proxy is only used under its connection's lock.
unsafe impl Send for RegistryProxy {}

#[cfg(test)]
impl RegistryProxy {
    /// The proxy, for a test that asks the library about it.
    pub(crate) fn raw(&self) -> *mut pw::pw_registry {
        self.raw
    }
}

impl Drop for RegistryProxy {
    fn drop(&mut self) {
        let _lock = self.connection.looper.lock();
        // SAFETY: a proxy this connection made and has not freed; the loop is locked.
        unsafe { (self.connection.api().proxy_destroy)(self.raw.cast()) };
    }
}

/// An object of the daemon, bound or created, with the data its listener's callbacks are handed. Dropping it takes the
/// listener out and then destroys the proxy under the loop's lock: a proxy the daemon has not yet answered for goes
/// on telling its listeners.
pub(crate) struct Proxy<T> {
    raw: *mut pw::pw_proxy,
    data: Lent<T>,
    listener: Listener,
    /// Last, so the connection outlives the proxy and what its listener was handed.
    connection: Arc<Connection>,
}

// SAFETY: the proxy is only used under its connection's lock; its data is touched by the loop's thread alone.
unsafe impl<T: Send + Sync> Send for Proxy<T> {}

impl<T> Proxy<T> {
    /// The proxy, for a method call made under its loop's lock while it is held.
    pub(crate) fn raw(&self) -> *mut pw::pw_proxy {
        self.raw
    }

    /// What its callbacks are handed; they change it only through atomics or cells that are `Sync`.
    pub(crate) fn data(&self) -> &T {
        // SAFETY: lent for as long as `self`, and only ever read through shared references.
        unsafe { &*self.data.as_ptr() }
    }
}

impl<T> Drop for Proxy<T> {
    fn drop(&mut self) {
        let _lock = self.connection.looper.lock();
        self.listener.detach();
        // SAFETY: a proxy this connection made and has not freed; the loop is locked.
        unsafe { (self.connection.api().proxy_destroy)(self.raw) };
    }
}
