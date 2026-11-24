//! A row of an ordered list moved to another place in it, as a drag or a key asks.

/// A row moved to sit just before another, or last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved<K> {
    /// The row moved.
    pub key: K,
    /// The row it now sits just before; none puts it last.
    pub before: Option<K>,
}
