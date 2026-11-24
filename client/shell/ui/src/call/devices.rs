//! The call's audio and view panels and the device list.

use super::sound::sound_controls;
use crate::app::Part;
use crate::discussion::Handles;
use crate::labels::{label, phrase};
use crate::{DeviceSection, MenuChoice, Renderer, device_menu};
use screens::{Call, DeviceOption, Place, Sound, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

/// Audio controls: every sound in the call but this account's own, each with its volume.
pub(crate) fn audio_panel<'a, R: Renderer + 'a>(
    place: &Place,
    call: &Call,
    handles: &Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let rows = call
        .sounds()
        .iter()
        .filter_map(|(tile, audio)| {
            let user_id = tile.user_id.as_ref()?;
            let named = match audio {
                choices::Audio::Voice => tile.name.clone(),
                choices::Audio::Share => words.say(&Sentence::SoundFromScreen(tile.name.clone())),
            };
            let volume = place.volumes().of(user_id, *audio);
            let row = sound_controls(place.id(), volume, user_id, *audio, label(&named), words);
            Some(crate::SoundRow {
                name: label(&tile.name),
                colour: tile.colour,
                picture: place.picture_of(user_id).and_then(handles),
                share: *audio == choices::Audio::Share,
                words: row.words,
                percent: row.percent,
                muted: row.muted,
                on_change: row.on_change,
                on_commit: row.on_commit,
                on_mute: row.on_mute,
            })
        })
        .collect();
    crate::audio_controls(
        crate::AudioWords {
            title: phrase(Phrase::AudioControls, words),
            empty: words.phrase(Phrase::AudioControlsEmpty).to_string(),
        },
        rows,
    )
}

/// The call's Voice options: how this device draws the call.
pub(crate) fn view_panel<'a, R: Renderer + 'a>(view: choices::CallView, words: &'a Words) -> Part<'a, R> {
    let row = |name, on, pick| crate::SwitchRow {
        name: phrase(name, words),
        on,
        flip: Message::Look(choices::LookPick::View(pick)),
    };
    crate::switch_panel(
        phrase(Phrase::VoiceOptions, words),
        vec![
            row(Phrase::HidePeople, view.people == choices::People::Pictured, choices::ViewPick::People),
            row(Phrase::AlwaysShowControls, view.controls == choices::Controls::Always, choices::ViewPick::Controls),
            row(Phrase::HideOwnShare, view.own_share == choices::OwnShare::Hidden, choices::ViewPick::OwnShare),
        ],
    )
}

/// The call's microphones and speakers, the ones in use ticked.
pub(crate) fn devices<'a, R: Renderer + 'a>(sound: &Sound, words: &'a Words) -> Part<'a, R> {
    let section = |heading: Phrase, options: Vec<DeviceOption>, pick: fn(Option<choices::Picked>) -> Message| {
        let rows = options
            .into_iter()
            .map(|each| MenuChoice {
                leading: None,
                label: label(&each.name),
                note: each.note.as_deref().map(label),
                chosen: each.ticked(),
                message: pick(each.asked),
            })
            .collect();
        DeviceSection { heading: phrase(heading, words), rows }
    };
    device_menu(
        phrase(Phrase::DeviceMenu, words),
        vec![
            section(Phrase::Microphone, sound.microphone_options(None, words), Message::PickMicrophone),
            section(Phrase::Speaker, sound.speaker_options(None, words), Message::PickSpeaker),
        ],
    )
}
