//! How a share starts on this device, kept for the next run. Each field is read alone: a word this build does not
//! know is that field's default, never the whole file lost.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::{Ceiling, Codec, FrameRate, Kbits, Resolution, ShareQuality, SoundSwitches};
use serde::{Deserialize, Serialize};

const FILE: &str = "share";

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    /// The short side it is scaled to; none for the screen's own size.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    short_side: Option<u32>,
    /// Pictures a second; none for the default.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    frame_rate: Option<u32>,
    /// The ceiling in kbit/s, a preset's or a custom one; none for Automatic.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    ceiling_kbits: Option<u32>,
    /// The word of the codec a person picked, H.264 included; none until one is picked, the device's own then.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    codec: Option<String>,
    /// Whether a whole screen's and a window's share carry sound; none for on, the default.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    screen_sound: Option<bool>,
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    window_sound: Option<bool>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

/// How the file names a codec.
fn word(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::Av1 => "av1",
    }
}

impl Held {
    fn quality(self) -> ShareQuality {
        let resolution = Resolution::ALL.into_iter().find(|resolution| resolution.short_side() == self.short_side);
        let frame_rate = FrameRate::ALL.into_iter().find(|rate| Some(rate.per_second()) == self.frame_rate);
        let bits = self.ceiling_kbits.map(|kbits| u64::from(kbits) * 1_000);
        let ceiling = Ceiling::ALL
            .into_iter()
            .find(|ceiling| ceiling.bits_per_second() == bits)
            .or_else(|| self.ceiling_kbits.and_then(Kbits::new).map(Ceiling::Custom));
        let codec = Codec::ALL.into_iter().find(|codec| self.codec.as_deref() == Some(word(*codec)));
        ShareQuality {
            resolution: resolution.unwrap_or_default(),
            frame_rate: frame_rate.unwrap_or_default(),
            ceiling: ceiling.unwrap_or_default(),
            codec,
            sound: SoundSwitches {
                screen: self.screen_sound.unwrap_or(true),
                window: self.window_sound.unwrap_or(true),
            },
        }
    }

    fn of(quality: &ShareQuality) -> Held {
        Held {
            short_side: quality.resolution.short_side(),
            frame_rate: (quality.frame_rate != FrameRate::default()).then(|| quality.frame_rate.per_second()),
            ceiling_kbits: quality.ceiling.bits_per_second().and_then(|bits| u32::try_from(bits / 1_000).ok()),
            codec: quality.codec.map(|codec| word(codec).to_string()),
            screen_sound: (!quality.sound.screen).then_some(false),
            window_sound: (!quality.sound.window).then_some(false),
        }
    }
}

/// How a share starts; the defaults where nothing was kept.
pub async fn quality(platform: &Platform) -> ShareQuality {
    read_kept::<Held>(platform).await.quality()
}

/// Keeps `quality` as how a share starts.
pub async fn keep(platform: &Platform, quality: ShareQuality) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(&quality)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{json_sorted, kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn held(folder: &std::path::Path) -> ShareQuality {
        read_kept_in::<Held>(folder).quality()
    }

    fn written(folder: &std::path::Path, text: &str) {
        std::fs::create_dir_all(folder.join("chatkord")).unwrap();
        raw_kept_in::<Held>(folder, text).unwrap();
    }

    #[test]
    fn nothing_kept_is_the_defaults() {
        assert_eq!(held(&scratch("none")), ShareQuality::default());
    }

    #[test]
    fn a_choice_reads_back_as_written_in_plain_words() {
        let folder = scratch("written");
        let quality = ShareQuality {
            resolution: Resolution::P1080,
            frame_rate: FrameRate::TwentyFour,
            ceiling: Ceiling::Mbit8,
            codec: Some(Codec::Av1),
            sound: SoundSwitches { screen: true, window: false },
        };
        write_kept_in(&folder, &Held::of(&quality)).unwrap();
        assert_eq!(held(&folder), quality);
        let text = kept_text_in::<Held>(&folder);
        assert_eq!(
            text,
            json_sorted(
                r#"{"short_side":1080,"frame_rate":24,"ceiling_kbits":8000,"codec":"av1","window_sound":false}"#
            )
        );
        write_kept_in(&folder, &Held::of(&ShareQuality::default())).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}", "defaults keep nothing");
    }

    #[test]
    fn an_encoder_kept_by_an_earlier_build_is_let_go_and_the_rest_kept() {
        let folder = scratch("encoder");
        written(&folder, r#"{"frame_rate":30,"codec":"av1","encoder":{"via":"SVT-AV1","on":"processor"}}"#);
        let quality = held(&folder);
        assert_eq!(
            quality,
            ShareQuality { frame_rate: FrameRate::Thirty, codec: Some(Codec::Av1), ..ShareQuality::default() }
        );
        write_kept_in(&folder, &Held::of(&quality)).unwrap();
        let text = kept_text_in::<Held>(&folder);
        assert_eq!(text, json_sorted(r#"{"frame_rate":30,"codec":"av1"}"#), "the next keep writes no encoder");
    }

    #[test]
    fn h264_picked_is_kept_as_picked_and_none_picked_is_written_as_nothing() {
        let folder = scratch("picked");
        let picked = ShareQuality { codec: Some(Codec::H264), ..ShareQuality::default() };
        write_kept_in(&folder, &Held::of(&picked)).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), r#"{"codec":"h264"}"#);
        assert_eq!(held(&folder), picked, "not taken for the device's own, which may be AV1");
        written(&folder, "{}");
        assert_eq!(held(&folder).codec, None);
    }

    #[test]
    fn a_word_this_build_does_not_know_is_that_fields_default_alone() {
        let folder = scratch("unknown");
        written(
            &folder,
            r#"{"short_side":900,"frame_rate":120,"ceiling_kbits":"lots","codec":"vp9","screen_sound":"no"}"#,
        );
        let read = held(&folder);
        assert_eq!(
            (read.resolution, read.frame_rate, read.ceiling),
            (Resolution::Source, FrameRate::Sixty, Ceiling::Automatic)
        );
        assert_eq!(read.codec, None, "a codec this build does not send is the device's own");
        assert_eq!(read.sound, SoundSwitches::default(), "a switch of another shape is on");
        written(&folder, "[1, 2]");
        assert_eq!(held(&folder), ShareQuality::default(), "a file of another shape is nothing kept");
        written(&folder, r#"{"ceiling_kbits":2250}"#);
        assert_eq!(held(&folder).ceiling, Ceiling::Automatic, "a rate between two steps is no ceiling");
    }

    #[test]
    fn a_custom_ceiling_reads_back_in_kbits() {
        let folder = scratch("custom");
        let kbits = Kbits::new(2_500).expect("a step in the bounds");
        let quality = ShareQuality { ceiling: Ceiling::Custom(kbits), ..ShareQuality::default() };
        write_kept_in(&folder, &Held::of(&quality)).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), r#"{"ceiling_kbits":2500}"#);
        assert_eq!(held(&folder), quality);
        let preset = ShareQuality { ceiling: Ceiling::Mbit16, ..ShareQuality::default() };
        write_kept_in(&folder, &Held::of(&preset)).unwrap();
        assert_eq!(held(&folder), preset, "a preset's rate reads back as the preset, not as a custom one");
    }
}
