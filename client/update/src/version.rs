//! A release's version: MAJOR.MINOR.PATCH, and on the dev channel a dev stamp after it, ordered as the numbers are.

use channel::Channel;
use std::fmt;
use std::str::FromStr;

/// What a dev version carries after its numbers, before the stamp.
const DEV_PREFIX: &str = "-dev.";

/// A dev release's build time: 14 decimal digits, `yyyymmddHHMMSS` in UTC, which order as the time does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Stamp(u64);

impl Stamp {
    /// The digits a stamp is written with.
    const DIGITS: usize = 14;

    fn parse(text: &str) -> Result<Stamp, VersionRefused> {
        if text.len() != Stamp::DIGITS || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(VersionRefused);
        }
        text.parse().map(Stamp).map_err(|_| VersionRefused)
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:0width$}", self.0, width = Stamp::DIGITS)
    }
}

/// A release's version. A release's build carries one, and a build from the tree alone the last release's
/// (`version.txt`), so the next release is newer than it and an equal one is not. A stable version is its three
/// numbers alone; a dev version (`terms::BUILD` on the dev channel) adds the stamp of its build, so dev releases of
/// one triple are ordered by it and a higher triple outranks every stamp of a lower one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    stamp: Option<Stamp>,
}

/// Why a version was refused: on the stable channel anything but three decimal numbers joined by dots, on the dev
/// channel those followed by `-dev.` and a 14-digit stamp; each number without a leading zero and within `u32`. Any
/// other suffix, and the other channel's form, is refused, so no release is ever ordered by one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRefused;

impl fmt::Display for VersionRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a version of this channel's form")
    }
}

impl std::error::Error for VersionRefused {}

impl Version {
    /// The longest text a version can be: three `u32`s, two dots and a dev stamp's suffix.
    pub const MAX_LEN: usize = 3 * 10 + 2 + DEV_PREFIX.len() + Stamp::DIGITS;

    /// `major.minor.patch` as this build's channel writes it: on the dev channel with one fixed stamp, so a test's
    /// versions order by their numbers on either channel.
    #[cfg(test)]
    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Version {
        let stamp = match channel::THIS.channel {
            Channel::Stable => None,
            Channel::Dev => Some(Stamp(20_260_101_000_000)),
        };
        Version { major, minor, patch, stamp }
    }

    /// This build's version, `terms::BUILD`.
    pub fn of_this_build() -> Result<Version, VersionRefused> {
        terms::BUILD.parse()
    }

    /// `text` as a version of `channel`: a stable one has no suffix, a dev one has its stamp.
    fn parse(channel: Channel, text: &str) -> Result<Version, VersionRefused> {
        if text.len() > Version::MAX_LEN {
            return Err(VersionRefused);
        }
        let (numbers, stamp) = match channel {
            Channel::Stable => (text, None),
            Channel::Dev => {
                let (numbers, stamp) = text.split_once(DEV_PREFIX).ok_or(VersionRefused)?;
                (numbers, Some(Stamp::parse(stamp)?))
            }
        };
        let mut parts = numbers.split('.').map(number);
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(major), Some(minor), Some(patch), None) => {
                Ok(Version { major: major?, minor: minor?, patch: patch?, stamp })
            }
            _ => Err(VersionRefused),
        }
    }
}

impl FromStr for Version {
    type Err = VersionRefused;

    /// Parses as this build's channel does.
    fn from_str(text: &str) -> Result<Version, VersionRefused> {
        Version::parse(channel::THIS.channel, text)
    }
}

