//! The one bridge between the server's mediasoup parameters, sent as JSON, and the SDP the engine speaks. Whether an
//! SDP it writes is valid is judged by libwebrtc itself, in a test.
#![forbid(unsafe_code)]

mod codec;
mod consumed;
#[cfg(any(test, fuzzing))]
pub mod fuzz;
mod h264;
mod reached;
mod remote;
mod send;
mod stereo;
mod transport;

pub use codec::Codec;
pub use consumed::{Consumed, consumed};
pub use reached::{NotReached, Reached, Resolved, Scope};
pub use remote::{Media, Offered, RemoteOffer, Unsafe};
pub use send::{Answered, LocalSend, SendRefused, answer};
pub use stereo::heard_in_stereo;
pub use transport::{Transport, TransportRefused, dtls_from_sdp};
