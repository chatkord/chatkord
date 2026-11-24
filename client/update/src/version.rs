//! A release's version: MAJOR.MINOR.PATCH and nothing else, ordered as the numbers are.

use std::fmt;
use std::str::FromStr;

/// A release's version. A release's build carries one, and a build from the tree alone the last release's
/// (`version.txt`), so the next release is newer than it and an equal one is not; a dev release's build carries its
/// pre-release (`terms::BUILD`), which is refused, so it offers itself no update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

/// Why a version was refused: anything but three decimal numbers joined by dots, each without a leading zero and
/// within `u32`. A pre-release or build suffix is refused too, so no release is ever ordered by one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRefused;

impl fmt::Display for VersionRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a version of the form MAJOR.MINOR.PATCH")
    }
}

impl std::error::Error for VersionRefused {}

impl Version {
    /// The longest text a version can be: three `u32`s and two dots.
    pub const MAX_LEN: usize = 3 * 10 + 2;

    /// The version `major.minor.patch`.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version { major, minor, patch }
    }

    /// This build's version, `terms::BUILD`; refused for a dev release's.
    pub fn of_this_build() -> Result<Version, VersionRefused> {
        terms::BUILD.parse()
    }
}

impl FromStr for Version {
    type Err = VersionRefused;

    fn from_str(text: &str) -> Result<Version, VersionRefused> {
        if text.len() > Version::MAX_LEN {
            return Err(VersionRefused);
        }
        let mut parts = text.split('.').map(number);
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(major), Some(minor), Some(patch), None) => Ok(Version::new(major?, minor?, patch?)),
            _ => Err(VersionRefused),
        }
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
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_build_has_a_version() {
        assert_eq!(Version::of_this_build().map(|version| version.to_string()), Ok(terms::BUILD.to_string()));
    }

    #[test]
    fn versions_are_ordered_by_their_numbers_not_their_text() {
        let parse = |text: &str| text.parse::<Version>().unwrap();
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
            assert_eq!(text.parse::<Version>(), Err(VersionRefused), "{text:?}");
        }
        assert_eq!("1.2.3".repeat(10).parse::<Version>(), Err(VersionRefused));
    }
}
