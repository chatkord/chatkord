//! Where a connection stood in what the whole server tells: the cursor of the last event it handed on, which a
//! connection that signed in again hands back to be told what it missed.

use proto::chatkord_v1::Event;

/// Opaque: the server sealed it and alone reads it. Kept in memory only, since it names a place in one server
/// process's sequence and is worth nothing once that process restarts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor(Vec<u8>);

impl Cursor {
    /// The cursor `event` carries; none for an event the server does not tell in order (a room's, who is in voice).
    pub fn of(event: &Event) -> Option<Self> {
        (!event.cursor.is_empty()).then(|| Self(event.cursor.clone()))
    }

    /// The sealed bytes, as a resume carries them back.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}
