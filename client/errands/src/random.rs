//! The window's one source of randomness, and the moment every message to the window is told with.

use screens::{Jitter, Moment};
use std::time::Instant;

/// 64 bits from the standard library's randomly keyed hasher, each draw a new key; for the jitter of a wait and a
/// call's ICE restarts, and a send's nonce, which need be unique, not secret.
fn drawn() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::hash::RandomState::new().build_hasher().finish()
}

/// The spread of a wait or a call's restart, drawn.
pub fn jitter() -> Jitter {
    Jitter::from_bits(drawn())
}

/// A nonce for a message this update may send, drawn.
pub fn nonce() -> terms::Nonce {
    let (high, low) = (drawn().to_le_bytes(), drawn().to_le_bytes());
    let mut bytes = [0; proto::rules::NONCE_BYTES];
    for (byte, drawn) in bytes.iter_mut().zip(high.into_iter().chain(low)) {
        *byte = drawn;
    }
    terms::Nonce::new(bytes)
}

/// What the window is told with every message: `now`, the wall clock, and the randomness a wait or a send may use.
pub fn moment(now: Instant) -> Moment {
    Moment { now, jitter: jitter(), nonce: nonce(), wall: wall() }
}

/// The wall clock in seconds since the epoch; a clock set before it reads as the epoch.
fn wall() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
}
