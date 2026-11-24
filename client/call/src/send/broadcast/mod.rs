//! This account's own screen, sent into the call it is in: opened each system's own way (`linux`, `windows`) and sent
//! the same way (`send`). A system with no capture refuses a share, and nothing it would hold can exist.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "linux", windows))]
mod send;
#[cfg(not(any(target_os = "linux", windows)))]
mod unavailable;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as system;
#[cfg(windows)]
use windows as system;

#[cfg(any(target_os = "linux", windows))]
pub use crate::send::share_sound::SoundAsked;
#[cfg(any(target_os = "linux", windows))]
pub use send::{Begun, Broadcast, Ended, begin};
#[cfg(not(any(target_os = "linux", windows)))]
pub use unavailable::{Begun, Broadcast, Ended, SoundAsked, begin};
