//! A device a person picked, known again by its id while it is away.

/// A microphone or a speaker picked: what it is asked for by, and its name when it was picked, which is what a
/// person is told of it while it is not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    /// What it is asked for by: its id, which two devices of one name never share.
    pub id: String,
    /// What a person read when they picked it.
    pub label: String,
}
