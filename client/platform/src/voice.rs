//! What this device uses for voice, kept for the next run: the microphone and the speaker and how the microphone is
//! processed, how the account bar left mute and deafen, which the next call starts with, and whether the window's own
//! sounds and a message's sound play. One choice for the
//! device, whichever server it is used at. A device is kept by its id with the name it was picked by, and one that is
//! not here is said by that name and the default used.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::{
    AutomaticGain, EchoCancellation, EventSounds, InputGain, MessageSounds, NoiseGate, Picked, Popups, Suppression,
    Threshold, VoiceProcessing,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const FILE: &str = "voice";

/// The choices kept, the defaults when nothing was.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Choice {
    /// The microphone, or the default one; a value of another shape is none.
    #[serde(default, with = "device", skip_serializing_if = "Option::is_none")]
    pub microphone: Option<Picked>,
    /// The speaker, as the microphone is.
    #[serde(default, with = "device", skip_serializing_if = "Option::is_none")]
    pub speaker: Option<Picked>,
    /// The account bar's mute, which the next call starts with.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// The account bar's deafen, which the next call starts with.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deafened: bool,
    /// Whether the window's own sounds play: the word `off`, absent for on; any other value is on.
    #[serde(default, with = "sounds_word", skip_serializing_if = "plays")]
    pub event_sounds: EventSounds,
    /// Whether a message that would alert plays a sound: the word `off`, absent for on; any other value is on.
    #[serde(default, with = "message_sounds_word", skip_serializing_if = "sounding")]
    pub message_sounds: MessageSounds,
    /// Whether a message that would alert shows a desktop popup: the word `off`, absent for on; any other value is on.
    #[serde(default, with = "popups_word", skip_serializing_if = "popping")]
    pub popups: Popups,
    /// How the microphone is processed: only what differs from a new device's is written, and a value of another
    /// shape or out of its range is the default for that one setting.
    #[serde(default, with = "processing_words", skip_serializing_if = "fresh")]
    pub processing: VoiceProcessing,
}

impl crate::kept::Section for Choice {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

fn fresh(processing: &VoiceProcessing) -> bool {
    *processing == VoiceProcessing::default()
}

/// How the file keeps the microphone's processing: `{"echo": "off", "suppression": "standard", "automatic_gain":
/// "off", "volume": 6, "gate": "on", "threshold": -40}`, each entry only when it is not a new device's.
mod processing_words {
    use super::*;
    use serde_json::{Map, Value};

    fn word(on: bool) -> &'static str {
        if on { "on" } else { "off" }
    }

    pub(super) fn serialize<S: Serializer>(kept: &VoiceProcessing, into: S) -> Result<S::Ok, S::Error> {
        let fresh = VoiceProcessing::default();
        let mut entries = Map::new();
        if kept.echo != fresh.echo {
            entries.insert("echo".into(), word(kept.echo == EchoCancellation::On).into());
        }
        if kept.suppression != fresh.suppression {
            let named = match kept.suppression {
                Suppression::Off => "off",
                Suppression::Standard => "standard",
                Suppression::Rnnoise => "rnnoise",
            };
            entries.insert("suppression".into(), named.into());
        }
        if kept.automatic_gain != fresh.automatic_gain {
            entries.insert("automatic_gain".into(), word(kept.automatic_gain == AutomaticGain::On).into());
        }
        if kept.volume != fresh.volume {
            entries.insert("volume".into(), kept.volume.db().into());
        }
        if kept.gate != fresh.gate {
            entries.insert("gate".into(), word(kept.gate == NoiseGate::On).into());
        }
        if kept.threshold != fresh.threshold {
            entries.insert("threshold".into(), kept.threshold.db().into());
        }
        entries.serialize(into)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<VoiceProcessing, D::Error> {
        let value = Value::deserialize(from)?;
        let fresh = VoiceProcessing::default();
        let entry = |name: &str| value.get(name);
        let switched = |name: &str| entry(name).and_then(Value::as_str);
        let decibels = |name: &str| entry(name).and_then(Value::as_i64).and_then(|db| i8::try_from(db).ok());
        Ok(VoiceProcessing {
            echo: match switched("echo") {
                Some("off") => EchoCancellation::Off,
                Some("on") => EchoCancellation::On,
                _ => fresh.echo,
            },
            suppression: match switched("suppression") {
                Some("off") => Suppression::Off,
                Some("standard") => Suppression::Standard,
                Some("rnnoise") => Suppression::Rnnoise,
                _ => fresh.suppression,
            },
            automatic_gain: match switched("automatic_gain") {
                Some("off") => AutomaticGain::Off,
                Some("on") => AutomaticGain::On,
                _ => fresh.automatic_gain,
            },
            volume: decibels("volume").and_then(InputGain::new).unwrap_or(fresh.volume),
            gate: match switched("gate") {
                Some("on") => NoiseGate::On,
                Some("off") => NoiseGate::Off,
                _ => fresh.gate,
            },
            threshold: decibels("threshold").and_then(Threshold::new).unwrap_or(fresh.threshold),
        })
    }
}

fn plays(sounds: &EventSounds) -> bool {
    *sounds == EventSounds::On
}

fn sounding(sounds: &MessageSounds) -> bool {
    *sounds == MessageSounds::On
}

fn popping(popups: &Popups) -> bool {
    *popups == Popups::On
}

mod message_sounds_word {
    use super::*;

