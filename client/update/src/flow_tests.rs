//! A check, a fetch and a placing against a feed on this machine's loopback, over TLS 1.3 as a release host is
//! reached; every refusal leaves the package as it was and nothing it fetched staged.

use crate::channel::Channel;
use crate::error::UpdateError;
use crate::fetch::{Offer, Progress, check_for, fetch};
use crate::http::{MAX_REDIRECTS, Patience};
use crate::key::SigningKey;
use crate::manifest::Manifest;
use crate::package::{Package, Platform};
use crate::place::{Target, place};
use crate::testing::{Certificate, Feed, Reply, package, signing_key};
use crate::version::Version;
use link::Trust;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use system::{FILE, INSTALLED, PLATFORM};
use tempfile::TempDir;
use terms::LinkError;

const RUNNING: Version = Version::new(0, 1, 0);
const NEWER: Version = Version::new(0, 2, 0);
const NEW: &[u8] = b"the new package, whole.";
const OLD: &[u8] = b"the running package";

#[cfg(target_os = "linux")]
mod system {
    //! Linux's package: the AppImage, renamed over the running one.

    use crate::package::Platform;
    use crate::place::Target;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    pub(super) const PLATFORM: Platform = Platform::LinuxX86_64;
    pub(super) const FILE: &str = "Chatkord-0.2.0-x86_64.AppImage";
    pub(super) const INSTALLED: &str = "Chatkord.AppImage";

    /// The package in `directory`, its downloads staged beside it.
    pub(super) fn target(directory: &Path, _: &Path) -> Target {
        Target::at(directory.join(INSTALLED)).unwrap()
    }

    /// Where a package put in place is: the running one's own name.
    pub(super) fn placed(target: &Target) -> PathBuf {
        target.file().to_path_buf()
    }

    /// The running package was replaced: executable, and alone in its directory.
    pub(super) fn assert_replaced(target: &Target, directory: &Path) {
        assert_eq!(std::fs::metadata(target.file()).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(super::names(directory), [INSTALLED]);
    }
}

#[cfg(target_os = "windows")]
mod system {
    //! Windows' package: the setup, kept where downloads are staged; the install is left as it was.

    use crate::package::Platform;
    use crate::place::{SETUP, Target};
    use std::path::{Path, PathBuf};

    pub(super) const PLATFORM: Platform = Platform::WindowsX86_64;
    pub(super) const FILE: &str = "chatkord-setup-0.2.0.exe";
    pub(super) const INSTALLED: &str = "Chatkord.exe";

    /// The install in `directory`, its downloads staged in `staging`, which stands for this user's temporary
    /// directory.
    pub(super) fn target(directory: &Path, staging: &Path) -> Target {
        Target::at(directory.join(INSTALLED)).unwrap().staged_in(staging.to_path_buf())
    }

    /// Where a package put in place is: the setup's one name where downloads are staged.
    pub(super) fn placed(target: &Target) -> PathBuf {
        target.staging().join(SETUP)
    }

    /// The installed window is the running one still and alone, and the setup alone where it is kept.
    pub(super) fn assert_replaced(target: &Target, directory: &Path) {
        assert_eq!(std::fs::read(target.file()).unwrap(), super::OLD);
        assert_eq!(super::names(directory), [INSTALLED]);
        assert_eq!(super::names(target.staging()), [SETUP]);
    }
}

/// A feed serving a release, the key that signed it, the running package in a directory of its own, and a directory
/// for a system that stages its downloads elsewhere.
struct Stage {
    feed: Feed,
    certificate: Certificate,
    at: SocketAddr,
    key: SigningKey,
    directory: TempDir,
    staging: TempDir,
}

impl Stage {
    /// Release `version` carrying `package`, signed by its key, served over TLS.
    async fn new(version: Version, package: Package) -> Stage {
        let (feed, certificate, key) = (Feed::default(), Certificate::new(), signing_key());
        let at = feed.serve_tls(&certificate).await;
        let (directory, staging) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let stage = Stage { feed, certificate, at, key, directory, staging };
        stage.publish(version, package);
        std::fs::write(stage.target().file(), OLD).unwrap();
        stage
    }

