//! A listener on the daemon's registry that keeps the [`Graph`] a share's sound is planned from, and tells a [`Heed`]
//! each time it changes.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::conn::{Connection, Listening, Proxy, RegistryProxy};
use crate::dict::{Borrowed, interface, text};
use crate::graph::{Graph, Kind};
use crate::lent::Lent;
use crate::methods;
use crate::session::Error;

/// What hears the graph change. It runs on PipeWire's thread with the loop's lock held, so it must not block.
pub(crate) trait Heed: Send + 'static {
    fn changed(&mut self, graph: &Graph, connection: &Arc<Connection>);
}

/// What the registry's and the nodes' callbacks share.
struct State<H> {
    connection: Arc<Connection>,
    registry: Option<RegistryProxy>,
    graph: Graph,
    heed: H,
    client_events: Lent<pw::pw_client_events>,
    /// The clients bound for their properties, by id.
    bound: HashMap<u32, Proxy<ClientListener>>,
}

struct ClientListener {
    id: u32,
    state: *mut c_void,
}

impl<H: Heed> State<H> {
    /// Binds the client `id`, whose properties arrive at [`on_info`]; one that cannot be bound stays unknown and its
    /// streams unlinked. `root` is the lent state's pointer, as one made from `self` would not outlive this borrow.
    fn bind(&mut self, id: u32, root: *mut c_void) {
        let Some(registry) = &self.registry else { return };
        let held = self.connection.lock();
        let events = self.client_events.as_ptr();
        let listener = ClientListener { id, state: root };
        let bound = held.bind(registry, id, interface::CLIENT, pw::PW_VERSION_CLIENT, listener, |proxy, hook, data| {
            // SAFETY: the proxy was just made and the loop is locked; the hook and the events are lent and outlive it,
            // as the state does.
            unsafe { methods::listen_client(proxy, hook, events, data) }
        });
        if let Some(bound) = bound {
            self.bound.insert(id, bound);
        }
    }

    fn changed(&mut self) {
        self.heed.changed(&self.graph, &self.connection);
    }
}

/// The registry, heard on a loop of its own. Dropping it stops listening.
pub(crate) struct Watch<H> {
    /// First, so the callbacks stop before the state they reach, and the registry in it, are freed.
    _listening: Listening<pw::pw_registry_events>,
    state: Lent<State<H>>,
    connection: Arc<Connection>,
}

// SAFETY: every use of the state is under the loop's lock, or in `Drop` after the listener that reaches it is gone;
// `H` is `Send`.
unsafe impl<H: Heed> Send for Watch<H> {}
// SAFETY: as above: `with` reaches the state only under the loop's lock.
unsafe impl<H: Heed> Sync for Watch<H> {}

impl<H: Heed> Watch<H> {
    /// Connects to the daemon with `modules` loaded, and keeps `graph` as the registry says, telling `heed`.
    pub(crate) fn open(name: &CStr, modules: &[&CStr], real_time: bool, graph: Graph, heed: H) -> Result<Self, Error> {
        let connection = Connection::open(name, modules, real_time)?;
        // SAFETY: a zeroed event table has every callback unset and the version zero, set below.
        let mut client_events: pw::pw_client_events = unsafe { std::mem::zeroed() };
        client_events.version = pw::PW_VERSION_CLIENT_EVENTS;
        client_events.info = Some(on_info::<H>);
        let state = Lent::new(State {
            connection: Arc::clone(&connection),
            registry: None,
            graph,
            heed,
            client_events: Lent::new(client_events),
            bound: HashMap::new(),
        });
        // SAFETY: as above.
        let mut events: pw::pw_registry_events = unsafe { std::mem::zeroed() };
        events.version = pw::PW_VERSION_REGISTRY_EVENTS;
        events.global = Some(on_global::<H>);
        events.global_remove = Some(on_global_remove::<H>);

        let held = connection.lock();
        let registry = held.registry()?;
        let data = state.as_ptr().cast::<c_void>();
        // SAFETY: the state is lent and outlives the listening, which `Watch` drops first.
        let listening = unsafe { held.listen_registry(&registry, events, data) }?;
        // SAFETY: no callback reads the state before the loop's lock is let go, which this holds.
        unsafe { (*state.as_ptr()).registry = Some(registry) };
        drop(held);
        Ok(Self { _listening: listening, state, connection })
    }

    /// Runs `f` on the heed, the graph and the connection, under the loop's lock.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut H, &Graph, &Arc<Connection>) -> R) -> R {
        let _lock = self.connection.lock();
        // SAFETY: the callbacks reach the state only under this same lock, so nothing else holds it now.
        let state = unsafe { &mut *self.state.as_ptr() };
        f(&mut state.heed, &state.graph, &self.connection)
    }
}

impl<H> Drop for Watch<H> {
    fn drop(&mut self) {
        let _lock = self.connection.lock();
        // SAFETY: the callbacks reach the state only under this same lock.
        let state = unsafe { &mut *self.state.as_ptr() };
        state.bound.clear();
    }
}

unsafe extern "C" fn on_global<H: Heed>(
    data: *mut c_void,
    id: u32,
    _permissions: u32,
    kind: *const c_char,
    _version: u32,
    props: *const spa::spa_dict,
) {
    // SAFETY: `data` is the lent state the listener was added with; callbacks run one at a time, under the loop's lock.
    let state = unsafe { &mut *data.cast::<State<H>>() };
    // SAFETY: `kind` is the global's interface name, a string PipeWire owns for this call.
    let kind = match unsafe { text(kind) } {
        Some(kind) if kind == interface::NODE.to_bytes() => Kind::Node,
        Some(kind) if kind == interface::PORT.to_bytes() => Kind::Port,
        Some(kind) if kind == interface::CLIENT.to_bytes() => Kind::Client,
        _ => return,
    };
    // SAFETY: `props` is the dictionary of this call.
    let props = unsafe { Borrowed::new(props) };
    if state.graph.global(id, kind, &props) {
        state.bind(id, data);
    }
    state.changed();
}

unsafe extern "C" fn on_global_remove<H: Heed>(data: *mut c_void, id: u32) {
    // SAFETY: as above.
    let state = unsafe { &mut *data.cast::<State<H>>() };
    state.bound.remove(&id);
    if state.graph.remove(id) {
        state.changed();
    }
}

unsafe extern "C" fn on_info<H: Heed>(data: *mut c_void, info: *const pw::pw_client_info) {
    // SAFETY: `data` is the lent listener of a bound client, whose `state` is the watch's lent state; callbacks run one
    // at a time under the loop's lock.
    let listener = unsafe { &*data.cast::<ClientListener>() };
    // SAFETY: as above.
    let state = unsafe { &mut *listener.state.cast::<State<H>>() };
    let id = listener.id;
    // SAFETY: `info` is PipeWire's for this call, or null.
    let Some(info) = (unsafe { info.as_ref() }) else { return };
    if info.change_mask & u64::from(pw::PW_CLIENT_CHANGE_MASK_PROPS) == 0 {
        return;
    }
    // SAFETY: the client's dictionary, PipeWire's for this call.
    let props = unsafe { Borrowed::new(info.props) };
    if state.graph.client(id, &props) {
        state.changed();
    }
}
