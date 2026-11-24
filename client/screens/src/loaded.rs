//! What the server was asked for and has not answered, answered, or could not answer: one shape for every list and
//! every set of facts a screen reads from it.

use terms::LinkError;

/// Something read from the server, as it last answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Loaded<T> {
    /// Asked, and not answered yet.
    #[default]
    Loading,
    /// What the server answered.
    Ready(T),
    /// The server did not answer the ask, and why.
    Failed(LinkError),
}

impl<T> From<Result<T, LinkError>> for Loaded<T> {
    fn from(answer: Result<T, LinkError>) -> Loaded<T> {
        match answer {
            Ok(ready) => Loaded::Ready(ready),
            Err(why) => Loaded::Failed(why),
        }
    }
}

impl<T> Loaded<T> {
    /// What was answered, once it was.
    pub fn ready(&self) -> Option<&T> {
        match self {
            Loaded::Ready(ready) => Some(ready),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    /// The same, to change.
    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Loaded::Ready(ready) => Some(ready),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    /// Whether the last ask went unanswered, so it may be asked again.
    pub fn failed(&self) -> bool {
        matches!(self, Loaded::Failed(_))
    }
}
