//! The crate's integration tests as one binary: a module for each subject, the helpers beside them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]

mod anonymity;
#[cfg(windows)]
mod common;
#[cfg(windows)]
mod placement;
#[cfg(windows)]
mod restart_manager;
#[cfg(windows)]
mod shown;
