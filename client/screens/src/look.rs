//! The words of how the window looks, and the runs Appearance previews a message's size and spacing on.

use design::{PersonColour, Preference, Swatch};
use words::Phrase;

/// A theme's name on the page.
pub fn theme_phrase(theme: Preference) -> Phrase {
    match theme {
        Preference::System => Phrase::ThemeSystem,
        Preference::Light => Phrase::ThemeLight,
        Preference::Dark => Phrase::ThemeDark,
    }
}

/// A run of messages Appearance's preview draws, as the channel draws one: its author, their colour, its time and its
/// lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewRun {
    /// Its author's name.
    pub author: Phrase,
    /// Its author's colour.
    pub colour: PersonColour,
    /// When its first message was sent.
    pub at: chat::DateTime,
    /// Its messages' words, oldest first.
    pub lines: &'static [Phrase],
}

/// The two runs Appearance previews, as the canvas draws them: a question and its answer.
pub const PREVIEW: [PreviewRun; 2] = [
    PreviewRun {
        author: Phrase::PreviewFirstAuthor,
        colour: PersonColour::Swatch(Swatch::Clay),
        at: chat::Date::constant(2026, 1, 1).at(20, 58, 0, 0),
        lines: &[Phrase::PreviewFirstLine, Phrase::PreviewSecondLine],
    },
    PreviewRun {
        author: Phrase::PreviewReplyAuthor,
        colour: PersonColour::Swatch(Swatch::Violet),
        at: chat::Date::constant(2026, 1, 1).at(21, 5, 0, 0),
        lines: &[Phrase::PreviewReplyLine],
    },
];
