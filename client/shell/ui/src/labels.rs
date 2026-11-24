//! Labels made from a phrase or a name.

use design::Label;
use words::{Phrase, Words};

/// A label made from a name or a sentence a name is in, or "?" for one a label cannot hold (a name the server bounds
/// to 64 characters is one): a screen shows what it was given, and never fails to draw it. A phrase is
/// [`Words::label`].
#[expect(clippy::expect_used, reason = "one character is a valid label")]
pub(crate) fn label(said: &str) -> Label {
    Label::new(said).unwrap_or_else(|_| Label::new("?").expect("one character is a label"))
}

pub(crate) fn phrase(said: Phrase, words: &Words) -> Label {
    words.label(said)
}
