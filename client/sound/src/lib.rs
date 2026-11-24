//! The client's sound on the system's sound server (PipeWire, WASAPI): the devices listed and watched, a call's
//! microphone and speaker, the window's chimes, and a share's sound. One thread of this process, owned by [`Sound`],
//! lists the devices and the applications, tells the devices' changes and plays the chimes, so no caller waits on
//! the sound server from a thread that serves something else; a call's stream and a share's capture keep their own
//! real-time threads.
#![cfg(any(target_os = "linux", windows))]
#![cfg_attr(not(windows), forbid(unsafe_code))]

pub mod chime;
mod host;
mod owner;
pub mod share;
pub mod speaker;
pub mod voice;
pub mod watch;

#[cfg(any(test, feature = "testing"))]
pub use host::{Call, Served};
pub use owner::Sound;
