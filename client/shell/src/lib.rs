//! The client window: [`window`] runs a [`Scene`] on a device of ours, and [`watched`] paints a decoded picture, both
//! shared by the product (`main.rs`) and the measuring spikes (`examples/`).
#![forbid(unsafe_code)]
// Clippy reads this lint's level at the crate alone, so the lines under other crates' targets a test sends are
// excused here.
#![cfg_attr(test, expect(clippy::disallowed_macros, reason = "a test sends lines under other crates' targets"))]

mod access;
mod asks;
pub mod logging;
mod lost;
mod painter;
pub mod themed;
mod typing;
pub mod watchdog;
pub mod watched;
pub mod window;

pub use window::{Failure, Input, Next, Redrawn, Scene, Started, Unhosted, Wake, Woke, event_loop, frames, run};
