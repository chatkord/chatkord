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

#[expect(clippy::print_stderr, reason = "a measuring test's output is what it prints")]
#[cfg(any(target_os = "linux", windows))]
mod beside_a_game;
#[cfg(target_os = "linux")]
mod captured;
mod common;
#[cfg(target_os = "linux")]
#[path = "../../../media/tests/suite/common/dmabuf.rs"]
mod dmabuf;
#[cfg(target_os = "linux")]
mod import;
#[cfg(windows)]
mod import_d3d12;
#[cfg(target_os = "linux")]
mod loader;
mod paint;
#[cfg(windows)]
mod preview_d3d12;
#[cfg(target_os = "linux")]
mod written;
