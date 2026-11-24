//! Unicode's emoji as the `emojis` crate knows them: which stretches of a text are one emoji, and the lookups a picker
//! and the composer's `:` completion search.

use std::ops::Range;

pub use emojis::{Emoji, Group, SkinTone};

/// The tones a picker offers, one hand each: the default yellow, then light to dark.
pub const TONES: [SkinTone; 6] =
    [SkinTone::Default, SkinTone::Light, SkinTone::MediumLight, SkinTone::Medium, SkinTone::MediumDark, SkinTone::Dark];

/// The emoji `text` is whole, or none. One scalar must be the emoji's fully qualified form, so a character Unicode
/// shows as text by default (`©`, `☺` without U+FE0F) stays text.
pub fn known(text: &str) -> Option<&'static Emoji> {
    let emoji = emojis::get(text)?;
    let one_scalar = text.chars().nth(1).is_none();
    (!one_scalar || emoji.as_str() == text).then_some(emoji)
}

/// The emoji a GitHub shortcode names (`thinking` for 🤔), without its colons.
pub fn by_shortcode(code: &str) -> Option<&'static Emoji> {
    emojis::get_by_shortcode(code)
}

/// Every emoji `emojis` knows, each skin tone of one included, in Unicode's order.
pub fn all() -> impl Iterator<Item = &'static Emoji> {
    emojis::iter().flat_map(|emoji| std::iter::once(emoji).chain(emoji.skin_tones().into_iter().flatten().skip(1)))
}

/// A picker's groups, in Unicode's order.
pub fn groups() -> impl Iterator<Item = Group> {
    Group::iter()
}

/// A group's emoji in the default tone, in Unicode's order.
pub fn in_group(group: Group) -> impl Iterator<Item = &'static Emoji> {
    group.emojis()
}

/// `emoji` in `tone`, or itself when it takes no tone.
pub fn toned(emoji: &'static Emoji, tone: SkinTone) -> &'static Emoji {
    emoji.with_skin_tone(tone).unwrap_or(emoji)
}

/// The emoji whose shortcode starts with `query`, then those a word of whose name does, each once, in Unicode's
/// order within each; `query` is matched without regard to case.
pub fn search(query: &str) -> impl Iterator<Item = &'static Emoji> {
    let query = query.to_lowercase();
    let by_code = {
        let query = query.clone();
        move |emoji: &&'static Emoji| emoji.shortcodes().any(|code| folded_prefix(code, &query))
    };
    let by_name = move |emoji: &&'static Emoji| {
        emoji.name().split(|c: char| !c.is_alphanumeric()).any(|word| folded_prefix(word, &query))
    };
    let first = by_code.clone();
    emojis::iter().filter(first).chain(emojis::iter().filter(move |emoji| !by_code(emoji) && by_name(emoji)))
}

fn folded_prefix(word: &str, query: &str) -> bool {
    let mut folded = word.chars().flat_map(char::to_lowercase);
    query.chars().all(|wanted| folded.next() == Some(wanted))
}

/// `text`'s stretches in order, each whole emoji alone and flagged, the text between in one.
pub(crate) fn clusters(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut plain_from = 0;
    let mut at = 0;
    while let Some(first) = text.get(at..).and_then(|rest| rest.chars().next()) {
        match emoji_at(text, at, first) {
            Some(end) => {
                if plain_from < at {
                    out.push((plain_from..at, false));
                }
                out.push((at..end, true));
                at = end;
                plain_from = end;
            }
            None => at += first.len_utf8(),
        }
    }
    if plain_from < text.len() {
        out.push((plain_from..text.len(), false));
    }
    out
}

const VS16: char = '\u{FE0F}';
const KEYCAP: char = '\u{20E3}';
const ZWJ: char = '\u{200D}';

fn continues(c: char) -> bool {
    matches!(c, VS16 | KEYCAP | '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0020}'..='\u{E007F}')
}

fn regional(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

/// Where the emoji starting at byte `at` ends, if one does: the longest known prefix of the sequence an emoji there
/// could be (its modifiers, tags, keycap, ZWJ joins, a flag's second letter).
fn emoji_at(text: &str, at: usize, first: char) -> Option<usize> {
    let rest = text.get(at..)?;
    let mut chars = rest.char_indices().peekable();
    chars.next();
    if first.is_ascii() {
        let keycap =
            matches!(first, '0'..='9' | '#' | '*') && chars.peek().is_some_and(|&(_, c)| c == VS16 || c == KEYCAP);
        if !keycap {
            return None;
        }
    }
    let mut ends = vec![first.len_utf8()];
    let mut joined = false;
    while let Some(&(offset, c)) = chars.peek() {
        let flag_pair = regional(first) && ends.len() == 1 && regional(c);
        if !(joined || continues(c) || c == ZWJ || flag_pair) || c.is_whitespace() {
            break;
        }
        joined = c == ZWJ;
        chars.next();
        ends.push(offset + c.len_utf8());
    }
    ends.iter().rev().find(|&&end| rest.get(..end).and_then(known).is_some()).map(|end| at + end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<(&str, bool)> {
        clusters(text).into_iter().map(|(range, emoji)| (text.get(range).unwrap(), emoji)).collect()
    }

    #[test]
    fn every_emoji_emojis_knows_is_one_cluster_between_text() {
        for emoji in all() {
            let text = format!("a{}b", emoji.as_str());
            assert_eq!(split(&text), vec![("a", false), (emoji.as_str(), true), ("b", false)], "{}", emoji.name());
        }
    }

    #[test]
    fn text_presentation_characters_stay_text_and_sequences_split_apart() {
        assert_eq!(split("© 2026 ☺"), vec![("© 2026 ☺", false)]);
        assert_eq!(split("☺\u{FE0F}"), vec![("☺\u{FE0F}", true)]);
        assert_eq!(split("🇬🇷🇫🇷"), vec![("🇬🇷", true), ("🇫🇷", true)]);
        assert_eq!(split("👍🏽👍"), vec![("👍🏽", true), ("👍", true)]);
        assert_eq!(split("#1 #️⃣"), vec![("#1 ", false), ("#️⃣", true)]);
        assert_eq!(split("plain"), vec![("plain", false)]);
    }

    #[test]
    fn search_puts_shortcodes_first_and_matches_name_words_without_case() {
        let found: Vec<&str> = search("Thinking").map(Emoji::as_str).collect();
        assert_eq!(found.first(), Some(&"🤔"));
        assert!(search("grinning").any(|emoji| emoji.as_str() == "😀"));
        assert!(search("zzzzzz-nothing").next().is_none());
        let mut seen = std::collections::HashSet::new();
        assert!(search("face").all(|emoji| seen.insert(emoji.as_str())), "each emoji once");
    }

    #[test]
    fn a_tone_applies_where_an_emoji_takes_one() {
        let thumbs = known("👍").unwrap();
        assert_eq!(toned(thumbs, SkinTone::Dark).as_str(), "👍🏿");
        let cool = known("😎").unwrap();
        assert_eq!(toned(cool, SkinTone::Dark).as_str(), "😎");
        assert_eq!(by_shortcode("rocket").map(Emoji::as_str), Some("🚀"));
        assert!(groups().all(|group| in_group(group).next().is_some()));
    }
}
