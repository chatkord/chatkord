//! The PipeWire client a share's picture and sound, a call's voice and the window's own sounds need, with
//! `libpipewire` loaded at run time: libwebrtc's weak `pw_init` stubs (measured: 37 in the client binary) jump through
//! null pointers and shadow a linked library, so it is opened into a handle of our own. A device without it gets
//! `Unavailable`, not a program that will not start.

#![cfg(target_os = "linux")]

mod apps;
mod audio;
mod cameras;
mod conn;
mod dict;
mod graph;
mod hearing;
mod hook;
mod lent;
mod library;
mod looper;
mod methods;
mod mix;
mod play;
mod pulse;
mod registry;
mod session;
mod voice;
mod watch;

pub use apps::Apps;
pub use cameras::{Camera, Cameras, Change};
pub use graph::{App, Hears};
pub use library::Unavailable;
pub use mix::Mix;
pub use play::{Play, Playing};
pub use pulse::{Listed, Listing, PulseError, devices};
pub use registry::Registry;
pub use session::{Buffer, Dequeued, Error, FORMAT, Handler, Mapped, Plane, Role, Session, State, StreamRef, Time};
pub use voice::{Microphone, Speaker, Timing, Voice};