    /// The release as published, signed by the stage's key, and its package's bytes at its name.
    fn publish(&self, version: Version, package: Package) {
        let signed = Manifest::new(version, BTreeMap::from([(PLATFORM, package)])).sign(&self.key).unwrap();
        self.feed.route("/download/latest.json", Reply::Bytes(signed.manifest().to_vec()));
        self.feed.route("/download/latest.json.sig", Reply::Bytes(signed.signature().to_vec()));
        self.feed.route(&format!("/download/{FILE}"), Reply::Bytes(NEW.to_vec()));
    }

    fn base(&self) -> String {
        format!("https://{}/download/", self.at)
    }

    fn channel(&self) -> Channel {
        self.channel_with(self.certificate.trust(), Patience::default())
    }

    fn channel_with(&self, trust: Trust, patience: Patience) -> Channel {
        Channel::at(&self.base(), self.key.verifying_key(), trust, patience)
    }

    fn target(&self) -> Target {
        system::target(self.directory.path(), self.staging.path())
    }

    async fn offer(&self) -> Offer {
        check_for(&self.channel(), RUNNING, PLATFORM).await.unwrap().unwrap()
    }

    async fn fetch(&self) -> Result<crate::fetch::Fetched, UpdateError> {
        fetch(&self.channel(), &self.offer().await, &self.target(), |_| {}).await
    }

    /// Where a package put in place is.
    fn placed(&self) -> PathBuf {
        system::placed(&self.target())
    }

    /// The package is the running one still, alone in its directory, and nothing is staged.
    fn untouched(&self) {
        let target = self.target();
        assert_eq!(std::fs::read(target.file()).unwrap(), OLD);
        assert_eq!(names(self.directory.path()), [INSTALLED]);
        assert!(names(target.staging()).iter().all(|name| name == INSTALLED), "nothing staged");
    }
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(directory).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

/// This system's package holding `bytes`.
fn packaged(bytes: &[u8]) -> Package {
    package(PLATFORM, FILE, bytes)
}

#[tokio::test]
async fn a_newer_release_is_fetched_checked_and_put_in_place() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let offer = stage.offer().await;
    assert_eq!(offer.version(), NEWER);
    let mut seen = Vec::new();
    let fetched = fetch(&stage.channel(), &offer, &stage.target(), |progress| seen.push(progress)).await.unwrap();
    assert_eq!(seen.last(), Some(&Progress { received: 23, total: 23 }));
    assert_eq!(fetched.file.parent(), Some(stage.target().staging()), "fetched where downloads are staged");
    assert_eq!(std::fs::read(stage.target().file()).unwrap(), OLD);
    place(fetched).await.unwrap();
    assert_eq!(std::fs::read(stage.placed()).unwrap(), NEW);
    system::assert_replaced(&stage.target(), stage.directory.path());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_package_put_in_place_starts_as_a_new_run_under_the_running_name() {
    let script = b"#!/bin/sh\nprintf '%s' \"$0\" > \"$(dirname \"$0\")/started\"\n";
    let stage = Stage::new(NEWER, packaged(script)).await;
    stage.feed.route(&format!("/download/{FILE}"), Reply::Bytes(script.to_vec()));
    place(stage.fetch().await.unwrap()).await.unwrap();
    stage.target().start().unwrap();
    let started = stage.directory.path().join("started");
    let mut waited = Duration::ZERO;
    while std::fs::read(&started).map_or(true, |said| said.is_empty()) && waited < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(10)).await;
        waited += Duration::from_millis(10);
    }
    assert_eq!(std::fs::read_to_string(&started).unwrap(), stage.target().file().to_str().unwrap());
}

#[tokio::test]
async fn a_release_redirected_to_another_host_is_followed_there() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let elsewhere = Feed::default();
    let there = elsewhere.serve_tls(&stage.certificate).await;
    elsewhere.route("/asset", Reply::Parts(NEW.to_vec()));
    stage.feed.route(&format!("/download/{FILE}"), Reply::Redirect(format!("https://{there}/asset")));
    place(stage.fetch().await.unwrap()).await.unwrap();
    assert_eq!(std::fs::read(stage.placed()).unwrap(), NEW);
    assert_eq!(elsewhere.asked(), ["/asset"]);
}

