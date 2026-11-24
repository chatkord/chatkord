//! The crate's integration tests as one binary: a module for each subject, the helpers beside them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]

mod appearance;
#[cfg(target_os = "linux")]
mod awake;
#[cfg(windows)]
mod awake_windows;
mod forget;
#[cfg(target_os = "linux")]
mod idle;
mod keyring;
mod keyring_casing;
#[cfg(windows)]
mod launcher_windows;
#[cfg(target_os = "linux")]
mod notify;
#[cfg(windows)]
mod notify_windows;
