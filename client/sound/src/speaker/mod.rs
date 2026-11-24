//! Short sounds played whole on a speaker by the system's sound server (PipeWire, WASAPI), not through the call's
//! engine: a sound is handed over at once and the server paces it, so nothing of ours can make it late or break it.
//! The [`Chimes`] are the owner thread's ([`crate::Sound`]) and play from that thread.

use std::fmt;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{Chimes, Stage};
#[cfg(windows)]
pub use windows::{Chimes, Stage};

/// The rate a sound is played at, in samples per second.
pub const RATE: u32 = 48_000;

/// Why a sound did not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum PlayError {
    /// This device has no sound server to play through.
    Unavailable,
    /// The sound server refused the sound or the speaker.
    Refused,
    /// The sound's owner is gone.
    Closed,
}

impl fmt::Display for PlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "no sound server to play through",
            Self::Refused => "the sound server refused the sound",
            Self::Closed => "the sound player stopped",
        })
    }
}

impl std::error::Error for PlayError {}
