//! PipeWire's thread loop: its thread runs the callbacks with its lock held, and any other thread takes that lock
//! around a call. The lock is recursive.

use std::ffi::CStr;

use pipewire_sys as pw;

use crate::library::Api;
use crate::session::Error;

/// What any connection needs, and what a stream needs besides to be a node of its own.
pub(crate) const PROTOCOL: &CStr = c"libpipewire-module-protocol-native";
pub(crate) const STREAM: [&CStr; 3] = [PROTOCOL, c"libpipewire-module-client-node", c"libpipewire-module-adapter"];
/// PipeWire's own module that runs a context's data thread ahead of ordinary work, by the process's limits, the
/// portal or RTKit as it finds them. A config file would load it; ours is `null`.
pub(crate) const REAL_TIME: &CStr = c"libpipewire-module-rt";

pub(crate) struct Looper {
    pub(crate) api: &'static Api,
    raw: *mut pw::pw_thread_loop,
}

// SAFETY: the loop pointer is only used through the loop's own lock, which is how PipeWire says a thread loop is
// shared.
unsafe impl Send for Looper {}
// SAFETY: as above.
unsafe impl Sync for Looper {}

/// The loop's lock, held until this is dropped.
pub(crate) struct Locked<'a>(&'a Looper);

impl Looper {
    /// A loop that is made but not started, so what runs on it can be set up first.
    pub(crate) fn new(api: &'static Api, name: &CStr) -> Result<Self, Error> {
        // SAFETY: the name is NUL-terminated; no properties.
        let raw = unsafe { (api.thread_loop_new)(name.as_ptr(), std::ptr::null()) };
        if raw.is_null() { Err(Error::Refused("make a loop")) } else { Ok(Self { api, raw }) }
    }

    pub(crate) fn start(&self) -> Result<(), Error> {
        // SAFETY: the loop was made by `new` and is destroyed only when `self` is dropped.
        if unsafe { (self.api.thread_loop_start)(self.raw) } < 0 {
            Err(Error::Refused("start the loop"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn lock(&self) -> Locked<'_> {
        // SAFETY: the loop lives as long as `self`; its lock is recursive.
        unsafe { (self.api.thread_loop_lock)(self.raw) };
        Locked(self)
    }

    /// The loop a context is made on. Valid for as long as `self`, and used under its lock.
    pub(crate) fn raw(&self) -> *mut pw::pw_loop {
        // SAFETY: the loop was made by `new`.
        unsafe { (self.api.thread_loop_get_loop)(self.raw) }
    }

    /// A context on this loop with only `modules` loaded. It asks for no config file (`null`): PipeWire refuses a
    /// context whose config is missing, and some distros ship none; `log.level=1` stops the warning that comes of it.
    pub(crate) fn context(&self, _held: &Locked<'_>, modules: &[&CStr]) -> Result<*mut pw::pw_context, Error> {
        // SAFETY: the string is the whitespace-separated `key=value` list the library documents, NUL-terminated.
        let config = unsafe { (self.api.properties_new_string)(c"config.name=\"null\" log.level=1".as_ptr()) };
        if config.is_null() {
            return Err(Error::Refused("make the context properties"));
        }
        // SAFETY: `pw_context_new` takes ownership of the properties it is handed; the loop is locked.
        let context = unsafe { (self.api.context_new)(self.raw(), config, 0) };
        if context.is_null() {
            return Err(Error::Refused("make a context"));
        }
        for module in modules {
            // SAFETY: the context is live; the name is NUL-terminated; no arguments and no properties. The module
            // belongs to the context and goes with it.
            let loaded = unsafe {
                (self.api.context_load_module)(context, module.as_ptr(), std::ptr::null(), std::ptr::null_mut())
            };
            if loaded.is_null() {
                // SAFETY: the context was made above and nothing else holds it.
                unsafe { (self.api.context_destroy)(context) };
                return Err(Error::Refused("load a module"));
            }
        }
        Ok(context)
    }

    /// Loads `module` into `context` if this system has it, and says whether it did: one that is only an offer, which
    /// the context works without.
    pub(crate) fn offer(&self, _held: &Locked<'_>, context: *mut pw::pw_context, module: &CStr) -> bool {
        // SAFETY: the context is live and the loop locked; the name is NUL-terminated; no arguments and no
        // properties. The module belongs to the context and goes with it.
        let loaded =
            unsafe { (self.api.context_load_module)(context, module.as_ptr(), std::ptr::null(), std::ptr::null_mut()) };
        !loaded.is_null()
    }

    /// Stops the loop's thread and waits for it. Never under the lock: the thread may want it.
    pub(crate) fn stop(&self) {
        // SAFETY: stopping a stopped loop is a no-op.
        unsafe { (self.api.thread_loop_stop)(self.raw) };
    }
}

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        // SAFETY: taken by `Looper::lock` on this thread.
        unsafe { (self.0.api.thread_loop_unlock)(self.0.raw) };
    }
}

impl Drop for Looper {
    fn drop(&mut self) {
        // SAFETY: the last handle is gone, so no thread uses the loop; it must be stopped before it is destroyed.
        unsafe { (self.api.thread_loop_stop)(self.raw) };
        // SAFETY: stopped above, and nothing else holds it.
        unsafe { (self.api.thread_loop_destroy)(self.raw) };
    }
}
