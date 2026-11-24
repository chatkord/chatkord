//! A language and the tag a choice of it is remembered by.

use crate::words::Language;
use choices::LanguageTag;
use unic_langid::LanguageIdentifier;

impl From<Language> for LanguageTag {
    fn from(language: Language) -> LanguageTag {
        LanguageTag::new(language.tag())
    }
}

impl Language {
    /// The carried language `tag` names; none for a tag this build has no catalog for.
    pub fn from_tag(tag: LanguageTag) -> Option<Language> {
        let id = tag.tag().parse::<LanguageIdentifier>().ok()?;
        Language::of(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_carried_language_is_remembered_by_its_tag_and_read_back() {
        for language in Language::all() {
            let tag = LanguageTag::from(language);
            assert_eq!(tag.tag(), language.tag());
            assert_eq!(Language::from_tag(tag), Some(language));
        }
        assert_eq!(Language::from_tag(LanguageTag::new("xx")), None, "a tag with no catalog");
    }
}
