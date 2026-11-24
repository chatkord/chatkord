//! The microphones and speakers of this device, as a call or a settings page lists them.

use choices::{CameraSettings, EventSounds, Look, MessageSounds, Picked, ShareQuality, Startup, VoiceProcessing};
use words::{Language, Phrase, Words};

/// A microphone or a speaker in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// What to ask for it by: its id, which two devices of one name never share, or none for the system's default.
    pub asked: Option<String>,
    /// What a person reads: for the default, the device it follows now.
    pub label: String,
}

impl Listed {
    /// What picking it asks for: none for the system's default.
    pub fn picked(&self) -> Option<Picked> {
        self.asked.clone().map(|id| Picked { id, label: self.label.clone() })
    }
}

/// A test's device, known by one word for both.
#[cfg(test)]
pub(crate) fn picked(name: &str) -> Picked {
    Picked { id: name.into(), label: name.into() }
}

/// What this device offers, and what a call is using of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sound {
    /// The microphones this device offers, the system's default among them.
    pub microphones: Vec<Listed>,
    /// The speakers this device offers, the system's default among them.
    pub speakers: Vec<Listed>,
    /// The microphone in use, as [`Listed::asked`] says it: none for the system's default.
    pub microphone: Option<String>,
    /// The speaker in use, as [`Listed::asked`] says it: none for the system's default.
    pub speaker: Option<String>,
}

impl Sound {
    /// The microphones as a list shows them. `saved` is what this device remembers: a saved microphone that is not
    /// here is listed last, not connected, where a page wants to show it (a menu passes none).
    pub fn microphone_options(&self, saved: Option<&Picked>, words: &Words) -> Vec<DeviceOption> {
        options(&self.microphones, self.microphone.as_deref(), saved, words)
    }

    /// The speakers as a list shows them, `saved` as for the microphones.
    pub fn speaker_options(&self, saved: Option<&Picked>, words: &Words) -> Vec<DeviceOption> {
        options(&self.speakers, self.speaker.as_deref(), saved, words)
    }
}

/// A device in one of the lists a person picks from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceOption {
    /// What a person reads: "Default" for the system's, else the device's name.
    pub name: String,
    /// Beside it: the device the default follows now, or that a saved device is not connected.
    pub note: Option<String>,
    /// What picking it asks for: none for the system's default.
    pub asked: Option<Picked>,
    /// In use, offered, or saved and away.
    pub state: DeviceState,
}

/// Where a listed device stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceState {
    /// Here, not in use: to be picked.
    Offered,
    /// Here and in use: ticked.
    InUse,
    /// Saved and not here: shown so a person sees what was chosen, and not to be picked.
    Away,
}

impl DeviceOption {
    /// Whether it is the device in use.
    pub fn ticked(&self) -> bool {
        self.state == DeviceState::InUse
    }

    /// Whether it is a saved device that is not here.
    pub fn away(&self) -> bool {
        self.state == DeviceState::Away
    }
}

fn in_use(yes: bool) -> DeviceState {
    if yes { DeviceState::InUse } else { DeviceState::Offered }
}

/// The list as a person sees it: the system's default first with the device it follows (when the module says which),
/// then each device, the one in use ticked; a saved device that is not listed comes last, not connected.
pub(crate) fn options(
    listed: &[Listed],
    using: Option<&str>,
    saved: Option<&Picked>,
    words: &Words,
) -> Vec<DeviceOption> {
    let mut rows: Vec<DeviceOption> = listed
        .iter()
        .map(|device| match &device.asked {
            None => DeviceOption {
                name: words.phrase(Phrase::SystemDefault).into_owned(),
                note: Some(device.label.clone()).filter(|label| !label.is_empty()),
                asked: None,
                state: in_use(using.is_none()),
            },
            Some(id) => DeviceOption {
                name: device.label.clone(),
                note: None,
                asked: device.picked(),
                state: in_use(using == Some(id.as_str())),
            },
        })
        .collect();
    if let Some(saved) = saved.filter(|saved| !listed.iter().any(|device| device.asked.as_ref() == Some(&saved.id))) {
        rows.push(DeviceOption {
            name: saved.label.clone(),
            note: Some(words.phrase(Phrase::NotConnected).into_owned()),
            asked: Some(saved.clone()),
            state: DeviceState::Away,
        });
    }
    rows
}

