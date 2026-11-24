//! The client's network and load policy. The engine's congestion controller states a rate target
//! often; what is worth doing about each one is decided here and nowhere else, as are whether an
//! encoder keeps up with what a share asks of it, the rate a share holds itself to before its encoder, and the steps
//! (size and frame rate) a share is sent at. This crate
//! does no I/O and reads no clock: time is an argument, so a test sets it.
#![forbid(unsafe_code)]

mod backoff;
mod budget;
mod envelope;
mod gate;
mod governor;
mod keep_up;
mod ladder;
mod pace;

pub use backoff::{Backoff, Jitter};
pub use budget::Budget;
pub use envelope::Envelope;
pub use gate::{RateGate, Verdict};
pub use governor::{Governor, Judged, Limit};
pub use keep_up::{KeepUp, Load};
pub use ladder::{Ladder, Size, Step};
pub use pace::Pace;