    const OFF: &str = "off";

    pub(super) fn serialize<S: Serializer>(sounds: &MessageSounds, into: S) -> Result<S::Ok, S::Error> {
        into.serialize_str(match sounds {
            MessageSounds::On => "on",
            MessageSounds::Off => OFF,
        })
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<MessageSounds, D::Error> {
        let word = serde_json::Value::deserialize(from)?;
        Ok(if word.as_str() == Some(OFF) { MessageSounds::Off } else { MessageSounds::On })
    }
}

mod popups_word {
    use super::*;

    const OFF: &str = "off";

    pub(super) fn serialize<S: Serializer>(popups: &Popups, into: S) -> Result<S::Ok, S::Error> {
        into.serialize_str(match popups {
            Popups::On => "on",
            Popups::Off => OFF,
        })
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Popups, D::Error> {
        let word = serde_json::Value::deserialize(from)?;
        Ok(if word.as_str() == Some(OFF) { Popups::Off } else { Popups::On })
    }
}

mod sounds_word {
    use super::*;

    const OFF: &str = "off";

    pub(super) fn serialize<S: Serializer>(sounds: &EventSounds, into: S) -> Result<S::Ok, S::Error> {
        into.serialize_str(match sounds {
            EventSounds::On => "on",
            EventSounds::Off => OFF,
        })
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<EventSounds, D::Error> {
        let word = serde_json::Value::deserialize(from)?;
        Ok(if word.as_str() == Some(OFF) { EventSounds::Off } else { EventSounds::On })
    }
}

/// A device as the file keeps it: `{"id": …, "label": …}`; the `camera` section keeps a camera so too.
pub(crate) mod device {
    use super::*;

    #[derive(Deserialize, Serialize)]
    struct Kept {
        id: String,
        label: String,
    }

    pub(crate) fn serialize<S: Serializer>(picked: &Option<Picked>, into: S) -> Result<S::Ok, S::Error> {
        picked.as_ref().map(|picked| Kept { id: picked.id.clone(), label: picked.label.clone() }).serialize(into)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Option<Picked>, D::Error> {
        let kept = serde_json::from_value::<Kept>(serde_json::Value::deserialize(from)?).ok();
        Ok(kept.map(|kept| Picked { id: kept.id, label: kept.label }))
    }
}

impl Choice {
    /// An empty id is the default device.
    fn tidied(mut self) -> Choice {
        self.microphone = self.microphone.filter(|picked| !picked.id.is_empty());
        self.speaker = self.speaker.filter(|picked| !picked.id.is_empty());
        self
    }
}

/// The choice made; the default when none was, or nothing was named to keep one in.
pub async fn choice(platform: &Platform) -> Choice {
    read_kept::<Choice>(platform).await.tidied()
}

/// Uses the microphone `picked`, or the default one for `None`.
pub async fn use_microphone(platform: &Platform, picked: Option<Picked>) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.microphone = picked).await
}

/// Uses the speaker `picked`, or the default one for `None`.
pub async fn use_speaker(platform: &Platform, picked: Option<Picked>) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.speaker = picked).await
}

/// Keeps whether the window's own sounds play.
pub async fn keep_event_sounds(platform: &Platform, sounds: EventSounds) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.event_sounds = sounds).await
}

