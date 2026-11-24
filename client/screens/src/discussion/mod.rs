//! A text channel open in the content area: its timeline, its scroll, what is written in it and what is done to its
//! messages.

use crate::place::{
    Ask, ChannelList, ChannelStanding, Content, ListEntry, Lobby, Message, Moment, Owner, Place, Roll, Root,
    ServerEmoji, SignedIn, Status, TimelineStanding, Told, close_page,
};

use crate::chime::Chime;
use chat::{Draft, End, Heights, MessageId, Row, RowKey, Scroll, TimeZone, Timeline};
use proto::files::FileId;
use proto::rules::{Carries, Grant, Grants};
use std::ops::ControlFlow;
use std::time::{Duration, Instant};
use terms::{ChannelId, LinkError, MemberId, RoleId, TimelineId};
use words::{Phrase, Sentence, failure};

mod compose;
mod editing;
mod history;
mod message;
mod model;
mod pane;
mod permissions;
mod picker;
mod pins;
mod preview;
mod reactions;
pub(crate) mod reading;
mod route;
mod rows;
mod search;
mod send;
mod viewing;

pub use compose::*;
use editing::*;
use history::*;
pub use message::*;
pub use model::*;
pub use pane::*;
use permissions::*;
pub use picker::*;
use pins::*;
pub use preview::*;
use reactions::*;
pub use reading::*;
pub use rows::*;
pub use search::*;
use send::*;
use viewing::*;

#[cfg(test)]
mod tests {
    use crate::place;
    use crate::window::Window;

    pub(super) fn timeline(window: &Window) -> chat::Timeline {
        window.places()[0].discussion().and_then(place::Discussion::timeline).cloned().unwrap()
    }
}
