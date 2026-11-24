//! The crate's integration tests as one binary: a module for each subject.

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

#[cfg(windows)]
mod adapters_windows;
#[cfg(windows)]
mod capture_rate;
#[cfg(windows)]
mod still_windows;
#[cfg(windows)]
mod window_closed;
