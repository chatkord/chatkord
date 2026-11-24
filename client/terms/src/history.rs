//! What names a send and a page of a timeline's history: the nonce that makes a send idempotent, and where a page is
//! read from.

use proto::rules::NONCE_BYTES;

/// The bytes that make one send idempotent: drawn once for a message and sent again with every retry of it, so a
/// resend after a lost reply is answered with the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Nonce([u8; NONCE_BYTES]);

impl Nonce {
    /// A nonce of these drawn bytes.
    pub const fn new(bytes: [u8; NONCE_BYTES]) -> Nonce {
        Nonce(bytes)
    }

    /// The drawn bytes, as a send carries them.
    pub const fn bytes(&self) -> &[u8; NONCE_BYTES] {
        &self.0
    }
}

/// Where a page of history is read from: a place in the timeline (`Message.place`), whether or not a message is kept
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The newest messages.
    Newest,
    /// The messages before this place.
    Before(u64),
    /// The messages after this place.
    After(u64),
    /// The message at this place and those around it.
    Around(u64),
}
