//! The end-to-end tests as one binary: a module for each subject, the helpers beside them. A call under test is the
//! product's own (`call::room::run`, driven through `common::running`), and what a test waits for is a report of it.

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

mod clock;
mod common;
mod e2e;
#[cfg(target_os = "linux")]
mod e2e_budget;
mod e2e_call;
mod e2e_call_camera;
#[cfg(target_os = "linux")]
mod e2e_call_screen;
#[cfg(any(target_os = "linux", windows))]
mod e2e_encoded;
#[cfg(target_os = "linux")]
mod e2e_face_to_voice;
#[cfg(target_os = "linux")]
mod e2e_join_voices;
#[cfg(any(target_os = "linux", windows))]
mod e2e_leave;
mod e2e_renegotiation;
mod e2e_room;
#[cfg_attr(
    any(target_os = "linux", windows),
    expect(clippy::print_stderr, reason = "the test reports what it measured in its output")
)]
#[cfg(any(target_os = "linux", windows))]
mod e2e_screen;
#[cfg(any(target_os = "linux", windows))]
mod e2e_share_bursts;
#[cfg(any(target_os = "linux", windows))]
mod e2e_share_keeps_up;
#[cfg(any(target_os = "linux", windows))]
mod e2e_share_late;
#[cfg(any(target_os = "linux", windows))]
mod e2e_share_paced;
#[cfg(any(target_os = "linux", windows))]
mod e2e_still_screen;
mod e2e_video;
#[cfg(target_os = "linux")]
mod e2e_viewer_cameras;
mod e2e_voice_load;
#[cfg(target_os = "linux")]
mod e2e_voice_trip;
mod join;
mod screen_support;
