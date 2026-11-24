//! The emoji this device picked lately, latest first, as Discord's frequently used, and the skin tone the picker shows
//! hands in.

use chat::ReactionEmoji;
use chat::emoji::SkinTone;

/// How many emoji the picker's Recent keeps: three rows of its grid.
pub const RECENT_EMOJI: usize = 3 * design::EMOJI_GRID_COLUMNS;

/// What the picker remembers on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiPicks {
    /// Latest first, each once, at most [`RECENT_EMOJI`].
    recent: Vec<ReactionEmoji>,
    /// The tone hands are shown and picked in.
    pub tone: SkinTone,
}

impl Default for EmojiPicks {
    fn default() -> EmojiPicks {
        EmojiPicks { recent: Vec::new(), tone: SkinTone::Default }
    }
}

impl EmojiPicks {
    /// The picks `recent` names, latest first, in `tone`: a repeat after its first is passed over and past the most
    /// the rest go.
    pub fn new(recent: impl IntoIterator<Item = ReactionEmoji>, tone: SkinTone) -> EmojiPicks {
        let mut kept: Vec<ReactionEmoji> = Vec::with_capacity(RECENT_EMOJI);
        for emoji in recent {
            if kept.len() == RECENT_EMOJI {
                break;
            }
            if !kept.contains(&emoji) {
                kept.push(emoji);
            }
        }
        EmojiPicks { recent: kept, tone }
    }

    /// The emoji picked lately, latest first.
    pub fn recent(&self) -> &[ReactionEmoji] {
        &self.recent
    }

    /// What the debug report may say of the picks: the tone's place in the picker's row, and how many emoji are
    /// remembered, never which.
    pub(crate) fn facts(&self) -> (Option<usize>, usize) {
        let EmojiPicks { recent, tone } = self;
        (chat::emoji::TONES.iter().position(|offered| offered == tone), recent.len())
    }

    /// `emoji` picked now: first in Recent, once.
    pub fn picked(&mut self, emoji: ReactionEmoji) {
        self.recent.retain(|held| *held != emoji);
        self.recent.insert(0, emoji);
        self.recent.truncate(RECENT_EMOJI);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unicode(glyph: &str) -> ReactionEmoji {
        ReactionEmoji::Unicode(glyph.to_owned())
    }

    #[test]
    fn a_pick_comes_first_once_and_the_oldest_goes_past_the_most() {
        let mut picks = EmojiPicks::default();
        for at in 0..=RECENT_EMOJI {
            picks.picked(unicode(&at.to_string()));
        }
        assert_eq!(picks.recent().len(), RECENT_EMOJI);
        assert_eq!(picks.recent().last(), Some(&unicode("1")), "the first picked went");
        picks.picked(unicode("5"));
        assert_eq!(picks.recent().first(), Some(&unicode("5")));
        assert_eq!(picks.recent().iter().filter(|held| **held == unicode("5")).count(), 1);
    }
}
