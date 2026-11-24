//! The crate's integration tests as one binary: a module for each subject, the helpers beside them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]

mod chat;
mod common;
mod devices;
mod door;
mod no_keyring;
mod remembered;
mod session;
mod stack;
