//! A call, toolkit- and terminal-free: joining, hearing, sharing and watching on one seat, or a probe with no call.
//! What happens is reported as values to a callback, so every front end runs the same loop and says it in its own words.
#![forbid(unsafe_code)]

mod bounded;
pub mod camera_probe;
mod chain_watch;
mod codec;
pub mod devices;
mod engines;
pub mod failure;
mod lines;
pub mod probe;
mod processing;
pub mod room;
mod send;
mod sound;
mod voice_log;
mod watch;

pub use crate::sound::LISTING_WITHIN;
pub use ::sound::speaker::PlayError;
pub use chain_watch::ChainWatch;
pub use engines::Engines;
pub use picture::Slot;
pub use send::stats::Timings;
pub use send::{broadcast, camera, screen, source};
pub use watch::receive;
pub use watch::viewing::UNSEEN;
pub use webrtc::{Engine, VoiceRefused};