/// Keeps whether a message that would alert plays a sound.
pub async fn keep_message_sounds(platform: &Platform, sounds: MessageSounds) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.message_sounds = sounds).await
}

/// Keeps whether a message that would alert shows a desktop popup.
pub async fn keep_popups(platform: &Platform, popups: Popups) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.popups = popups).await
}

/// Keeps how the microphone is processed, for the next run.
pub async fn keep_processing(platform: &Platform, processing: VoiceProcessing) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| held.processing = processing).await
}

/// Keeps how the account bar left mute and deafen, for the next call and the next run.
pub async fn keep_next_call(platform: &Platform, muted: bool, deafened: bool) -> Result<(), Unavailable> {
    change_kept::<Choice, _>(platform, move |held| {
        held.muted = muted;
        held.deafened = deafened;
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{json_sorted, kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-voice-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn held(folder: &std::path::Path) -> Choice {
        read_kept_in::<Choice>(folder).tidied()
    }

    #[test]
    fn nothing_chosen_is_the_default_devices_and_a_call_started_unmuted() {
        let folder = scratch("none");
        assert_eq!(held(&folder), Choice::default());
    }

    #[test]
    fn a_choice_reads_back_as_written_and_holds_nothing_else() {
        let folder = scratch("written");
        let choice = Choice {
            microphone: Some(Picked { id: "usb-desk".into(), label: "Desk microphone".into() }),
            speaker: None,
            muted: true,
            deafened: true,
            event_sounds: EventSounds::Off,
            processing: VoiceProcessing { gate: NoiseGate::On, ..VoiceProcessing::default() },
            ..Choice::default()
        };
        write_kept_in(&folder, &choice).unwrap();
        assert_eq!(held(&folder), choice);
        let text = kept_text_in::<Choice>(&folder);
        assert_eq!(
            text,
            json_sorted(
                r#"{"microphone":{"id":"usb-desk","label":"Desk microphone"},"muted":true,"deafened":true,"event_sounds":"off","processing":{"gate":"on"}}"#
            )
        );
    }

    #[test]
    fn a_device_kept_by_name_alone_is_forgotten_and_the_rest_is_kept() {
        let folder = scratch("by-name");
        write_kept_in(&folder, &Choice::default()).unwrap();
        raw_kept_in::<Choice>(&folder, r#"{"microphone":"Desk","muted":true}"#).unwrap();
        assert_eq!(held(&folder), Choice { muted: true, ..Choice::default() });
    }

    #[test]
    fn the_window_s_sounds_are_on_unless_the_file_says_off() {
        let folder = scratch("sounds");
        write_kept_in(&folder, &Choice::default()).unwrap();
        let text = kept_text_in::<Choice>(&folder);
        assert_eq!(text, "{}", "on is written as nothing");
        for (word, sounds) in [(r#""off""#, EventSounds::Off), (r#""loud""#, EventSounds::On), ("7", EventSounds::On)] {
            raw_kept_in::<Choice>(&folder, format!(r#"{{"muted":true,"event_sounds":{word}}}"#)).unwrap();
            assert_eq!(held(&folder), Choice { muted: true, event_sounds: sounds, ..Choice::default() }, "{word}");
        }
    }

    #[test]
    fn a_message_s_sound_is_on_unless_the_file_says_off() {
        let folder = scratch("message-sounds");
        write_kept_in(&folder, &Choice::default()).unwrap();
        assert_eq!(kept_text_in::<Choice>(&folder), "{}", "on is written as nothing");
        for (word, sounds) in
            [(r#""off""#, MessageSounds::Off), (r#""loud""#, MessageSounds::On), ("7", MessageSounds::On)]
        {
            raw_kept_in::<Choice>(&folder, format!(r#"{{"message_sounds":{word}}}"#)).unwrap();
            assert_eq!(held(&folder).message_sounds, sounds, "{word}");
        }
        raw_kept_in::<Choice>(&folder, r#"{"muted":true}"#).unwrap();
        assert_eq!(held(&folder).message_sounds, MessageSounds::On, "a file from before the switch reads on");
    }

    #[test]
    fn popups_are_on_unless_the_file_says_off() {
        let folder = scratch("alerts");
        write_kept_in(&folder, &Choice::default()).unwrap();
        assert_eq!(kept_text_in::<Choice>(&folder), "{}", "on and off are written as nothing");
        for (word, popups) in [(r#""off""#, Popups::Off), (r#""loud""#, Popups::On), ("7", Popups::On)] {
            raw_kept_in::<Choice>(&folder, format!(r#"{{"popups":{word}}}"#)).unwrap();
            assert_eq!(held(&folder).popups, popups, "{word}");
        }
    }

    #[test]
    fn a_do_not_disturb_an_earlier_build_kept_here_is_read_as_gone_and_the_next_keep_drops_it() {
        let folder = scratch("quiet-gone");
        write_kept_in(&folder, &Choice::default()).unwrap();
        raw_kept_in::<Choice>(&folder, r#"{"muted":true,"do_not_disturb":{"for":"one_hour","until":1763038800}}"#)
            .unwrap();
        let read = held(&folder);
        assert_eq!(read, Choice { muted: true, ..Choice::default() }, "the rest is kept");
        write_kept_in(&folder, &read).unwrap();
        assert_eq!(kept_text_in::<Choice>(&folder), r#"{"muted":true}"#, "the device keeps no do not disturb");
    }

    #[test]
    fn a_file_of_another_shape_or_an_empty_id_is_the_default() {
        let folder = scratch("odd");
        raw_kept_in::<Choice>(&folder, "[1, 2]").unwrap();
        assert_eq!(held(&folder), Choice::default());
        let empty = Picked { id: String::new(), label: "Desk".into() };
        write_kept_in(&folder, &Choice { microphone: Some(empty), ..Choice::default() }).unwrap();
        assert_eq!(held(&folder), Choice::default(), "an empty id is no device");
    }

    #[test]
    fn choosing_one_thing_keeps_the_others() {
        let folder = scratch("keeps");
        let headphones = Picked { id: "usb-headphones".into(), label: "Headphones".into() };
        let mut choice = Choice { speaker: Some(headphones), ..Choice::default() };
        write_kept_in(&folder, &choice).unwrap();
        let mut again = read_kept_in::<Choice>(&folder);
        again.deafened = true;
        write_kept_in(&folder, &again).unwrap();
        choice.deafened = true;
        assert_eq!(held(&folder), choice);
    }

    #[test]
    fn processing_keeps_what_differs_and_a_setting_of_another_shape_or_range_is_its_default() {
        let folder = scratch("processing");
        let kept = VoiceProcessing {
            echo: EchoCancellation::Off,
            suppression: Suppression::Standard,
            automatic_gain: AutomaticGain::Off,
            volume: InputGain::new(-6).unwrap(),
            gate: NoiseGate::On,
            threshold: Threshold::new(-40).unwrap(),
        };
        write_kept_in(&folder, &Choice { processing: kept, ..Choice::default() }).unwrap();
        assert_eq!(held(&folder).processing, kept);
        raw_kept_in::<Choice>(
            &folder,
            r#"{"processing":{"echo":"off","suppression":"loud","volume":99,"threshold":-30}}"#,
        )
        .unwrap();
        let read = held(&folder).processing;
        let fresh = VoiceProcessing::default();
        assert_eq!(read.echo, EchoCancellation::Off, "a setting that reads is kept");
        assert_eq!((read.suppression, read.volume), (fresh.suppression, fresh.volume), "the rest is a new device's");
        assert_eq!(read.threshold.db(), -30);
        raw_kept_in::<Choice>(&folder, r#"{"processing":"loud","muted":true}"#).unwrap();
        assert_eq!(held(&folder), Choice { muted: true, ..Choice::default() }, "a whole of another shape is fresh");
    }
}
