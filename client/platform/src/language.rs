//! The languages the window may be said in: the ones the system names, most wanted first, and the one chosen in
//! Settings › Appearance, kept for the next run as its tag alone, so a tag this build carries no catalog for is the
//! system's languages again.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use serde::{Deserialize, Serialize};
use words::{Language, LanguageIdentifier};

const FILE: &str = "language";

/// The file: the chosen language's tag, absent to match the system's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    chosen: Option<String>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

impl Held {
    fn language(&self) -> Option<Language> {
        let id = self.chosen.as_deref()?.parse::<LanguageIdentifier>().ok()?;
        Language::of(&id)
    }

    fn of(language: Option<Language>) -> Held {
        Held { chosen: language.map(|language| language.tag().to_string()) }
    }
}

/// The languages the system names for this person, most wanted first: Windows' display languages, the locale
/// variables (`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`) on Linux. None where it names none.
pub fn preferred() -> Vec<LanguageIdentifier> {
    identified(sys_locale::get_locales())
}

/// `tags` as identifiers, each a BCP 47 tag the system gave; one that names no language (POSIX's `C`, an empty one,
/// `und`) is left out.
fn identified(tags: impl Iterator<Item = String>) -> Vec<LanguageIdentifier> {
    tags.filter_map(|tag| tag.parse::<LanguageIdentifier>().ok()).filter(|id| !id.language.is_empty()).collect()
}

/// The language chosen for the window; none to match the system's, and where nothing was kept.
pub async fn chosen(platform: &Platform) -> Option<Language> {
    read_kept::<Held>(platform).await.language()
}

/// Keeps `language` for the next run; none to match the system's.
pub async fn keep(platform: &Platform, language: Option<Language>) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(language)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-language-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn every_carried_language_reads_back_as_kept_and_matching_the_system_writes_nothing() {
        let folder = scratch("round");
        for language in Language::all() {
            write_kept_in(&folder, &Held::of(Some(language))).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).language(), Some(language));
        }
        write_kept_in(&folder, &Held::of(None)).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}");
        assert_eq!(read_kept_in::<Held>(&folder).language(), None);
    }

    #[test]
    fn a_tag_this_build_carries_no_catalog_for_or_not_a_tag_is_the_systems_languages() {
        let folder = scratch("odd");
        for settings in
            [r#"{"chosen":"xx-XX"}"#, r#"{"chosen":"en-US"}"#, r#"{"chosen":"not a tag"}"#, r#"{"chosen":7}"#]
        {
            raw_kept_in::<Held>(&folder, settings).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).language(), None, "{settings}");
        }
        raw_kept_in::<Held>(&folder, r#"{"chosen":"en"}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).language(), Some(Language::ENGLISH));
    }

    #[test]
    fn the_systems_tags_are_read_as_languages_and_what_names_none_is_left_out() {
        let tags = ["fr-CA", "C", "", "und", "en-US", "zh-Hant-TW", "not a tag"].map(String::from);
        let read: Vec<String> = identified(tags.into_iter()).iter().map(ToString::to_string).collect();
        assert_eq!(read, ["fr-CA", "en-US", "zh-Hant-TW"]);
    }

    // Every Windows installation has a display language, so an empty read is the read failing.
    #[cfg(windows)]
    #[test]
    fn windows_names_its_display_languages() {
        let read = preferred();
        assert!(!read.is_empty());
        eprintln!("the system's languages: {read:?}");
    }
}
