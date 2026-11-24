//! What the screens ask of the world, carried out the same way for every host: the door's errands over the keyring
//! and the network, a sign-in at a named address for a host that asks a person itself, and a window's every ask on a
//! runner of its own. A host brings only what its toolkit owns ([`Host`]). No toolkit.

mod attach;
mod call;
mod debug_report;
pub mod door;
mod errand;
mod host;
mod keeps;
mod launch;
pub mod mailbox;
mod pictures;
mod place;
mod previews;
mod probe;
pub mod prompted;
mod random;
mod runner;
pub mod session;
pub mod signin;
mod sources;
mod startup;
mod update;

pub use door::{Carried, carry_out};
pub use host::{Host, Still};
pub use launch::opened_at;
pub use prompted::{Prompt, Stopped, sign_in_at};
pub use random::{jitter, moment, nonce};
pub use runner::Runner;
pub use session::{Kept, Lent, Session};
pub use update::Handover;
