//! `libpipewire`, opened by name into a private handle, and the functions capture calls.

use std::ffi::{c_char, c_int, c_void};
use std::fmt;
use std::sync::OnceLock;

use libloading::Library;
use libspa_sys::{spa_dict, spa_direction, spa_hook, spa_pod};
use pipewire_sys::{
    pw_buffer, pw_context, pw_core, pw_impl_module, pw_loop, pw_properties, pw_proxy, pw_proxy_events, pw_stream,
    pw_stream_events, pw_stream_flags, pw_thread_loop, pw_time,
};

/// The library every distribution ships for PipeWire's current ABI.
const NAME: &str = "libpipewire-0.3.so.0";

/// Why PipeWire could not be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// No `libpipewire` on this device.
    NotInstalled,
    /// It is there but lacks a function this client needs: too old.
    TooOld(&'static str),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "{NAME} is not installed"),
            Self::TooOld(function) => write!(f, "{NAME} has no {function}"),
        }
    }
}

impl std::error::Error for Unavailable {}

/// Declares [`Api`] from the C signature of each function, and checks each against the declaration the headers produced
/// (`pipewire_sys`) by naming the function in a constant of its type: a mismatch is a compile error, and since no
/// constant is used, no reference to the symbol is emitted, so nothing is resolved at link time.
macro_rules! functions {
    ($($field:ident: $symbol:ident($($argument:ty),*) $(-> $result:ty)?;)*) => {
        /// The functions of `libpipewire` the client uses, each the C signature of the installed headers, resolved by
        /// name from our own handle.
        pub(crate) struct Api {
            $(pub(crate) $field: unsafe extern "C" fn($($argument),*) $(-> $result)?,)*
            /// Takes `(id, count, values)` triples after the first, ended by an id of 0. Written out, as a macro
            /// argument list cannot carry the `...`.
            pub(crate) stream_set_control: unsafe extern "C" fn(*mut pw_stream, u32, u32, *mut f32, ...) -> c_int,
            /// Kept last so the functions above never outlive the library they point into.
            _library: Library,
        }

        $(const _: unsafe extern "C" fn($($argument),*) $(-> $result)? = pipewire_sys::$symbol;)*
        const _: unsafe extern "C" fn(*mut pw_stream, u32, u32, *mut f32, ...) -> c_int =
            pipewire_sys::pw_stream_set_control;

        fn resolve(library: Library) -> Result<Api, Unavailable> {
            Ok(Api {
                $($field: function(&library, stringify!($symbol))?,)*
                stream_set_control: function(&library, "pw_stream_set_control")?,
                _library: library,
            })
        }
    };
}

functions! {
    thread_loop_new: pw_thread_loop_new(*const c_char, *const spa_dict) -> *mut pw_thread_loop;
    thread_loop_get_loop: pw_thread_loop_get_loop(*mut pw_thread_loop) -> *mut pw_loop;
    thread_loop_start: pw_thread_loop_start(*mut pw_thread_loop) -> c_int;
    thread_loop_stop: pw_thread_loop_stop(*mut pw_thread_loop);
    thread_loop_destroy: pw_thread_loop_destroy(*mut pw_thread_loop);
    thread_loop_lock: pw_thread_loop_lock(*mut pw_thread_loop);
    thread_loop_unlock: pw_thread_loop_unlock(*mut pw_thread_loop);
    context_new: pw_context_new(*mut pw_loop, *mut pw_properties, usize) -> *mut pw_context;
    context_connect_fd: pw_context_connect_fd(*mut pw_context, c_int, *mut pw_properties, usize) -> *mut pw_core;
    context_load_module: pw_context_load_module(*mut pw_context, *const c_char, *const c_char, *mut pw_properties)
        -> *mut pw_impl_module;
    context_connect: pw_context_connect(*mut pw_context, *mut pw_properties, usize) -> *mut pw_core;
    context_destroy: pw_context_destroy(*mut pw_context);
    core_disconnect: pw_core_disconnect(*mut pw_core) -> c_int;
    properties_new_string: pw_properties_new_string(*const c_char) -> *mut pw_properties;
    properties_new_dict: pw_properties_new_dict(*const spa_dict) -> *mut pw_properties;
    proxy_destroy: pw_proxy_destroy(*mut pw_proxy);
    proxy_add_listener: pw_proxy_add_listener(*mut pw_proxy, *mut spa_hook, *const pw_proxy_events, *mut c_void);
    stream_new: pw_stream_new(*mut pw_core, *const c_char, *mut pw_properties) -> *mut pw_stream;
    stream_add_listener: pw_stream_add_listener(*mut pw_stream, *mut spa_hook, *const pw_stream_events, *mut c_void);
    stream_connect: pw_stream_connect(*mut pw_stream, spa_direction, u32, pw_stream_flags, *mut *const spa_pod, u32)
        -> c_int;
    stream_update_params: pw_stream_update_params(*mut pw_stream, *mut *const spa_pod, u32) -> c_int;
    stream_dequeue_buffer: pw_stream_dequeue_buffer(*mut pw_stream) -> *mut pw_buffer;
    stream_queue_buffer: pw_stream_queue_buffer(*mut pw_stream, *mut pw_buffer) -> c_int;
    stream_return_buffer: pw_stream_return_buffer(*mut pw_stream, *mut pw_buffer) -> c_int;
    stream_flush: pw_stream_flush(*mut pw_stream, bool) -> c_int;
    stream_disconnect: pw_stream_disconnect(*mut pw_stream) -> c_int;
    stream_destroy: pw_stream_destroy(*mut pw_stream);
    stream_get_time_n: pw_stream_get_time_n(*mut pw_stream, *mut pw_time, usize) -> c_int;
}

fn function<T: Copy>(library: &Library, name: &'static str) -> Result<T, Unavailable> {
    // SAFETY: `T` is the C signature of `name`; the pointer is copied out and the library is kept alive beside it for
    // the life of the process.
    unsafe { library.get::<T>(format!("{name}\0").as_bytes()) }
        .map(|symbol| *symbol)
        .map_err(|_| Unavailable::TooOld(name))
}

fn load() -> Result<Api, Unavailable> {
    // SAFETY: loading a shared library runs its initialisers; this is the system's PipeWire client library, which is
    // built to be loaded by any program.
    let library = unsafe { Library::new(NAME) }.map_err(|_| Unavailable::NotInstalled)?;
    let init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) = function(&library, "pw_init")?;
    let api = resolve(library)?;
    // SAFETY: `pw_init` takes the program's arguments or nulls, and may be called more than once.
    unsafe { init(std::ptr::null_mut(), std::ptr::null_mut()) };
    Ok(api)
}

/// The library, loaded the first time anything asks and kept for the life of the process.
pub(crate) fn api() -> Result<&'static Api, Unavailable> {
    static API: OnceLock<Result<Api, Unavailable>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(|why| *why)
}