/// A choice for the host to keep for the next run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The microphone picked, none for the system's default.
    Microphone(Option<Picked>),
    /// The speaker picked, none for the system's default.
    Speaker(Option<Picked>),
    /// How a share starts, as last picked.
    Share(ShareQuality),
    /// How the window looks, as last picked.
    Look(Look),
    /// Whether the window's own sounds play, as last switched.
    EventSounds(EventSounds),
    /// Whether a message that would alert plays a sound, as last switched.
    MessageSounds(MessageSounds),
    /// Whether a message that would alert shows a desktop popup, as last switched.
    Popups(choices::Popups),
    /// How the microphone is processed, as last set.
    Processing(VoiceProcessing),
    /// How Chatkord starts and closes, as last switched.
    Startup(Startup),
    /// The camera and how it is shown, as last set.
    Camera(CameraSettings),
    /// The language the window is said in, as last picked; none to match the system's.
    Language(Option<Language>),
    /// Whether Chatkord downloads its own updates, as last switched.
    AutoUpdate(choices::AutoUpdate),
    /// The emoji picked lately and the skin tone hands are picked in, as last changed.
    Emoji(choices::EmojiPicks),
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADSET: &str = "Headset";
    const DESK: &str = "Desk";

    fn device(label: &str, asked: Option<&str>) -> Listed {
        Listed { asked: asked.map(str::to_string), label: label.to_string() }
    }

    fn sound(using: Option<&str>) -> Sound {
        let here = || vec![device(DESK, None), device(HEADSET, Some(HEADSET)), device(DESK, Some(DESK))];
        Sound { microphones: here(), speakers: here(), microphone: using.map(str::to_string), speaker: None }
    }

    #[test]
    fn the_default_comes_first_with_the_device_it_follows_and_the_one_in_use_is_ticked() {
        let on_default = sound(None).microphone_options(None, &Words::english());
        assert_eq!(on_default.len(), 3);
        assert_eq!((on_default[0].name.as_str(), on_default[0].note.as_deref()), ("Default", Some(DESK)));
        assert_eq!(on_default.iter().map(|row| row.ticked()).collect::<Vec<_>>(), [true, false, false]);
        assert_eq!(on_default[0].asked, None, "the default is asked for with no id");

        let on_headset = sound(Some(HEADSET)).microphone_options(Some(&picked(HEADSET)), &Words::english());
        assert_eq!(on_headset.iter().map(|row| row.ticked()).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(on_headset[1].asked, Some(picked(HEADSET)));
        assert!(on_headset.iter().all(|row| !row.away()));
    }

    #[test]
    fn a_saved_device_that_is_not_here_is_listed_last_not_connected_and_never_ticked() {
        // The studio microphone was chosen, is unplugged, and the default stands in for it.
        let studio = Picked { id: "usb-studio".into(), label: "Studio".into() };
        let rows = sound(None).microphone_options(Some(&studio), &Words::english());
        assert_eq!(rows.len(), 4);
        let away = &rows[3];
        assert_eq!((away.name.as_str(), away.note.as_deref()), ("Studio", Some("Not connected")));
        assert_eq!(away.state, DeviceState::Away);
        assert_eq!(away.asked, Some(studio), "known by the name it was picked by");
        assert!(rows[0].ticked(), "what is in use is ticked, which is the default");
    }

    #[test]
    fn nothing_is_added_for_a_saved_device_that_is_here_or_for_none_and_a_menu_passes_none() {
        assert_eq!(sound(Some(HEADSET)).microphone_options(Some(&picked(HEADSET)), &Words::english()).len(), 3);
        assert_eq!(sound(None).speaker_options(None, &Words::english()).len(), 3);
    }

    #[test]
    fn a_default_the_module_cannot_name_has_no_note() {
        let nameless = Sound { microphones: vec![device("", None)], ..Sound::default() };
        let rows = nameless.microphone_options(None, &Words::english());
        assert_eq!((rows[0].name.as_str(), rows[0].note.as_deref()), ("Default", None));
    }
}
