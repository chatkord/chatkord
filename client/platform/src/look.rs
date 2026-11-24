//! How the window looks on this device, kept for the next run: the theme, the contrast and the zoom picked in
//! Settings › Appearance, the server pane's width as its splitter left it, the members column (closed by its toggle, its
//! width as its splitter left it), a call's chat column (the same), how a call is drawn (its Voice options), and a message's text size and the space
//! between runs. A file of its own; each field is a word read alone, so a word this build does not know is
//! that field's default, never the whole file lost.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::PaneWidth;
use choices::{CallChat, CallChatShown, CallChatWidth};
use choices::{CallView, Controls, Look, OwnShare, People};
use choices::{Members, MembersShown, MembersWidth, Previews};
use design::{Contrast, MessageScale, MessageSize, Preference, RunGap, Zoom};
use serde::{Deserialize, Serialize};

const FILE: &str = "look";

/// The file: each field absent for its default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    contrast: Option<String>,
    /// The percent, a step of the ladder.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    zoom: Option<String>,
    /// The pane's width in whole pixels, held to its range when read.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    pane: Option<String>,
    /// A call's people with no picture, `pictured` when only those with one show.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    people: Option<String>,
    /// The call bar, `while-hovered` when it shows only while it is wanted.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    controls: Option<String>,
    /// One's own share's card in a call, `hidden` when it has none.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    own_share: Option<String>,
    /// The members column, `closed` when its toggle put it away.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    members: Option<String>,
    /// The members column's width in whole pixels, held to its range when read.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    members_width: Option<String>,
    /// A message's text size in pixels, a step of its ladder.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    text_size: Option<String>,
    /// The space between a text channel's runs in pixels, a step of its ladder.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    run_gap: Option<String>,
    /// A call's chat column, `open` when its Chat button left it open.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    call_chat: Option<String>,
    /// A call's chat column's width in whole pixels, held to its range when read.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    call_chat_width: Option<String>,
    /// Link previews, `off` when the switch turned them off.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    previews: Option<String>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

const fn theme_word(theme: Preference) -> Option<&'static str> {
    match theme {
        Preference::System => None,
        Preference::Light => Some("light"),
        Preference::Dark => Some("dark"),
    }
}

fn zoom_word(zoom: Zoom) -> Option<String> {
    (zoom != Zoom::DEFAULT).then(|| zoom.percent().to_string())
}

fn text_size_word(size: MessageSize) -> Option<String> {
    (size != MessageSize::DEFAULT).then(|| size.px().to_string())
}

fn run_gap_word(gap: RunGap) -> Option<String> {
    (gap != RunGap::DEFAULT).then(|| gap.px().to_string())
}

fn pane_word(pane: PaneWidth) -> Option<String> {
    (pane != PaneWidth::default()).then(|| pane.whole().to_string())
}

const fn members_word(shown: MembersShown) -> Option<&'static str> {
    match shown {
        MembersShown::Open => None,
        MembersShown::Closed => Some("closed"),
    }
}

fn members_width_word(width: MembersWidth) -> Option<String> {
    (width != MembersWidth::default()).then(|| width.whole().to_string())
}

const fn call_chat_word(shown: CallChatShown) -> Option<&'static str> {
    match shown {
        CallChatShown::Closed => None,
        CallChatShown::Open => Some("open"),
    }
}

fn call_chat_width_word(width: CallChatWidth) -> Option<String> {
    (width != CallChatWidth::default()).then(|| width.whole().to_string())
}

const fn previews_word(previews: Previews) -> Option<&'static str> {
    match previews {
        Previews::On => None,
        Previews::Off => Some("off"),
    }
}

const fn people_word(people: People) -> Option<&'static str> {
    match people {
        People::All => None,
        People::Pictured => Some("pictured"),
    }
}

const fn controls_word(controls: Controls) -> Option<&'static str> {
    match controls {
        Controls::Always => None,
        Controls::WhileHovered => Some("while-hovered"),
    }
}

const fn own_share_word(own_share: OwnShare) -> Option<&'static str> {
    match own_share {
        OwnShare::Shown => None,
        OwnShare::Hidden => Some("hidden"),
    }
}

const fn contrast_word(contrast: Contrast) -> Option<&'static str> {
    match contrast {
        Contrast::Standard => None,
        Contrast::More => Some("more"),
    }
}