#[tokio::test]
async fn a_dropped_fetch_leaves_nothing_staged() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    drop(stage.fetch().await.unwrap());
    stage.untouched();
}

#[tokio::test]
async fn a_manifest_another_key_signed_is_refused_before_any_package_is_asked_for() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let trust = stage.certificate.trust();
    let other = Channel::at(&stage.base(), signing_key().verifying_key(), trust, Patience::default());
    assert_eq!(check_for(&other, RUNNING, PLATFORM).await, Err(UpdateError::BadSignature));
    assert!(!stage.feed.asked().iter().any(|path| path.ends_with(PLATFORM.suffix())));
    stage.untouched();
}

#[tokio::test]
async fn a_signature_of_any_other_length_is_refused() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    for length in [0, 63, 65, 4096] {
        stage.feed.route("/download/latest.json.sig", Reply::Bytes(vec![0; length]));
        let refused = check_for(&stage.channel(), RUNNING, PLATFORM).await;
        let expected = if length > 64 { UpdateError::TooLarge } else { UpdateError::BadSignature };
        assert_eq!(refused, Err(expected), "{length} bytes");
    }
}

#[tokio::test]
async fn a_manifest_over_its_bound_is_refused_whether_or_not_its_length_is_stated() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let over = vec![b' '; crate::manifest::MAX_MANIFEST_BYTES + 1];
    for reply in [Reply::Bytes(over.clone()), Reply::Parts(over)] {
        stage.feed.route("/download/latest.json", reply);
        assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Err(UpdateError::TooLarge));
    }
}

#[tokio::test]
async fn an_equal_or_older_release_is_no_offer() {
    for version in [RUNNING, Version::new(0, 0, 9)] {
        let stage = Stage::new(version, packaged(NEW)).await;
        assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Ok(None), "{version}");
    }
}

#[tokio::test]
async fn a_release_with_no_manifest_yet_is_no_offer_but_a_missing_signature_is_a_failure() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    stage.feed.route("/download/latest.json", Reply::Redirect("/elsewhere/latest.json".to_string()));
    assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Ok(None));
    stage.feed.route("/download/latest.json", Reply::Status(404));
    assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Ok(None));
    stage.publish(NEWER, packaged(NEW));
    stage.feed.route("/download/latest.json.sig", Reply::Status(404));
    assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Err(UpdateError::Status(404)));
    stage.feed.route("/download/latest.json", Reply::Status(500));
    assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Err(UpdateError::Status(500)));
}

#[tokio::test]
async fn a_manifest_read_before_its_new_signature_is_a_failure_and_the_next_check_offers_it() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let older = Manifest::new(RUNNING, BTreeMap::from([(PLATFORM, packaged(OLD))])).sign(&stage.key).unwrap();
    stage.feed.route("/download/latest.json.sig", Reply::Bytes(older.signature().to_vec()));
    assert_eq!(check_for(&stage.channel(), RUNNING, PLATFORM).await, Err(UpdateError::BadSignature));
    stage.publish(NEWER, packaged(NEW));
    assert_eq!(stage.offer().await.version(), NEWER);
}

#[tokio::test]
async fn a_release_with_no_package_for_this_system_is_no_offer() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let other = Platform::ALL.into_iter().find(|platform| *platform != PLATFORM).unwrap();
    assert_eq!(check_for(&stage.channel(), RUNNING, other).await, Ok(None));
}

#[tokio::test]
async fn a_package_of_another_hash_is_refused_and_removed() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let forged = b"the new package, wh0le.";
    assert_eq!(forged.len(), NEW.len());
    stage.feed.route(&format!("/download/{FILE}"), Reply::Bytes(forged.to_vec()));
    assert_eq!(stage.fetch().await.err(), Some(UpdateError::WrongHash));
    stage.untouched();
}

#[tokio::test]
async fn a_package_longer_than_its_release_says_is_refused_stated_or_not() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let longer = [NEW, b"and more"].concat();
    for reply in [Reply::Bytes(longer.clone()), Reply::Parts(longer)] {
        stage.feed.route(&format!("/download/{FILE}"), reply);
        assert_eq!(stage.fetch().await.err(), Some(UpdateError::TooLarge));
        stage.untouched();
    }
}

