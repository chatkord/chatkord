//! What a server tells a client unasked, and what it hands back for an invite made.

use crate::error::Ended;
use crate::secret::Secret;
use proto::chatkord_v1::Event;

/// What the server tells a client without being asked, then the end.
#[derive(Debug)]
pub enum Heard {
    /// One event, in the order the server sent it.
    Event(Event),
    /// Events were dropped because the reader was behind: the state follows whole, and what is never sent whole (the
    /// members list) is the reader's to ask for again.
    Behind,
    /// The connection ended, and nothing follows.
    Ended(Ended),
}

/// An invite just made: its code, shown once, and when it stops working (seconds since the epoch).
#[derive(Clone, Debug)]
pub struct Invited {
    /// The code, which admits whoever holds it.
    pub code: Secret,
    /// When it stops working.
    pub expires_at: i64,
}