impl Held {
    fn look(&self) -> Look {
        let theme = [Preference::Light, Preference::Dark].into_iter().find(|t| theme_word(*t) == self.theme.as_deref());
        let contrast = Contrast::ALL.into_iter().find(|c| contrast_word(*c) == self.contrast.as_deref());
        let zoom = self.zoom.as_deref().and_then(|word| word.parse().ok()).and_then(Zoom::of_percent);
        let pane = self.pane.as_deref().and_then(|word| word.parse::<f32>().ok()).map(PaneWidth::new);
        let members_width =
            self.members_width.as_deref().and_then(|word| word.parse::<f32>().ok()).map(MembersWidth::new);
        let call_chat_width =
            self.call_chat_width.as_deref().and_then(|word| word.parse::<f32>().ok()).map(CallChatWidth::new);
        let text_size = self.text_size.as_deref().and_then(|word| word.parse().ok()).and_then(MessageSize::of_px);
        let run_gap = self.run_gap.as_deref().and_then(|word| word.parse().ok()).and_then(RunGap::of_px);
        Look {
            theme: theme.unwrap_or_default(),
            contrast: contrast.unwrap_or_default(),
            zoom: zoom.unwrap_or_default(),
            pane: pane.unwrap_or_default(),
            call: CallView {
                people: [People::Pictured]
                    .into_iter()
                    .find(|p| people_word(*p) == self.people.as_deref())
                    .unwrap_or_default(),
                controls: [Controls::WhileHovered]
                    .into_iter()
                    .find(|c| controls_word(*c) == self.controls.as_deref())
                    .unwrap_or_default(),
                own_share: [OwnShare::Hidden]
                    .into_iter()
                    .find(|o| own_share_word(*o) == self.own_share.as_deref())
                    .unwrap_or_default(),
            },
            members: Members {
                shown: [MembersShown::Closed]
                    .into_iter()
                    .find(|shown| members_word(*shown) == self.members.as_deref())
                    .unwrap_or_default(),
                width: members_width.unwrap_or_default(),
            },
            messages: MessageScale { size: text_size.unwrap_or_default(), gap: run_gap.unwrap_or_default() },
            call_chat: CallChat {
                shown: [CallChatShown::Open]
                    .into_iter()
                    .find(|shown| call_chat_word(*shown) == self.call_chat.as_deref())
                    .unwrap_or_default(),
                width: call_chat_width.unwrap_or_default(),
            },
            previews: [Previews::Off]
                .into_iter()
                .find(|previews| previews_word(*previews) == self.previews.as_deref())
                .unwrap_or_default(),
        }
    }

    fn of(look: Look) -> Held {
        let word = |word: Option<&str>| word.map(str::to_string);
        Held {
            theme: word(theme_word(look.theme)),
            contrast: word(contrast_word(look.contrast)),
            zoom: zoom_word(look.zoom),
            pane: pane_word(look.pane),
            people: word(people_word(look.call.people)),
            controls: word(controls_word(look.call.controls)),
            own_share: word(own_share_word(look.call.own_share)),
            members: word(members_word(look.members.shown)),
            members_width: members_width_word(look.members.width),
            text_size: text_size_word(look.messages.size),
            run_gap: run_gap_word(look.messages.gap),
            call_chat: word(call_chat_word(look.call_chat.shown)),
            call_chat_width: call_chat_width_word(look.call_chat.width),
            previews: word(previews_word(look.previews)),
        }
    }
}

/// How the window looks; the defaults where nothing was kept.
pub async fn look(platform: &Platform) -> Look {
    read_kept::<Held>(platform).await.look()
}

