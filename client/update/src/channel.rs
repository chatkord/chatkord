//! Where this build looks for its next release and whose signature it takes: both compiled in, or there is no channel.

use crate::address::{Address, Loopback};
use crate::http::Patience;
use crate::key::VerifyingKey;
use link::Trust;
use std::ffi::OsStr;
use std::fmt;

/// This channel's public key, from the file `channel::THIS.update_key` names when the build had it (`build.rs`).
const RELEASE_KEY: Option<[u8; VerifyingKey::LEN]> = include!(concat!(env!("OUT_DIR"), "/release_key.rs"));

/// The feed this build was stamped with at build time: the directory of a repository's latest release.
const STAMPED: Option<&str> = option_env!("CHATKORD_UPDATES");

/// The variable naming, at run time, another feed for a stamped build: for a check by hand. This channel's key still
/// decides what is taken from it.
pub const FEED_VARIABLE: &str = "CHATKORD_UPDATES_FEED";

/// A feed and the key its manifest must be signed by.
#[derive(Clone, Debug)]
pub struct Channel {
    pub(crate) base: Address,
    pub(crate) key: VerifyingKey,
    pub(crate) trust: Trust,
    pub(crate) patience: Patience,
}

/// Why this build has no update channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoChannel {
    /// It was built without this channel's public key, so no update could be verified.
    NoReleaseKey,
    /// It was built without a feed (`CHATKORD_UPDATES`): a local build.
    NotStamped,
    /// Its feed is not an `https` directory, nor, named at run time, a plain one on this machine's loopback.
    BadFeed,
}

impl fmt::Display for NoChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NoChannel::NoReleaseKey => "no updates: this build carries no update key",
            NoChannel::NotStamped => "no updates: this build names no feed",
            NoChannel::BadFeed => "no updates: the feed is not an https directory",
        })
    }
}

impl std::error::Error for NoChannel {}

impl Channel {
    /// This build's channel: its feed, or the one [`FEED_VARIABLE`] names, checked under the system's verifier
    /// alone (a key `trust` keeps for a server is not taken for a feed's host), and this channel's key.
    pub fn of_this_build(trust: &Trust) -> Result<Channel, NoChannel> {
        let feed = std::env::var_os(FEED_VARIABLE);
        Channel::of(RELEASE_KEY.map(VerifyingKey::new), STAMPED, feed.as_deref(), trust.for_server(None))
    }

    /// The channel of `key` and `stamped`, its feed `named` instead when one is.
    fn of(
        key: Option<VerifyingKey>,
        stamped: Option<&str>,
        named: Option<&OsStr>,
        trust: Trust,
    ) -> Result<Channel, NoChannel> {
        let key = key.ok_or(NoChannel::NoReleaseKey)?;
        let stamped = stamped.filter(|stamped| !stamped.is_empty()).ok_or(NoChannel::NotStamped)?;
        let base = match named.filter(|named| !named.is_empty()) {
            Some(named) => Address::base(named.to_str().ok_or(NoChannel::BadFeed)?, Loopback::Allowed),
            None => Address::base(stamped, Loopback::Refused),
        };
        let base = base.map_err(|_| NoChannel::BadFeed)?;
        Ok(Channel { base, key, trust, patience: Patience::default() })
    }

    /// A channel to `base` (`https`, or plain on a loopback IP), trusting `key`, its connections checked under
    /// `trust` and waiting as `patience` says.
    #[cfg(test)]
    pub(crate) fn at(base: &str, key: VerifyingKey, trust: Trust, patience: Patience) -> Channel {
        let base = Address::base(base, Loopback::Allowed).unwrap();
        Channel { base, key, trust, patience }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    const KEY: VerifyingKey = VerifyingKey::new([9; VerifyingKey::LEN]);
    const FEED: &str = "https://example.invalid/o/r/releases/latest/download/";

    fn of(key: Option<VerifyingKey>, stamped: Option<&str>, named: Option<&str>) -> Result<Address, NoChannel> {
        Channel::of(key, stamped, named.map(OsStr::new), Trust::system()).map(|channel| channel.base)
    }

    #[test]
    fn a_channel_needs_the_release_key_whatever_feed_is_named() {
        assert_eq!(of(None, Some(FEED), None), Err(NoChannel::NoReleaseKey));
        assert_eq!(of(None, Some(FEED), Some("http://127.0.0.1/")), Err(NoChannel::NoReleaseKey));
    }

    #[test]
    fn a_channel_needs_a_stamped_feed_which_a_named_one_may_stand_in_for() {
        assert_eq!(of(Some(KEY), None, None), Err(NoChannel::NotStamped));
        assert_eq!(of(Some(KEY), Some(""), None), Err(NoChannel::NotStamped));
        assert_eq!(of(Some(KEY), None, Some("http://127.0.0.1/")), Err(NoChannel::NotStamped));
        assert_eq!(of(Some(KEY), Some(FEED), None).map(|base| base.uri().to_string()), Ok(FEED.to_string()));
        let named = of(Some(KEY), Some(FEED), Some("http://127.0.0.1:8080/")).map(|base| base.uri().to_string());
        assert_eq!(named, Ok("http://127.0.0.1:8080/".to_string()));
        assert_eq!(of(Some(KEY), Some(FEED), Some("")).map(|base| base.uri().to_string()), Ok(FEED.to_string()));
    }

    #[test]
    fn a_feed_out_of_shape_is_no_channel() {
        assert_eq!(of(Some(KEY), Some("http://example.invalid/"), None), Err(NoChannel::BadFeed));
        assert_eq!(of(Some(KEY), Some("http://127.0.0.1/"), None), Err(NoChannel::BadFeed));
        assert_eq!(of(Some(KEY), Some(FEED), Some("http://example.invalid/")), Err(NoChannel::BadFeed));
        let channel = Channel::of(Some(KEY), Some(FEED), Some(&unreadable()), Trust::system());
        assert_eq!(channel.map(|_| ()), Err(NoChannel::BadFeed));
    }

    /// A feed's name that is no text: a byte no UTF-8 starts with.
    #[cfg(unix)]
    fn unreadable() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff, b'/'])
    }

    /// A feed's name that is no text: a UTF-16 surrogate with no pair.
    #[cfg(windows)]
    fn unreadable() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800, u16::from(b'/')])
    }

    #[test]
    fn this_build_has_a_channel_only_with_both_its_release_key_and_a_feed() {
        let expected = match (RELEASE_KEY, STAMPED.filter(|stamped| !stamped.is_empty())) {
            (None, _) => Err(NoChannel::NoReleaseKey),
            (Some(_), None) => Err(NoChannel::NotStamped),
            (Some(_), Some(_)) => Ok(()),
        };
        assert_eq!(Channel::of_this_build(&Trust::system()).map(|_| ()), expected);
    }

    #[test]
    fn the_feed_this_build_was_stamped_with_is_one() {
        if let Some(stamped) = STAMPED.filter(|stamped| !stamped.is_empty()) {
            assert!(Address::base(stamped, Loopback::Refused).is_ok(), "CHATKORD_UPDATES is not an https directory");
        }
    }
}
