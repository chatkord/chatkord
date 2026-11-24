//! The sound devices PipeWire knows, heard as they come and go, so a headset unplugged mid-call is told at once
//! rather than found out by whoever asks next.

use std::collections::HashSet;
use std::ffi::{c_char, c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::conn::{Connection, Listening, RegistryProxy, ended};
use crate::dict::{Borrowed, interface, text};
use crate::graph::Props;
use crate::lent::Lent;
use crate::looper::PROTOCOL;
use crate::session::Error;

/// What the daemon says a device is: `Audio/Source`, `Audio/Sink`, and their kin.
const SOUND_CLASS: &str = "Audio/";

struct Callbacks {
    changed: Box<dyn FnMut() + Send>,
    /// The ids of the sound devices seen, because a removal names only the id.
    devices: HashSet<u32>,
    /// Set when the sound server ended the connection.
    lost: Arc<AtomicBool>,
}

/// A listener on the daemon's registry. Dropping it stops listening.
pub struct Registry {
    // In the order they go: the listeners that reach the callbacks, the proxy, the callbacks, the connection last.
    _core_listening: Listening<pw::pw_core_events>,
    _listening: Listening<pw::pw_registry_events>,
    _registry: RegistryProxy,
    _callbacks: Lent<Callbacks>,
    lost: Arc<AtomicBool>,
    _connection: Arc<Connection>,
}

// SAFETY: every use of the lent callbacks is on the loop's thread under its lock, or in `Drop` after the listeners
// that reach them are taken out.
unsafe impl Send for Registry {}
// SAFETY: as above, and nothing reads through a shared reference: a registry has no method but `Drop`.
unsafe impl Sync for Registry {}

impl Registry {
    /// Calls `changed` whenever a sound device appears or goes, once for each already there, and once when the sound
    /// server ends the connection ([`Registry::lost`]). It runs on PipeWire's thread with the loop's lock held, so it
    /// must not block or call back into this crate.
    pub fn sound(changed: impl FnMut() + Send + 'static) -> Result<Self, Error> {
        let connection = Connection::open(c"chatkord-sound", &[PROTOCOL], false)?;
        let lost = Arc::new(AtomicBool::new(false));
        let callbacks =
            Lent::new(Callbacks { changed: Box::new(changed), devices: HashSet::new(), lost: Arc::clone(&lost) });
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut core_events: pw::pw_core_events = unsafe { std::mem::zeroed() };
        core_events.version = pw::PW_VERSION_CORE_EVENTS;
        core_events.error = Some(on_core_error);
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut events: pw::pw_registry_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_REGISTRY_EVENTS;
        events.global = Some(on_global);
        events.global_remove = Some(on_global_remove);
        let data = callbacks.as_ptr().cast::<c_void>();
        let held = connection.lock();
        let registry = held.registry()?;
        // SAFETY: the callbacks are lent and outlive the listeners, which this holds and drops first.
        let core_listening = unsafe { held.listen_core(core_events, data) }?;
        // SAFETY: as above.
        let listening = unsafe { held.listen_registry(&registry, events, data) }?;
        drop(held);
        Ok(Self {
            _core_listening: core_listening,
            _listening: listening,
            _registry: registry,
            _callbacks: callbacks,
            lost,
            _connection: connection,
        })
    }
}

impl Registry {
    /// Whether the sound server ended this connection: nothing more will be heard on it.
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

/// Whether a global's properties say it is a sound device.
fn is_sound_device(props: &impl Props) -> bool {
    props.get("media.class").is_some_and(|class| class.starts_with(SOUND_CLASS))
}

unsafe extern "C" fn on_global(
    data: *mut c_void,
    id: u32,
    _permissions: u32,
    kind: *const c_char,
    _version: u32,
    props: *const spa::spa_dict,
) {
    // SAFETY: `data` is the boxed `Callbacks` the listener was added with; PipeWire runs its callbacks one at a time.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    // SAFETY: `kind` is the global's interface name, a string PipeWire owns for this call.
    let node = unsafe { text(kind) } == Some(interface::NODE.to_bytes());
    // SAFETY: `props` is the dictionary of this call.
    if node && is_sound_device(&unsafe { Borrowed::new(props) }) && callbacks.devices.insert(id) {
        (callbacks.changed)();
    }
}

unsafe extern "C" fn on_global_remove(data: *mut c_void, id: u32) {
    // SAFETY: as above.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    if callbacks.devices.remove(&id) {
        (callbacks.changed)();
    }
}

unsafe extern "C" fn on_core_error(data: *mut c_void, id: u32, _seq: c_int, res: c_int, _message: *const c_char) {
    // SAFETY: as `on_global`: the core's listener runs on the same loop, one callback at a time.
    let callbacks = unsafe { &mut *data.cast::<Callbacks>() };
    if ended(id, res) && !callbacks.lost.swap(true, Ordering::AcqRel) {
        (callbacks.changed)();
    }
}
