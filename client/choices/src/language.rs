//! The language a person chose for the window, as the tag it is remembered by.

/// A language's tag ("en", "pt-BR"), the value a chosen language is remembered as; `words` makes one from its
/// language and reads it back, so this crate never needs the catalogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageTag(&'static str);

impl LanguageTag {
    /// `tag`, unchecked: a tag `words` carries no catalog for reads back as no language, which is the system's.
    pub const fn new(tag: &'static str) -> LanguageTag {
        LanguageTag(tag)
    }

    /// The tag, as it was made.
    pub const fn tag(self) -> &'static str {
        self.0
    }
}
