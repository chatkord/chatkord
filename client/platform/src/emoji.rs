//! The emoji picker's memory on this device, kept for the next run: the emoji picked lately, latest first, and the skin
//! tone hands are picked in. A file of its own; an entry this build cannot read is passed over alone.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::EmojiPicks;
use screens::chat::emoji::{self, SkinTone};
use screens::chat::{EmojiId, ReactionEmoji};
use serde::{Deserialize, Serialize};

const FILE: &str = "emoji";

/// The file: each field absent for its default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    /// Latest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recent: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    tone: Option<String>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

/// One emoji as the file writes it: a Unicode one as itself, a server's own by its id.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Written {
    Unicode(String),
    Custom(String),
}

impl Written {
    fn of(emoji: &ReactionEmoji) -> Written {
        match emoji {
            ReactionEmoji::Unicode(glyph) => Written::Unicode(glyph.clone()),
            ReactionEmoji::Custom(id) => Written::Custom(id.as_str().to_owned()),
        }
    }

    /// The emoji it names: a Unicode one `emojis` knows, a well-formed id; none else.
    fn read(self) -> Option<ReactionEmoji> {
        match self {
            Written::Unicode(glyph) => {
                emoji::known(&glyph).map(|known| ReactionEmoji::Unicode(known.as_str().to_owned()))
            }
            Written::Custom(id) => EmojiId::parse(&id).map(ReactionEmoji::Custom),
        }
    }
}

const fn tone_word(tone: SkinTone) -> Option<&'static str> {
    match tone {
        SkinTone::Light => Some("light"),
        SkinTone::MediumLight => Some("medium-light"),
        SkinTone::Medium => Some("medium"),
        SkinTone::MediumDark => Some("medium-dark"),
        SkinTone::Dark => Some("dark"),
        // `emojis` adds two-person tones the picker never offers.
        _ => None,
    }
}

impl Held {
    fn picks(self) -> EmojiPicks {
        let tone = emoji::TONES
            .into_iter()
            .find(|tone| tone_word(*tone).is_some() && tone_word(*tone) == self.tone.as_deref())
            .unwrap_or(SkinTone::Default);
        let recent = self
            .recent
            .into_iter()
            .filter_map(|written| serde_json::from_value::<Written>(written).ok())
            .filter_map(Written::read);
        EmojiPicks::new(recent, tone)
    }

    fn of(picks: &EmojiPicks) -> Held {
        Held {
            recent: picks.recent().iter().filter_map(|emoji| serde_json::to_value(Written::of(emoji)).ok()).collect(),
            tone: tone_word(picks.tone).map(str::to_owned),
        }
    }
}

/// What the picker remembers; none picked and the default tone where nothing was kept.
pub async fn picks(platform: &Platform) -> EmojiPicks {
    read_kept::<Held>(platform).await.picks()
}

/// Keeps `picks` for the next run.
pub async fn keep(platform: &Platform, picks: EmojiPicks) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(&picks)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-emoji-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn picks_read_back_as_written_and_the_default_writes_nothing() {
        let folder = scratch("round");
        let owl = EmojiId::parse("owlwave").unwrap();
        let recent = [ReactionEmoji::Unicode("🎲".to_owned()), ReactionEmoji::Custom(owl)];
        for tone in emoji::TONES {
            let picks = EmojiPicks::new(recent.clone(), tone);
            write_kept_in(&folder, &Held::of(&picks)).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).picks(), picks);
        }
        write_kept_in(&folder, &Held::of(&EmojiPicks::default())).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_entry_this_build_cannot_read_is_passed_over_and_leaves_the_rest() {
        let folder = scratch("odd");
        raw_kept_in::<Held>(&folder, r#"{"recent":[{"unicode":"no"},7,{"custom":""},{"unicode":"🎲"}],"tone":"blue"}"#)
            .unwrap();
        let picks = read_kept_in::<Held>(&folder).picks();
        assert_eq!(picks, EmojiPicks::new([ReactionEmoji::Unicode("🎲".to_owned())], SkinTone::Default));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
