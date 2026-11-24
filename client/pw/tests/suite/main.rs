//! The crate's integration tests as one binary: a module for each subject, the helpers beside them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "a test's numbers are its own fixtures, bounded where they are written; the casts differ by system"
)]

#[cfg(target_os = "linux")]
mod beside_libwebrtc;
#[cfg(target_os = "linux")]
mod devices;
#[cfg(target_os = "linux")]
mod mix;
#[cfg(target_os = "linux")]
mod play;
#[cfg(target_os = "linux")]
mod sound;
#[cfg(target_os = "linux")]
mod voice;