/// Keeps `look` for the next run.
pub async fn keep(platform: &Platform, look: Look) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(look)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-look-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn every_look_reads_back_as_written_and_the_default_writes_nothing() {
        let folder = scratch("round");
        for theme in [Preference::System, Preference::Light, Preference::Dark] {
            for contrast in Contrast::ALL {
                for zoom in Zoom::all() {
                    let pane = PaneWidth::new(320.0);
                    let look = Look { theme, contrast, zoom, pane, ..Look::default() };
                    write_kept_in(&folder, &Held::of(look)).unwrap();
                    assert_eq!(read_kept_in::<Held>(&folder).look(), look);
                }
            }
        }
        for people in [People::All, People::Pictured] {
            for controls in [Controls::Always, Controls::WhileHovered] {
                for own_share in [OwnShare::Shown, OwnShare::Hidden] {
                    let look = Look { call: CallView { people, controls, own_share }, ..Look::default() };
                    write_kept_in(&folder, &Held::of(look)).unwrap();
                    assert_eq!(read_kept_in::<Held>(&folder).look(), look);
                }
            }
        }
        for shown in [MembersShown::Open, MembersShown::Closed] {
            let look = Look { members: Members { shown, width: MembersWidth::new(300.0) }, ..Look::default() };
            write_kept_in(&folder, &Held::of(look)).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).look(), look);
        }
        for shown in [CallChatShown::Closed, CallChatShown::Open] {
            let look = Look { call_chat: CallChat { shown, width: CallChatWidth::new(480.0) }, ..Look::default() };
            write_kept_in(&folder, &Held::of(look)).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).look(), look);
        }
        for size in MessageSize::all() {
            for gap in RunGap::all() {
                let look = Look { messages: MessageScale { size, gap }, ..Look::default() };
                write_kept_in(&folder, &Held::of(look)).unwrap();
                assert_eq!(read_kept_in::<Held>(&folder).look(), look);
            }
        }
        for previews in [Previews::On, Previews::Off] {
            let look = Look { previews, ..Look::default() };
            write_kept_in(&folder, &Held::of(look)).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).look(), look);
        }
        write_kept_in(&folder, &Held::of(Look::default())).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}");
    }

    #[test]
    fn a_message_size_or_gap_off_its_ladder_is_its_default_and_leaves_the_other() {
        let folder = scratch("messages");
        raw_kept_in::<Held>(&folder, r#"{"text_size":"24","run_gap":"0"}"#).unwrap();
        let read = read_kept_in::<Held>(&folder).look().messages;
        assert_eq!((read.size.px(), read.gap.px()), (24, 0));
        for file in [r#"{"text_size":"13","run_gap":"0"}"#, r#"{"text_size":24,"run_gap":"0"}"#] {
            raw_kept_in::<Held>(&folder, file).unwrap();
            let read = read_kept_in::<Held>(&folder).look().messages;
            assert_eq!((read.size, read.gap.px()), (MessageSize::DEFAULT, 0), "{file}");
        }
    }

    #[test]
    fn a_members_width_out_of_its_range_is_held_to_it_and_an_unknown_word_leaves_the_column_open() {
        let folder = scratch("members");
        for (word, kept) in [("300", 300), ("9000", 360), ("12", 200), ("wide", 240)] {
            raw_kept_in::<Held>(&folder, format!(r#"{{"members":"closed","members_width":"{word}"}}"#)).unwrap();
            let read = read_kept_in::<Held>(&folder).look().members;
            assert_eq!((read.shown, read.width.whole()), (MembersShown::Closed, kept), "{word}");
        }
        raw_kept_in::<Held>(&folder, r#"{"members":"sideways"}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).look().members, Members::default());
    }

    #[test]
    fn a_word_this_build_does_not_know_is_that_fields_default_and_leaves_the_others() {
        let folder = scratch("odd");
        raw_kept_in::<Held>(&folder, r#"{"theme":"dark","grain":"plaid","contrast":7}"#).unwrap();
        let read = read_kept_in::<Held>(&folder).look();
        assert_eq!(read, Look { theme: Preference::Dark, ..Look::default() });
        raw_kept_in::<Held>(&folder, r#"{"theme":"dark","zoom":"999"}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).look(), read, "a zoom off the ladder is none");
        raw_kept_in::<Held>(&folder, r#"{"theme":"dark","zoom":150}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).look(), read, "and so is a number where a word goes");
        raw_kept_in::<Held>(&folder, "[1, 2]").unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).look(), Look::default());
    }

    #[test]
    fn a_pane_width_out_of_its_range_is_held_to_it_and_one_not_a_number_is_its_least() {
        let folder = scratch("pane");
        for (word, kept) in [("300", 300), ("9000", 400), ("12", 248), ("-5", 248), ("wide", 248), ("NaN", 248)] {
            let file = format!(r#"{{"theme":"dark","pane":"{word}"}}"#);
            raw_kept_in::<Held>(&folder, file).unwrap();
            let read = read_kept_in::<Held>(&folder).look();
            assert_eq!((read.theme, read.pane.whole()), (Preference::Dark, kept), "{word}");
        }
        raw_kept_in::<Held>(&folder, r#"{"zoom":"150","pane":320}"#).unwrap();
        let read = read_kept_in::<Held>(&folder).look();
        assert_eq!((read.zoom.percent(), read.pane), (150, PaneWidth::default()), "a number where a word goes");
    }
}
