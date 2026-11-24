//! The client's own update, from its repository's latest release as sharkord's client takes its, and signed: a
//! release publishes `latest.json`, the release key's Ed25519 signature over it as `latest.json.sig`, and each
//! system's package beside them.
//!
//! A build has a [`Channel`] only when it carries its channel's public key (`release.pub` or `dev.pub`) and was
//! stamped with a feed (`CHATKORD_UPDATES`), and a run has a [`Target`] only when it is from a package this system's
//! half can replace (on Linux an AppImage, on Windows the setup's install). With both, [`check`] reads the manifest only once
//! its signature holds, and offers a release newer than the running one; [`fetch`] streams its package where this
//! system stages it (beside the AppImage; the user's temporary directory on Windows), checking its size and SHA-256 as
//! it arrives; [`place`] renames it over the AppImage on Linux, and on Windows keeps the setup, which
//! [`Target::start`] runs over the install once the window has ended. Every refusal
//! leaves the running package as it was and nothing fetched behind. `release-manifest` writes and signs the manifest
//! for a release with these same types.
#![forbid(unsafe_code)]

mod address;
mod channel;
mod error;
mod fetch;
#[cfg(test)]
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod flow_tests;
#[cfg(any(test, fuzzing))]
pub mod fuzz;
mod http;
mod key;
mod manifest;
mod package;
mod place;
pub mod release;
#[cfg(test)]
mod testing;
mod version;

pub use channel::{Channel, FEED_VARIABLE, NoChannel};
pub use error::UpdateError;
pub use fetch::{Fetched, Offer, Progress, check, fetch};
pub use key::{SIGNATURE_BYTES, SigningKey, SigningKeyRefused, VerifyingKey};
pub use manifest::{MANIFEST_FILE, MAX_MANIFEST_BYTES, Manifest, SIGNATURE_FILE, Signed};
pub use package::{Digest, MAX_FILE_NAME, MAX_PACKAGE_BYTES, Package, PackageName, PackageRefused, Platform};
pub use place::{Target, place};
pub use version::{Version, VersionRefused};