#[tokio::test]
async fn a_package_shorter_than_its_release_says_is_refused() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let shorter = NEW[..10].to_vec();
    let stated = u64::try_from(NEW.len()).unwrap();
    for (reply, refusal) in [
        (Reply::Bytes(shorter.clone()), UpdateError::Short),
        (Reply::Parts(shorter.clone()), UpdateError::Short),
        (Reply::Stating(stated, shorter), UpdateError::Lost),
    ] {
        stage.feed.route(&format!("/download/{FILE}"), reply);
        assert_eq!(stage.fetch().await.err(), Some(refusal));
        stage.untouched();
    }
}

#[tokio::test]
async fn a_redirect_to_plain_http_is_refused() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let plain = Feed::default();
    let there = plain.serve_plain().await;
    plain.route("/asset", Reply::Bytes(NEW.to_vec()));
    stage.feed.route(&format!("/download/{FILE}"), Reply::Redirect(format!("http://{there}/asset")));
    assert_eq!(stage.fetch().await.err(), Some(UpdateError::BadRedirect));
    assert!(plain.asked().is_empty());
    stage.untouched();
}

#[tokio::test]
async fn redirects_are_followed_up_to_their_bound_and_no_further() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let chain = |hops: usize| {
        for hop in 0..hops {
            let from = if hop == 0 { format!("/download/{FILE}") } else { format!("/hop{hop}") };
            stage.feed.route(&from, Reply::Redirect(format!("/hop{}", hop + 1)));
        }
        stage.feed.route(&format!("/hop{hops}"), Reply::Bytes(NEW.to_vec()));
    };
    chain(MAX_REDIRECTS);
    drop(stage.fetch().await.unwrap());
    chain(MAX_REDIRECTS + 1);
    assert_eq!(stage.fetch().await.err(), Some(UpdateError::TooManyRedirects));
    stage.untouched();
}

#[tokio::test]
async fn a_refusal_status_is_said_as_it_came() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    stage.feed.route(&format!("/download/{FILE}"), Reply::Status(404));
    assert_eq!(stage.fetch().await.err(), Some(UpdateError::Status(404)));
    stage.untouched();
}

#[tokio::test]
async fn a_feed_whose_certificate_no_authority_vouches_for_is_refused() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let unpinned = stage.channel_with(Trust::system(), Patience::default());
    let refused = check_for(&unpinned, RUNNING, PLATFORM).await;
    assert!(matches!(refused, Err(UpdateError::Reach(LinkError::Unvouched(_) | LinkError::Untrusted))), "{refused:?}");
    assert!(stage.feed.asked().is_empty(), "nothing was asked of it");
}

#[tokio::test]
async fn a_download_that_stalls_is_given_up() {
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let offer = stage.offer().await;
    stage.feed.route(&format!("/download/{FILE}"), Reply::Stall);
    let patience = Patience { connect: Duration::from_secs(5), stall: Duration::from_millis(200) };
    let channel = stage.channel_with(stage.certificate.trust(), patience);
    assert_eq!(fetch(&channel, &offer, &stage.target(), |_| {}).await.err(), Some(UpdateError::Timeout));
    stage.untouched();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_directory_that_cannot_be_written_is_said_so() {
    use std::os::unix::fs::PermissionsExt;
    let stage = Stage::new(NEWER, packaged(NEW)).await;
    let directory = stage.directory.path();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    let refused = stage.fetch().await.err();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(refused, Some(UpdateError::Unwritable));
    stage.untouched();
}

#[tokio::test]
async fn a_feed_on_this_machines_loopback_is_read_in_plain_http() {
    let (feed, key) = (Feed::default(), signing_key());
    let at = feed.serve_plain().await;
    let signed = Manifest::new(NEWER, BTreeMap::from([(PLATFORM, packaged(NEW))])).sign(&key).unwrap();
    feed.route("/latest.json", Reply::Bytes(signed.manifest().to_vec()));
    feed.route("/latest.json.sig", Reply::Bytes(signed.signature().to_vec()));
    let channel = Channel::at(&format!("http://{at}/"), key.verifying_key(), Trust::system(), Patience::default());
    let offer = check_for(&channel, RUNNING, PLATFORM).await.unwrap();
    assert_eq!(offer.map(|offer| offer.version()), Some(NEWER));
}
