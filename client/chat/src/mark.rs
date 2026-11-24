//! What a stretch of a body names beside its text: a member, a role, everyone, a channel, a custom emoji or a link,
//! by its range in the text's Unicode scalar values, as the wire counts them.

use std::ops::Range;

use terms::{ChannelId, MemberId, RoleId};

/// A custom emoji, which a server names by an id.
pub enum CustomEmoji {}

impl terms::Kind for CustomEmoji {
    const NAME: &'static str = "EmojiId";
}

/// A custom emoji's id at its server.
pub type EmojiId = terms::Id<CustomEmoji>;

/// What a mark names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MarkKind {
    /// A member, whose name is the mark's text.
    Mention(MemberId),
    /// Everyone who holds a role.
    Role(RoleId),
    /// Everyone who may read the channel.
    Everyone,
    /// Everyone who may read the channel and is online.
    Here,
    /// A channel, whose name is the mark's text.
    Channel(ChannelId),
    /// A server's own emoji, drawn as its picture; its text is `:name:`.
    CustomEmoji(EmojiId),
    /// A link whose target is the mark's own text.
    Link,
}

/// A stretch of a body that names something; markdown never splits it, and its markers are literal inside it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mark {
    /// Where it is, in Unicode scalar values of the text, end exclusive.
    pub range: Range<usize>,
    /// What it names.
    pub kind: MarkKind,
}

/// Whether a link mark of `text` opens when pressed: its text is a web link (`proto::rules::LinkTarget`). A link of any
/// other scheme a stored message keeps, such as a 1.0 invite's, is drawn as its text alone, with no press.
pub fn link_opens(text: &str) -> bool {
    proto::rules::LinkTarget::parse(text).is_some()
}

/// Why marks were refused for a text: each is a server or a caller not keeping the shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarksRefused {
    /// A mark covers nothing.
    Empty,
    /// A mark ends past the text.
    OutOfRange,
    /// A mark starts before the one before it ends: marks are sorted and never overlap.
    Overlapping,
}

/// The marks' ranges in bytes of `text`, or why they do not fit it.
pub(crate) fn byte_ranges(text: &str, marks: &[Mark]) -> Result<Vec<Range<usize>>, MarksRefused> {
    let mut offsets: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    offsets.push(text.len());
    let mut last_end = 0;
    marks
        .iter()
        .map(|mark| {
            let Range { start, end } = mark.range;
            if start >= end {
                return Err(MarksRefused::Empty);
            }
            if start < last_end {
                return Err(MarksRefused::Overlapping);
            }
            last_end = end;
            match (offsets.get(start), offsets.get(end)) {
                (Some(&from), Some(&to)) => Ok(from..to),
                _ => Err(MarksRefused::OutOfRange),
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a run's one range is the fixture, not a range of numbers")]
mod tests {
    use super::*;

    fn link(range: Range<usize>) -> Mark {
        Mark { range, kind: MarkKind::Link }
    }

    #[test]
    fn only_a_web_link_opens() {
        assert!(link_opens("https://example.invalid/a"));
        assert!(link_opens("HTTP://example.invalid"));
        assert!(!link_opens("chatkord://example.invalid/join/code"), "a stored 1.0 invite is its text alone");
        assert!(!link_opens("ftp://example.invalid"));
    }

    #[test]
    fn marks_are_in_range_sorted_and_apart_counted_in_scalars() {
        assert_eq!(byte_ranges("é@ada", &[link(1..5)]), Ok(vec![2..6]));
        assert_eq!(byte_ranges("abc", &[link(1..1)]), Err(MarksRefused::Empty));
        assert_eq!(byte_ranges("abc", &[link(1..4)]), Err(MarksRefused::OutOfRange));
        assert_eq!(byte_ranges("abcd", &[link(0..2), link(1..3)]), Err(MarksRefused::Overlapping));
        assert_eq!(byte_ranges("abcd", &[link(2..3), link(0..1)]), Err(MarksRefused::Overlapping));
        assert_eq!(byte_ranges("abcd", &[link(0..2), link(2..4)]), Ok(vec![0..2, 2..4]));
    }
}
