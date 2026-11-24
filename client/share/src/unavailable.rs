//! A system with no picker yet: nothing can be picked, so no pick exists.

use crate::pick::{Cursor, Refused, Source, Want};
use std::convert::Infallible;

/// A pick that is still live; none can exist here.
pub struct Selection {
    pub source: Source,
    never: Infallible,
}

impl Selection {
    pub async fn close(self) {
        match self.never {}
    }
}

/// Refuses: this system has no picker yet.
pub async fn choose(_: Want, _: Cursor) -> Result<Selection, Refused> {
    Err(Refused::Unavailable)
}
