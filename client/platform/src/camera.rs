//! The camera this device uses and how one's own picture is shown, kept for the next run: one choice for the device,
//! whichever server it is used at. Each field is read alone: a word this build does not know is that field's default.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::{CameraRate, CameraResolution, CameraSettings, Mirror, Picked};
use serde::{Deserialize, Serialize};

const FILE: &str = "camera";

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    /// The camera picked, by its id and the name it was picked by; none for the first the system lists.
    #[serde(default, with = "crate::voice::device", skip_serializing_if = "Option::is_none")]
    device: Option<Picked>,
    /// The picture's height asked for; none for the default.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    resolution: Option<u32>,
    /// Pictures a second; none for the default.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    frame_rate: Option<u32>,
    /// `false` when one's own picture is shown as others see it; none for mirrored, the default.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    mirror: Option<bool>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

impl Held {
    fn settings(self) -> CameraSettings {
        let resolution = CameraResolution::ALL.into_iter().find(|kept| Some(kept.size().1) == self.resolution);
        let rate = CameraRate::ALL.into_iter().find(|kept| Some(kept.per_second()) == self.frame_rate);
        CameraSettings {
            device: self.device.filter(|picked| !picked.id.is_empty()),
            resolution: resolution.unwrap_or_default(),
            rate: rate.unwrap_or_default(),
            mirror: if self.mirror == Some(false) { Mirror::Off } else { Mirror::On },
        }
    }

    fn of(settings: &CameraSettings) -> Held {
        Held {
            device: settings.device.clone(),
            resolution: (settings.resolution != CameraResolution::default()).then(|| settings.resolution.size().1),
            frame_rate: (settings.rate != CameraRate::default()).then(|| settings.rate.per_second()),
            mirror: (settings.mirror == Mirror::Off).then_some(false),
        }
    }
}

/// The camera kept; the defaults where nothing was.
pub async fn settings(platform: &Platform) -> CameraSettings {
    read_kept::<Held>(platform).await.settings()
}

/// Keeps `settings` for the next run.
pub async fn keep(platform: &Platform, settings: CameraSettings) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(&settings)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{json_sorted, kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-camera-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn held(folder: &std::path::Path) -> CameraSettings {
        read_kept_in::<Held>(folder).settings()
    }

    fn written(folder: &std::path::Path, text: &str) {
        raw_kept_in::<Held>(folder, text).unwrap();
    }

    #[test]
    fn nothing_kept_is_the_defaults() {
        assert_eq!(held(&scratch("none")), CameraSettings::default());
        assert_eq!(CameraSettings::default().ask(), choices::CameraAsk::default(), "the first camera at 720p30");
    }

    #[test]
    fn a_choice_reads_back_as_written_in_plain_words() {
        let folder = scratch("written");
        let settings = CameraSettings {
            device: Some(Picked { id: "usb-camera".into(), label: "USB camera".into() }),
            resolution: CameraResolution::P1080,
            rate: CameraRate::Fifteen,
            mirror: Mirror::Off,
        };
        write_kept_in(&folder, &Held::of(&settings)).unwrap();
        assert_eq!(held(&folder), settings);
        let text = kept_text_in::<Held>(&folder);
        assert_eq!(
            text,
            json_sorted(
                r#"{"device":{"id":"usb-camera","label":"USB camera"},"resolution":1080,"frame_rate":15,"mirror":false}"#
            )
        );
        write_kept_in(&folder, &Held::of(&CameraSettings::default())).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}", "defaults keep nothing");
    }

    #[test]
    fn a_file_holding_a_key_this_build_does_not_know_loads_and_is_written_without_it() {
        let folder = scratch("old-key");
        written(&folder, r#"{"resolution":1080,"mirror":false,"start_paused":true}"#);
        let settings = held(&folder);
        assert_eq!(
            settings,
            CameraSettings { resolution: CameraResolution::P1080, mirror: Mirror::Off, ..CameraSettings::default() }
        );
        write_kept_in(&folder, &Held::of(&settings)).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), json_sorted(r#"{"resolution":1080,"mirror":false}"#));
    }

    #[test]
    fn a_word_this_build_does_not_know_is_that_field_s_default_alone() {
        let folder = scratch("unknown");
        written(&folder, r#"{"device":"USB camera","resolution":2160,"frame_rate":"fast","mirror":"no"}"#);
        assert_eq!(held(&folder), CameraSettings::default());
        written(&folder, r#"{"device":{"id":"","label":"Old"},"resolution":480}"#);
        assert_eq!(held(&folder), CameraSettings { resolution: CameraResolution::P480, ..CameraSettings::default() });
        written(&folder, "[1, 2]");
        assert_eq!(held(&folder), CameraSettings::default(), "a file of another shape is nothing kept");
    }
}
