//! The terms a connection to a server is spoken in: where the server is, who it says it is, the ids it names things
//! by, a secret, an invite, how a request or the connection ended, and what the server tells. Plain values, so what
//! decides a screen names them with no network beneath it; `link` speaks them.
#![forbid(unsafe_code)]

mod address;
mod build;
mod cursor;
mod error;
mod files;
#[cfg(any(test, fuzzing))]
pub mod fuzz;
mod heard;
mod history;
pub mod id;
mod identity;
mod invite;
mod release;
mod secret;
mod server_report;

pub use address::{Address, AddressError, ServerUrl};
pub use build::{BuildRefused, ServerBuild};
pub use cursor::Cursor;
pub use error::{Ended, Kind as ErrorKind, LinkError, ServerIs};
pub use files::{FileError, FileLimits, ReportError};
pub use heard::{Heard, Invited};
pub use history::{Anchor, Nonce};
pub use id::{
    CategoryId, ChannelId, ConversationId, Id, InviteId, Kind, MemberId, MessageId, RoleId, TimelineId, WebhookId,
};
pub use identity::ServerIdentity;
pub use invite::InviteLink;
pub use release::BUILD;
pub use secret::Secret;
pub use server_report::ArrivedReport;
