//! The methods PipeWire's headers define inline, as a call through the object's interface table: nothing to resolve
//! from the library, so each is called the way the header's macro does.

use std::ffi::{CStr, c_int, c_void};

use libspa_sys as spa;
use pipewire_sys as pw;

use crate::session::Error;

/// An interface's method table and the object its methods take.
///
/// # Safety
/// `object` is a live PipeWire object whose interface's methods are an `M`.
unsafe fn table<'a, M>(object: *mut c_void) -> Option<(&'a M, *mut c_void)> {
    let interface = object.cast::<spa::spa_interface>();
    // SAFETY: as the caller says, `object` is a live interface.
    let callbacks = unsafe { &(*interface).cb };
    // SAFETY: as the caller says, its methods are an `M`; a method table this library lacks is null.
    let methods = unsafe { callbacks.funcs.cast::<M>().as_ref() }?;
    Some((methods, callbacks.data))
}

/// The core's registry (`pw_core_get_registry`), or null when the core lacks the method or refused.
///
/// # Safety
/// `core` is connected, and its loop is locked.
pub(crate) unsafe fn registry_of(core: *mut pw::pw_core) -> *mut pw::pw_registry {
    // SAFETY: a core's methods are a `pw_core_methods`, and the caller says it is live.
    let Some((methods, object)) = (unsafe { table::<pw::pw_core_methods>(core.cast()) }) else {
        return std::ptr::null_mut();
    };
    let Some(get_registry) = methods.get_registry else {
        return std::ptr::null_mut();
    };
    // SAFETY: the call is the header's, on a connected core whose loop the caller has locked.
    unsafe { get_registry(object, pw::PW_VERSION_REGISTRY, 0) }
}

/// Adds a listener to `registry` (`pw_registry_add_listener`).
///
/// # Safety
/// `registry` is live, its loop is locked, and `hook`, `events` and `data` outlive it.
pub(crate) unsafe fn listen_registry(
    registry: *mut pw::pw_registry,
    hook: *mut spa::spa_hook,
    events: *const pw::pw_registry_events,
    data: *mut c_void,
) -> Result<(), Error> {
    let refused = Error::Refused("listen to the registry");
    // SAFETY: a registry's methods are a `pw_registry_methods`, and the caller says it is live.
    let (methods, object) = unsafe { table::<pw::pw_registry_methods>(registry.cast()) }.ok_or(refused)?;
    let add = methods.add_listener.ok_or(refused)?;
    // SAFETY: the call is the header's; the caller says the loop is locked and the listener outlives the registry.
    if unsafe { add(object, hook, events, data) } < 0 { Err(refused) } else { Ok(()) }
}

/// A proxy of the global `id` as an object of `kind` at `version` (`pw_registry_bind`), or null when refused.
///
/// # Safety
/// `registry` is live and its loop is locked.
pub(crate) unsafe fn bind(registry: *mut pw::pw_registry, id: u32, kind: &CStr, version: u32) -> *mut pw::pw_proxy {
    // SAFETY: as `listen_registry`.
    let Some((methods, object)) = (unsafe { table::<pw::pw_registry_methods>(registry.cast()) }) else {
        return std::ptr::null_mut();
    };
    let Some(bind) = methods.bind else {
        return std::ptr::null_mut();
    };
    // SAFETY: the call is the header's, on a live registry whose loop the caller has locked.
    unsafe { bind(object, id, kind.as_ptr(), version, 0) }.cast()
}

/// Adds a listener to a proxy bound as a client (`pw_client_add_listener`).
///
/// # Safety
/// `client` is a live client proxy, its loop is locked, and `hook`, `events` and `data` outlive it.
pub(crate) unsafe fn listen_client(
    client: *mut pw::pw_proxy,
    hook: *mut spa::spa_hook,
    events: *const pw::pw_client_events,
    data: *mut c_void,
) -> Result<(), Error> {
    let refused = Error::Refused("listen to a client");
    // SAFETY: a client proxy's methods are a `pw_client_methods`, and the caller says it is live.
    let (methods, object) = unsafe { table::<pw::pw_client_methods>(client.cast()) }.ok_or(refused)?;
    let add = methods.add_listener.ok_or(refused)?;
    // SAFETY: the call is the header's; the caller says the loop is locked and the listener outlives the proxy.
    if unsafe { add(object, hook, events, data) } < 0 { Err(refused) } else { Ok(()) }
}

