//! The volume of a shared screen's sound and of a person's voice.

use crate::Renderer;
use crate::app::Part;
use crate::labels::{label, phrase};
use design::Label;
use screens::{Call, Place, PlaceId, PlaceMessage, WindowMessage as Message};
use terms::MemberId;
use words::{Phrase, Sentence, Words};

/// The popover of the sound of the shared screen it is open on over the call: how loud it plays here.
pub(crate) fn share_sound_popover<'a, R: Renderer + 'a>(
    place: &Place,
    call: &Call,
    words: &'a Words,
) -> Option<Part<'a, R>> {
    let user_id = call.share_sound()?;
    let tile = call.tiles().iter().find(|tile| tile.user_id.as_ref() == Some(user_id))?;
    Some(screen_sound(place, user_id, &tile.name, words))
}

/// How loud the sound of `name`'s shared screen plays here, `user_id` theirs.
pub(crate) fn screen_sound<'a, R: Renderer + 'a>(
    place: &Place,
    user_id: &MemberId,
    name: &str,
    words: &'a Words,
) -> Part<'a, R> {
    let audio = choices::Audio::Share;
    let volume = place.volumes().of(user_id, audio);
    let row = sound_controls(place.id(), volume, user_id, audio, phrase(Phrase::Volume, words), words);
    let said = crate::PersonWords {
        name: label(&words.say(&Sentence::SoundFromScreen(name.to_string()))),
        volume: phrase(Phrase::Volume, words),
        as_admin: phrase(Phrase::AsAnAdmin, words),
        roles: phrase(Phrase::Roles, words),
    };
    let heard = crate::Heard {
        words: row.words,
        percent: row.percent,
        muted: row.muted,
        on_change: row.on_change,
        on_commit: row.on_commit,
        on_mute: row.on_mute,
    };
    crate::person_popover(said, None, None, Vec::new(), None, Some(heard), None, Vec::new())
}

/// What one sound's volume row says and sends.
pub(crate) struct SoundControls {
    pub(crate) words: crate::VolumeWords,
    pub(crate) percent: u16,
    pub(crate) muted: bool,
    pub(crate) on_change: Box<dyn Fn(u16) -> Message>,
    pub(crate) on_commit: Box<dyn Fn(u16) -> Message>,
    pub(crate) on_mute: Message,
}

/// The row of `user_id`'s `audio` at `volume` on place `id`, its slider named `slider`.
pub(crate) fn sound_controls(
    id: PlaceId,
    volume: choices::Volume,
    user_id: &MemberId,
    audio: choices::Audio,
    slider: Label,
    words: &Words,
) -> SoundControls {
    let at = move |message: screens::CallMessage| Message::At(id, PlaceMessage::Call(message));
    let mute = if volume.silent() { Phrase::UnmuteForMe } else { Phrase::MuteForMe };
    let (moving, letting_go) = (user_id.clone(), user_id.clone());
    SoundControls {
        words: crate::VolumeWords { slider, mute: phrase(mute, words), figure: words.percent(volume.percent().into()) },
        percent: volume.percent().into(),
        muted: volume.silent(),
        on_change: Box::new(move |percent| at(screens::CallMessage::Volume(moving.clone(), audio, percent))),
        on_commit: Box::new(move |percent| at(screens::CallMessage::KeepVolume(letting_go.clone(), audio, percent))),
        on_mute: at(screens::CallMessage::MuteForMe(user_id.clone(), audio)),
    }
}
