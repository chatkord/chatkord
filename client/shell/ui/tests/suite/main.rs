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

mod access;
mod app;
mod app_icon;
mod button_states;
mod call;
mod choosers;
mod common;
mod components;
mod composer;
mod composer_focus;
mod door;
mod emoji;
mod emoji_machine;
mod emoji_page;
mod files;
mod focus;
mod harness;
mod host;
mod icons_rail;
mod input_states;
mod isolation;
mod layers;
mod messages;
mod reactions;
mod replies;
mod rich_body;
mod search_previews;
mod secret;
mod share;
mod sources;
mod talk;
mod viewer_app;
mod voice;
mod watch;
