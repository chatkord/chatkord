//! Why an update was not checked, fetched or put in place. `Display` is the operator's; a person reads the window's
//! words for the variant.

use std::fmt;
use std::io;
use terms::LinkError;

/// Why an update did not happen. Every refusal leaves the running package as it was and no fetched file behind.
#[derive(Clone, Debug, PartialEq, Eq, report::Public)]
pub enum UpdateError {
    /// This system has no package a release carries, or no way here to put one in place.
    NotOffered,
    /// This run is not from a package an update replaces: on Linux, not from an AppImage; on Windows, not from the
    /// setup's install.
    NotPackaged,
    /// The feed's host could not be reached or did not prove who it is: the connection's own failure.
    Reach(LinkError),
    /// The feed's host took too long to connect, to answer or to send the next part of an answer.
    Timeout,
    /// The connection ended before the answer did.
    Lost,
    /// The feed answered with this status, neither the file nor a redirect.
    Status(#[public] u16),
    /// A redirect led to no `https` address.
    BadRedirect,
    /// More redirects than a fetch follows.
    TooManyRedirects,
    /// An answer longer than its bound or than the manifest says the package is.
    TooLarge,
    /// The package ended before the size the manifest says it is.
    Short,
    /// The manifest's signature is not the release key's over its exact bytes.
    BadSignature,
    /// The manifest is signed but not of the form this build reads.
    Malformed,
    /// The package's SHA-256 is not the one the manifest says.
    WrongHash,
    /// The directory the package is in cannot be written, so nothing can be put in its place.
    Unwritable,
    /// Reading or writing the package's file failed.
    Disk(io::ErrorKind),
    /// The runtime ended before the work on the package's file did.
    Stopped,
    /// The new run could not be started: on Linux the package in place, on Windows the setup kept to install it.
    NotStarted(io::ErrorKind),
}

impl UpdateError {
    /// A file operation's failure: a directory this user may not write is its own refusal.
    pub(crate) fn of_disk(failed: &io::Error) -> UpdateError {
        match failed.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => UpdateError::Unwritable,
            kind => UpdateError::Disk(kind),
        }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::NotOffered => f.write_str("updates are not offered on this system"),
            UpdateError::NotPackaged => f.write_str("this run is not from a package an update replaces"),
            UpdateError::Reach(failed) => write!(f, "the update feed: {failed}"),
            UpdateError::Timeout => f.write_str("the update feed took too long to answer"),
            UpdateError::Lost => f.write_str("the connection to the update feed ended early"),
            UpdateError::Status(status) => write!(f, "the update feed answered {status}"),
            UpdateError::BadRedirect => f.write_str("the update feed redirected to an address that is not https"),
            UpdateError::TooManyRedirects => f.write_str("the update feed redirected too many times"),
            UpdateError::TooLarge => f.write_str("an answer from the update feed was over its bound"),
            UpdateError::Short => f.write_str("the package ended before its stated size"),
            UpdateError::BadSignature => f.write_str("the release manifest is not signed by the release key"),
            UpdateError::Malformed => f.write_str("the release manifest is not of a form this build reads"),
            UpdateError::WrongHash => f.write_str("the package's SHA-256 is not the manifest's"),
            UpdateError::Unwritable => f.write_str("the package's directory cannot be written"),
            UpdateError::Disk(kind) => write!(f, "the package's file: {kind}"),
            UpdateError::Stopped => f.write_str("the runtime ended before the package's file was done"),
            UpdateError::NotStarted(kind) => write!(f, "the new run did not start: {kind}"),
        }
    }
}

impl std::error::Error for UpdateError {}
