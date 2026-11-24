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

#[cfg(any(windows, target_os = "linux"))]
mod camera;
#[cfg(windows)]
mod capture_windows;
mod common;
mod decode;
#[expect(clippy::print_stderr, reason = "a check run for each tier prints its figures, as a test does")]
mod encode;
#[cfg(target_os = "linux")]
mod loader;
mod quality;
#[cfg(target_os = "linux")]
mod screen;
#[cfg(target_os = "linux")]
mod surface;
#[cfg(target_os = "linux")]
mod survey;
