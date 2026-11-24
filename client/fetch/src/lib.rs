//! The client's bounded HTTPS GET, and what is built on it.
//!
//! [`get`] is the fetch both users share: a GET on a connection of its own, redirects followed up to a bound, each
//! wait limited by [`Patience`], the body read a part at a time and refused past a bound. Where it connects and which
//! redirects it follows is a [`Route`]'s: the update feed's is in `update`, a link preview's is [`preview`]'s.
//!
//! [`preview`] is the sender's client making a link's preview: https on port 443 only, every address the name
//! resolves to [`vet`]ted as globally routable before a connection is opened to one of them, every redirect
//! vetted again, a page scanned as it arrives (at most [`MAX_PAGE_BYTES`] read, none kept) for the title, description
//! and picture of its `<head>`, the picture fetched the same way within [`MAX_PICTURE_BYTES`], all within one deadline.
//! Names and sockets come from a [`Reach`]: [`Internet`] in the client, a stand-in in a test.
#![forbid(unsafe_code)]

#[cfg(any(test, fuzzing))]
pub mod fuzz;
mod get;
mod guard;
mod net;
mod preview;
mod scan;
#[cfg(test)]
mod tests;
mod url;

pub use get::{Answer, GetError, Io, Patience, Route, Stream, get};
pub use guard::{Refused, Vetted, vet};
pub use net::{Internet, Reach};
pub use preview::{
    MAX_PAGE_BYTES, MAX_PICTURE_BYTES, MAX_REDIRECTS, MediaType, Picture, Preview, PreviewError, preview,
};
pub use scan::{MAX_DESCRIPTION, MAX_TITLE};