/// Adds a listener to a proxy bound as a node (`pw_node_add_listener`).
///
/// # Safety
/// `node` is a live node proxy, its loop is locked, and `hook`, `events` and `data` outlive it.
pub(crate) unsafe fn listen_node(
    node: *mut pw::pw_proxy,
    hook: *mut spa::spa_hook,
    events: *const pw::pw_node_events,
    data: *mut c_void,
) -> Result<(), Error> {
    let refused = Error::Refused("listen to a node");
    // SAFETY: a node proxy's methods are a `pw_node_methods`, and the caller says it is live.
    let (methods, object) = unsafe { table::<pw::pw_node_methods>(node.cast()) }.ok_or(refused)?;
    let add = methods.add_listener.ok_or(refused)?;
    // SAFETY: the call is the header's; the caller says the loop is locked and the listener outlives the proxy.
    if unsafe { add(object, hook, events, data) } < 0 { Err(refused) } else { Ok(()) }
}

/// Asks the daemon's `factory` for an object of `kind` at `version`, made with `props` (`pw_core_create_object`); null
/// when the ask could not be sent.
///
/// # Safety
/// `core` is connected and its loop is locked; `props` is valid for the call, which copies it.
pub(crate) unsafe fn create_object(
    core: *mut pw::pw_core,
    factory: &CStr,
    kind: &CStr,
    version: u32,
    props: &spa::spa_dict,
) -> *mut pw::pw_proxy {
    // SAFETY: as `registry_of`.
    let Some((methods, object)) = (unsafe { table::<pw::pw_core_methods>(core.cast()) }) else {
        return std::ptr::null_mut();
    };
    let Some(create) = methods.create_object else {
        return std::ptr::null_mut();
    };
    // SAFETY: the call is the header's, on a connected core whose loop the caller has locked; it copies `props`.
    unsafe { create(object, factory.as_ptr(), kind.as_ptr(), version, props, 0) }.cast()
}

/// Adds a listener to a connected core (`pw_core_add_listener`).
///
/// # Safety
/// `core` is connected, its loop is locked, and `hook`, `events` and `data` outlive it.
pub(crate) unsafe fn listen_core(
    core: *mut pw::pw_core,
    hook: *mut spa::spa_hook,
    events: *const pw::pw_core_events,
    data: *mut c_void,
) -> Result<(), Error> {
    let refused = Error::Refused("listen to the sound server");
    // SAFETY: as `registry_of`.
    let (methods, object) = unsafe { table::<pw::pw_core_methods>(core.cast()) }.ok_or(refused)?;
    let add = methods.add_listener.ok_or(refused)?;
    // SAFETY: the call is the header's; the caller says the loop is locked and the listener outlives the core.
    if unsafe { add(object, hook, events, data) } < 0 { Err(refused) } else { Ok(()) }
}

/// Asks the daemon to answer `seq` with the core's `done` once it has answered everything asked before it
/// (`pw_core_sync`); the sequence number `done` will carry, or an error's negative number.
///
/// # Safety
/// `core` is connected and its loop is locked.
pub(crate) unsafe fn sync(core: *mut pw::pw_core, seq: c_int) -> c_int {
    // SAFETY: as `registry_of`.
    let Some((methods, object)) = (unsafe { table::<pw::pw_core_methods>(core.cast()) }) else {
        return -1;
    };
    let Some(sync) = methods.sync else {
        return -1;
    };
    // SAFETY: the call is the header's, on a connected core whose loop the caller has locked.
    unsafe { sync(object, pw::PW_ID_CORE, seq) }
}

/// Asks a bound node for every parameter of `id` (`pw_node_enum_params`); each arrives at its listener's `param`.
///
/// # Safety
/// `node` is a live node proxy and its loop is locked.
pub(crate) unsafe fn enum_params(node: *mut pw::pw_proxy, id: u32) -> Result<(), Error> {
    let refused = Error::Refused("ask a node for its parameters");
    // SAFETY: as `listen_node`.
    let (methods, object) = unsafe { table::<pw::pw_node_methods>(node.cast()) }.ok_or(refused)?;
    let enumerate = methods.enum_params.ok_or(refused)?;
    // SAFETY: the call is the header's, on a live node whose loop the caller has locked; no filter.
    if unsafe { enumerate(object, 0, id, 0, u32::MAX, std::ptr::null()) } < 0 { Err(refused) } else { Ok(()) }
}
