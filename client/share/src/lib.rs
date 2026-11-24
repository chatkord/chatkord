//! Choosing what a share shows: on Linux in the desktop portal's picker, which hands back a PipeWire node and a
//! connection that may open it; on Windows in our own, from the monitors and windows listed here; elsewhere refused.
//! A picked window's owner, whose sound the share carries, is named where the desktop can say. Nothing here captures.

#[cfg(target_os = "linux")]
mod owner;
mod pick;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(not(any(target_os = "linux", windows)))]
mod unavailable;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use portal as system;
#[cfg(not(any(target_os = "linux", windows)))]
use unavailable as system;
#[cfg(windows)]
use windows as system;

#[cfg(target_os = "linux")]
pub use owner::{NoAnswer, ask_kwin, owner};
pub use pick::{Cursor, Kind, Owner, Refused, Source, Want};
pub use system::{Selection, choose};
#[cfg(windows)]
pub use windows::{Candidate, Icon, Id, candidates, select};
