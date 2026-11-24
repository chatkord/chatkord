//! The version this client carries, one owner for the handshake, the user agent, About and the debug report.

use proto::envelope::MAX_HANDSHAKE_TEXT;

/// The version this build carries: `CHATKORD_VERSION` at compile time when set (the version its package is named by,
/// from `scripts/release/next.py`), else the repository's `version.txt` (release-please writes it). What a client
/// says in `Handshake.client_build`, and what `--version`, About and a debug report show.
pub const BUILD: &str = carried(option_env!("CHATKORD_VERSION"), RELEASE);

/// The release `version.txt` names: the last one made, which a build from the tree alone carries.
const RELEASE: &str = include_str!("../../../version.txt").trim_ascii();

/// The version a build stamped with `stamped` carries, else `release`.
const fn carried(stamped: Option<&'static str>, release: &'static str) -> &'static str {
    match stamped {
        Some(version) => version,
        None => release,
    }
}

// A stamp a handshake could not carry stops the build that was given it.
const _: () = assert!(!BUILD.is_empty() && BUILD.len() <= MAX_HANDSHAKE_TEXT);

#[cfg(test)]
mod tests {
    use super::{RELEASE, carried};

    #[test]
    fn a_stamped_build_carries_its_stamp_and_any_other_the_release() {
        assert_eq!(carried(Some("0.1.0-dev.20261124120000"), RELEASE), "0.1.0-dev.20261124120000");
        assert_eq!(carried(None, RELEASE), RELEASE);
    }

    #[test]
    fn the_release_is_a_plain_version() {
        let parts: Vec<&str> = RELEASE.split('.').collect();
        assert_eq!(parts.len(), 3, "version.txt holds MAJOR.MINOR.PATCH, found {RELEASE:?}");
        for part in parts {
            assert!(
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()),
                "not a number: {part:?} in {RELEASE:?}"
            );
            assert!(part == "0" || !part.starts_with('0'), "a leading zero: {part:?} in {RELEASE:?}");
        }
    }
}
