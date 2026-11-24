//! The client's connection to a server's `/v1` socket: the hello, the sign-in or registration, requests answered by
//! the reply that names them, and the events the server sends unasked. It decides nothing about what an op means.
//!
//! A server that answers no heartbeat for too long is gone, since a connection can die without a close frame. What
//! reaches the network is proved by its type first: a parsed `terms::ServerUrl` or `terms::Address`, and a `terms::Secret`.
#![forbid(unsafe_code)]

pub mod files;
mod race;
mod sending;
mod session;
mod trust;

pub use rustls::HandshakeKind;
pub use sending::Sending;
pub use session::{ASK_TIMEOUT, Events, Link, PARTING, Quickened, Resolved};
pub use trust::{Trust, Vouched};
