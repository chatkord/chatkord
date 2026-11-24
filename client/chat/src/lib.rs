//! A timeline as the client holds it: its messages read at the edge and kept newest-copy-wins, the rows a list shows,
//! which of them to build and where the list is scrolled, a body's marks and markdown read into styled runs, and the
//! draft being written. Values in, values out: no toolkit, no OS, no clock and no time zone of its own.
#![forbid(unsafe_code)]

mod body;
mod draft;
pub mod emoji;
mod heights;
mod link;
mod mark;
mod markdown;
mod message;
mod rows;
mod search;
mod timeline;

pub use body::{Body, Run, RunKind};
pub use draft::{COUNT_SHOWN_WITHIN, Completion, Draft, EMOJI_QUERY_FROM, Query, Trigger};
pub use heights::{Heights, Scroll, Window};
pub use jiff::civil::{Date, DateTime};
pub use jiff::tz::TimeZone;
pub use mark::{CustomEmoji, EmojiId, Mark, MarkKind, MarksRefused, link_opens};
pub use markdown::{Code, SpoilerId, Style};
pub use message::{
    Attachment, Author, Card, CardPicture, Message, MessageId, Picture, Reaction, ReactionEmoji, Reply, ThreadSummary,
    Unread, Withdrawn, spans_of,
};
pub use rows::{RUN_SECONDS, Row, RowKey, local, rows, tomorrow_at};
pub use search::{Found, Home, Results};
pub use timeline::{End, MAX_KEPT, Page, Timeline};