/// One part: decimal digits, no leading zero but a lone `0`.
fn number(part: &str) -> Result<u32, VersionRefused> {
    let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits || (part.len() > 1 && part.starts_with('0')) {
        return Err(VersionRefused);
    }
    part.parse().map_err(|_| VersionRefused)
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match self.stamp {
            Some(stamp) => write!(f, "{DEV_PREFIX}{stamp}"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_build_has_a_version_unless_a_dev_build_was_given_none() {
        // A dev build from the tree alone carries version.txt's release, no dev version, so it offers itself none.
        let expected = match (channel::THIS.channel, option_env!("CHATKORD_VERSION")) {
            (Channel::Dev, None) => Err(VersionRefused),
            _ => Ok(terms::BUILD.to_string()),
        };
        assert_eq!(Version::of_this_build().map(|version| version.to_string()), expected);
    }

    fn stable(text: &str) -> Result<Version, VersionRefused> {
        Version::parse(Channel::Stable, text)
    }

    fn dev(text: &str) -> Result<Version, VersionRefused> {
        Version::parse(Channel::Dev, text)
    }

    #[test]
    fn versions_are_ordered_by_their_numbers_not_their_text() {
        let parse = |text: &str| stable(text).unwrap();
        assert!(parse("0.10.0") > parse("0.9.9"));
        assert!(parse("1.0.0") > parse("0.99.99"));
        assert!(parse("0.1.10") > parse("0.1.2"));
        assert_eq!(parse("4294967295.0.1").to_string(), "4294967295.0.1");
    }

    #[test]
    fn anything_but_three_plain_numbers_is_refused() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-rc.1",
            "1.2.3+build",
            "v1.2.3",
            " 1.2.3",
            "1..3",
            "1.2.",
            "4294967296.0.0",
            "-1.2.3",
            "+1.2.3",
            "1.2.٣",
        ] {
            assert_eq!(stable(text), Err(VersionRefused), "{text:?}");
        }
        assert_eq!(stable(&"1.2.3".repeat(10)), Err(VersionRefused));
    }

    #[test]
    fn dev_versions_are_ordered_by_their_triple_and_then_their_stamp() {
        let parse = |text: &str| dev(text).unwrap();
        assert!(parse("0.1.0-dev.20261124194418") > parse("0.1.0-dev.20261124194417"));
        assert!(parse("0.1.0-dev.20270101000000") > parse("0.1.0-dev.20261231235959"));
        assert!(parse("0.1.1-dev.20200101000000") > parse("0.1.0-dev.20991231235959"));
        assert!(parse("1.0.0-dev.20200101000000") > parse("0.99.99-dev.20991231235959"));
        assert_eq!(parse("0.1.0-dev.20261124194418"), parse("0.1.0-dev.20261124194418"));
    }

    #[test]
    fn a_dev_version_is_written_back_as_it_was_read() {
        for text in ["0.1.0-dev.20261124194418", "4294967295.4294967295.4294967295-dev.00000000000001"] {
            assert_eq!(dev(text).unwrap().to_string(), text);
            assert!(text.len() <= Version::MAX_LEN);
        }
        assert_eq!(stable("1.2.3").unwrap().to_string(), "1.2.3");
    }

    #[test]
    fn the_stable_channel_refuses_every_suffix_and_the_dev_channel_everything_but_its_stamp() {
        for text in ["0.1.0-dev.20261124194418", "1.2.3-dev.1", "1.2.3-dev."] {
            assert_eq!(stable(text), Err(VersionRefused), "{text:?}");
        }
        for text in [
            "0.1.0",
            "0.1.0-dev",
            "0.1.0-dev.",
            "0.1.0-dev.2026112419441",
            "0.1.0-dev.202611241944180",
            "0.1.0-dev.2026112419441a",
            "0.1.0-dev.+0261124194418",
            "0.1.0-dev.٢0261124194418",
            "0.1.0-rc.20261124194418",
            "0.1.0-dev.20261124194418-dev.20261124194418",
            "0.1.0-dev.20261124194418+build",
            "0.1.0+dev.20261124194418",
            "0.1-dev.20261124194418",
            "01.1.0-dev.20261124194418",
            "-dev.20261124194418",
            "",
        ] {
            assert_eq!(dev(text), Err(VersionRefused), "{text:?}");
        }
    }
}
