//! Why a file was not sent or not fetched.

use crate::error::LinkError;
use proto::files::{FileKind, FileRefusal};
use std::fmt;

/// Why a file was not sent or not fetched. `Display` is for a log, not a person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    /// The server could not be reached, was not trusted, or did not answer as one of ours.
    Reach(LinkError),
    /// Refused, by the server's status or by this end's own check of the same bound.
    Refused {
        /// The kind of file asked for.
        kind: FileKind,
        /// Why it was refused.
        refusal: FileRefusal,
    },
    /// It said it was a file of its kind and could not be read as one.
    Unreadable {
        /// The kind it said it was.
        kind: FileKind,
    },
    /// This device's file, sent from or saved to a path, could not be read, written or put in place.
    Disk(std::io::ErrorKind),
}

/// Why the server's debug report was not had: the ask, or the download it names, failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportError {
    /// The server could not be asked or reached, did not answer as one of ours, refused the ask, or sent a report over
    /// what a report holds.
    Reach(LinkError),
    /// The download was refused, by the status the file routes answer it with.
    Refused(FileRefusal),
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reach(failed) => write!(f, "server report: {failed}"),
            Self::Refused(refusal) => write!(f, "server report refused: {}", refusal.status()),
        }
    }
}

impl std::error::Error for ReportError {}

/// What a server takes in files, as its greeting says its host set it; none where it said nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileLimits {
    /// The most bytes one attachment may be.
    pub attachment: Option<u64>,
    /// The most bytes one account's files may hold together.
    pub account: Option<u64>,
}

impl FileLimits {
    /// The limits as the greeting states them: an attachment's 0 says nothing, an absent account limit is none.
    pub fn of_wire(attachment: u64, account: Option<u64>) -> FileLimits {
        FileLimits { attachment: (attachment > 0).then_some(attachment), account }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reach(failed) => write!(f, "file request: {failed}"),
            Self::Refused { kind, refusal } => write!(f, "{} refused: {}", kind.word(), refusal.status()),
            Self::Unreadable { kind } => write!(f, "{} unreadable", kind.word()),
            Self::Disk(failed) => write!(f, "file on this device: {failed}"),
        }
    }
}

impl std::error::Error for FileError {}
