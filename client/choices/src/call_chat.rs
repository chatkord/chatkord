//! A call's chat column, as this device keeps it: open or closed by the call header's Chat button, and as wide as its
//! splitter left it.

use crate::members::{ColumnWidth, Span};
use design::{CALL_CHAT_LEAST, CALL_CHAT_MOST};

/// Whether a call's channel's messages show beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum CallChatShown {
    /// Put away, as a call starts until the Chat button opens it.
    #[default]
    Closed,
    /// Beside the call where it docks, else over it once its button opened it.
    Open,
}

impl CallChatShown {
    /// The other one: what the button makes of it.
    #[must_use]
    pub const fn toggled(self) -> CallChatShown {
        match self {
            CallChatShown::Open => CallChatShown::Closed,
            CallChatShown::Closed => CallChatShown::Open,
        }
    }
}

/// A call's chat column's range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CallChatSpan;

impl Span for CallChatSpan {
    const START: f32 = CALL_CHAT_LEAST;
    const LEAST: f32 = CALL_CHAT_LEAST;
    const MOST: f32 = CALL_CHAT_MOST;
}

/// A call's chat column's width: whole pixels, never outside [`CALL_CHAT_LEAST`] to [`CALL_CHAT_MOST`].
pub type CallChatWidth = ColumnWidth<CallChatSpan>;

/// A call's chat column as this device keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CallChat {
    /// Open or closed.
    pub shown: CallChatShown,
    /// Its width, as its splitter left it.
    pub width: CallChatWidth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_chat_starts_closed_at_its_least_and_is_held_to_its_range() {
        assert_eq!(CallChat::default().shown, CallChatShown::Closed);
        assert_eq!(CallChatWidth::default().px(), CALL_CHAT_LEAST);
        assert_eq!(CallChatWidth::new(100.0).px(), CALL_CHAT_LEAST);
        assert_eq!(CallChatWidth::new(9000.0).px(), CALL_CHAT_MOST);
        assert_eq!(CallChatWidth::new(480.4).whole(), 480);
    }
}
