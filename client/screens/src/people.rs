//! How a name and a state are said: the letters a name is shortened to in a small picture, and the
//! word for what someone in a call is doing.

use design::{PersonColour, Presence, Swatch};
use proto::rules::Colour;
use terms::MemberId;
use unicode_segmentation::UnicodeSegmentation;
use words::Phrase;

/// The most letters an avatar shows.
const MOST: usize = 2;

/// A name shortened to the first letter of its first words, in capitals: "Ada Lovelace" is "AL", "ada"
/// is "A", "Ελένη Παπά" is "ΕΠ". A letter is a whole grapheme (an accented letter, an emoji with its
/// modifiers), never part of one; a name with nothing to show gives nothing.
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.graphemes(true).next())
        .filter(|letter| letter.chars().any(|c| !c.is_ascii_punctuation()))
        .take(MOST)
        .map(str::to_uppercase)
        .collect()
}

/// Which of the six colours an account is, from its id: the same on every device, because the function is fixed (FNV-1a),
/// never the standard library's seeded hasher.
pub(crate) fn swatch_of(user_id: &MemberId) -> Swatch {
    swatch_of_text(user_id.as_str())
}

/// The swatch of an account whose id is `text`, for a name with no account known behind it yet.
pub(crate) fn swatch_of_text(text: &str) -> Swatch {
    let hash = text
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let at = usize::try_from(hash % Swatch::ALL.len() as u64).unwrap_or(0);
    Swatch::ALL[at]
}

/// A person's colour: the one they picked, else their id's swatch.
pub fn colour_of(user_id: &MemberId, picked: Option<Colour>) -> PersonColour {
    picked.map_or_else(
        || PersonColour::Swatch(swatch_of(user_id)),
        |picked| PersonColour::Picked(crate::colour::rgb(picked)),
    )
}

/// The colour the profile's picker starts from for one who picked none: their swatch, as the dark scheme fills it.
pub(crate) fn unpicked(user_id: &MemberId) -> Colour {
    let fill = swatch_of(user_id).fill(design::Scheme::Dark);
    Colour::new(fill.r, fill.g, fill.b)
}

/// The word on a badge for what someone is doing.
pub const fn presence_phrase(presence: Presence) -> Phrase {
    match presence {
        Presence::Online => Phrase::PresenceOnline,
        Presence::Speaking => Phrase::PresenceSpeaking,
        Presence::Muted => Phrase::PresenceMuted,
        Presence::Deafened => Phrase::PresenceDeafened,
        Presence::MutedByAdmin => Phrase::PresenceMutedByAdmin,
        Presence::Connecting => Phrase::PresenceConnecting,
        Presence::Failed => Phrase::PresenceFailed,
        Presence::Live => Phrase::PresenceLive,
        Presence::Reconnecting => Phrase::PresenceReconnecting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::member;

    /// Pinned: a colour that moved between releases would change who looks like whom on every device.
    #[test]
    fn an_account_has_the_same_colour_on_every_device_and_every_colour_is_used() {
        assert_eq!(swatch_of(&member("ada")), Swatch::Violet);
        assert_eq!(swatch_of(&member("grace")), Swatch::Indigo);
        assert_eq!(swatch_of(&member("bo")), Swatch::Teal);
        let used: std::collections::HashSet<_> = (0..60).map(|n| swatch_of(&member(&format!("account-{n}")))).collect();
        assert_eq!(used.len(), Swatch::ALL.len(), "sixty accounts reach all six colours");
    }

    #[test]
    fn initials_are_the_first_letters_of_the_first_words() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("ada"), "A");
        assert_eq!(initials("grace brewster murray hopper"), "GB", "at most two");
        assert_eq!(initials("Ελένη Παπά"), "ΕΠ");
        assert_eq!(initials("  spaced   out  "), "SO");
    }

    #[test]
    fn an_initial_is_a_whole_letter_never_half_of_one() {
        assert_eq!(initials("e\u{301}lan vital"), "E\u{301}V", "an accent stays with its letter");
        assert_eq!(initials("👨‍👩‍👧 family"), "👨‍👩‍👧F", "a joined emoji is one");
        assert_eq!(initials("ǆ"), "Ǆ", "capitalised by Unicode's rule, not ASCII's");
    }

    #[test]
    fn a_name_with_nothing_to_show_gives_nothing() {
        assert_eq!(initials(""), "");
        assert_eq!(initials("   "), "");
        assert_eq!(initials("---"), "");
    }

    #[test]
    fn every_presence_has_its_word() {
        for presence in Presence::ALL {
            assert!(!presence_phrase(presence).in_english().is_empty(), "{presence:?}");
        }
        let words: Vec<_> = Presence::ALL.iter().map(|p| presence_phrase(*p).in_english()).collect();
        for (at, word) in words.iter().enumerate() {
            assert!(!words[..at].contains(word), "{word} is on two badges");
        }
    }
}
