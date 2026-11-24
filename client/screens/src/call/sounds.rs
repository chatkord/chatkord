//! The sound of a call: its devices, what failed of them, and how loud each person and share plays here.

use super::*;

impl Call {
    /// Whether `user_id` is someone else here sharing a screen that carries sound.
    pub fn shares_sound(&self, user_id: &MemberId) -> bool {
        self.shares(user_id) && self.room.shares_sound(user_id)
    }

    /// Why `user_id`'s screen's sound is not heard here, while its picture may play.
    pub fn unheard(&self, user_id: &MemberId) -> Option<&Failure> {
        self.unheard.get(user_id)
    }

    /// Whose share's sound the open popover is about, over the call; that of a screen in its own window opens there.
    pub fn share_sound(&self) -> Option<&MemberId> {
        match &self.open {
            Some(Opened::ShareSound(user_id))
                if self.open() == Some(Over::ShareSound) && !self.is_out(user_id, Video::Screen) =>
            {
                Some(user_id)
            }
            _ => None,
        }
    }

    /// Every sound in the call but this account's own, as Audio controls lists them: each person's voice, then the
    /// sound of their share while it has one.
    pub fn sounds(&self) -> Vec<(&Tile, Audio)> {
        self.tiles
            .iter()
            .filter(|tile| !tile.own())
            .filter_map(|tile| tile.user_id.as_ref().map(|user_id| (tile, user_id)))
            .flat_map(|(tile, user_id)| {
                let share = self.shares_sound(user_id).then_some((tile, Audio::Share));
                std::iter::once((tile, Audio::Voice)).chain(share)
            })
            .collect()
    }

    /// A notice the host sent of `subject`, as a toast: a microphone that failed is marked on its control until it is
    /// pressed or heard again, one that moved to another device or came back is news only.
    fn noticed(&mut self, subject: Subject, said: Sentence) -> Telling {
        match (subject, &said) {
            (Subject::Microphone, Sentence::SoundBack(_)) => {
                self.faults.clear(Device::Microphone);
                Telling::of(design::Status::Success, said)
            }
            (_, Sentence::SoundBack(_)) => Telling::of(design::Status::Success, said),
            (Subject::Microphone, Sentence::SoundGone { .. }) => Telling::of(design::Status::Warning, said),
            (Subject::Microphone, _) => {
                self.faults.set(Device::Microphone, said.clone());
                Telling::of(design::Status::Danger, said)
            }
            _ => Telling::of(design::Status::Warning, said),
        }
    }

    /// A device of `subject` that had not answered opened: a microphone's mark is cleared, as a press clears it.
    pub(super) fn answered(&mut self, subject: Subject) {
        if subject == Subject::Microphone {
            self.faults.clear(Device::Microphone);
        }
    }

    /// What failed of `device`, which its control marks and says as its tooltip.
    pub fn fault(&self, device: Device) -> Option<&Sentence> {
        self.faults.of(device).filter(|_| self.live())
    }

    /// The devices the call can use and the ones it does.
    pub fn sound(&self) -> Option<&Sound> {
        self.sound.as_ref()
    }
}

impl Place {
    pub(super) fn notice_told(&mut self, subject: Subject, said: Sentence, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live()) {
            *told = Some(Told::Toast(call.noticed(subject, said)));
        }
        Vec::new()
    }

    pub(super) fn device_answered(&mut self, subject: Subject) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.answered(subject);
        }
        Vec::new()
    }

    pub(super) fn devices_found(&mut self, sound: Sound) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            call.sound = Some(sound);
        }
        Vec::new()
    }

    pub(super) fn toggle_share_sound(&mut self, user_id: MemberId) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if self.standing.as_ref().is_some_and(|standing| may_open(standing, signed, &user_id))
            && let Some(call) = signed.call.as_mut().filter(|call| call.shares_sound(&user_id))
        {
            let same = matches!(&call.open, Some(Opened::ShareSound(open)) if *open == user_id);
            call.open = if same { None } else { Some(Opened::ShareSound(user_id)) };
        }
        Vec::new()
    }

    pub(super) fn mute_for_me(&mut self, user_id: MemberId, audio: Audio) -> Vec<Ask> {
        let volume = self.volumes.toggle_mute(&user_id, audio);
        let server = self.server.url.clone();
        vec![
            Ask::Call(Control::Volume { user_id: user_id.clone(), audio, volume }),
            Ask::KeepVolume { server, user_id, audio, volume },
        ]
    }

    pub(super) fn set_volume(&mut self, user_id: MemberId, audio: Audio, percent: u16) -> Vec<Ask> {
        let Some(volume) = Volume::new(percent) else { return Vec::new() };
        self.volumes.set(&user_id, audio, volume);
        vec![Ask::Call(Control::Volume { user_id, audio, volume })]
    }

    pub(super) fn keep_volume(&mut self, user_id: MemberId, audio: Audio, percent: u16) -> Vec<Ask> {
        let Some(volume) = Volume::new(percent) else { return Vec::new() };
        self.volumes.set(&user_id, audio, volume);
        let server = self.server.url.clone();
        vec![
            Ask::Call(Control::Volume { user_id: user_id.clone(), audio, volume }),
            Ask::KeepVolume { server, user_id, audio, volume },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::harness::{
        Clock, ada_in_call, call_says, member, moderator_in_call, owls, sharer_in_call, tell, the_call,
    };
    use crate::notice::Subject;
    use crate::place;
    use crate::window::Window;
    use choices::{Audio, Volume};
    use proto::rules::Grants;
    use std::time::Instant;
    use words::Sentence;

    #[test]
    fn what_a_call_could_not_do_is_a_toast_and_a_failed_microphone_is_marked_on_its_control() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let microphone = |said: Sentence| place::Message::Call(place::CallMessage::Notice(Subject::Microphone, said));
        let fault = |window: &Window| {
            window.places()[0].call().and_then(|call| call.fault(crate::Device::Microphone)).map(Sentence::in_english)
        };
        let failed = Sentence::Said("The microphone could not be opened.".into());
        tell(&mut window, &clock, place, microphone(failed));
        let toast = window.toast().expect("told");
        assert_eq!((toast.kind(), toast.action()), (design::Status::Danger, None));
        assert_eq!(fault(&window).as_deref(), Some("The microphone could not be opened."));
        let gone = Sentence::SoundGone { was: "Desk".into(), now: Some("Laptop".into()) };
        tell(&mut window, &clock, place, microphone(gone));
        assert_eq!(window.toast().map(|toast| toast.kind()), Some(design::Status::Warning), "a fallback is a warning");
        assert!(fault(&window).is_some(), "and does not clear a failure");
        tell(&mut window, &clock, place, microphone(Sentence::SoundBack("Desk".into())));
        assert_eq!(window.toast().map(|toast| toast.kind()), Some(design::Status::Success), "back is good news");
        assert_eq!(fault(&window), None, "and the control is as at rest");
        tell(&mut window, &clock, place, microphone(Sentence::Said("again".into())));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute));
        assert_eq!(fault(&window), None, "pressed: a new try");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        window.update(crate::WindowMessage::DismissToast, clock.at());
        tell(&mut window, &clock, place, microphone(Sentence::Said("late".into())));
        assert!(window.toast().is_none(), "a call that is over says nothing more");
    }

    #[test]
    fn a_microphone_that_did_not_answer_is_marked_until_its_retry_opens_with_no_press() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let microphone = |said: Sentence| place::Message::Call(place::CallMessage::Notice(Subject::Microphone, said));
        let fault =
            |window: &Window| window.places()[0].call().and_then(|call| call.fault(crate::Device::Microphone)).cloned();
        let stalled = words::failure::of(&words::Failure::EngineStalled);
        tell(&mut window, &clock, place, microphone(stalled.clone()));
        assert_eq!(window.toast().map(|toast| toast.kind()), Some(design::Status::Danger));
        assert_eq!(fault(&window), Some(stalled.clone()), "marked on its control");
        tell(&mut window, &clock, place, microphone(Sentence::SoundBack("Desk".into())));
        assert_eq!(window.toast().map(|toast| toast.kind()), Some(design::Status::Success), "its retry opened");
        assert_eq!(fault(&window), None, "the mark is gone with no mute pressed");

        tell(&mut window, &clock, place, microphone(stalled.clone()));
        assert_eq!(fault(&window), Some(stalled));
        let answered = place::Message::Call(place::CallMessage::Answered(Subject::Microphone));
        tell(&mut window, &clock, place, answered);
        assert_eq!(fault(&window), None, "a device with no name to say back is cleared all the same");
        assert_eq!(window.toast().map(|toast| toast.kind()), Some(design::Status::Danger), "with no toast of its own");
    }

    #[test]
    fn a_persons_volume_plays_at_once_is_kept_once_let_go_and_starts_the_next_call() {
        let clock = Clock(Instant::now());
        let (mut window, place) = moderator_in_call(&clock, proto::rules::Place::BOTTOM, Grants::default());
        let quiet = Volume::new(40).unwrap();
        let moved = place::Message::Call(place::CallMessage::Volume(member("bo"), Audio::Voice, 40));
        let asked = tell(&mut window, &clock, place, moved);
        assert!(
            matches!(&asked[..], [crate::Ask::At(_, place::Ask::Call(place::Control::Volume { user_id, audio: Audio::Voice, volume }))]
                if user_id.as_str() == "bo" && *volume == quiet),
            "played at once, kept only when let go: {asked:?}"
        );
        assert_eq!(window.places()[0].volumes().of(&member("bo"), Audio::Voice), quiet);
        assert_eq!(
            window.places()[0].volumes().of(&member("bo"), Audio::Share),
            Volume::FULL,
            "the share's sound is its own"
        );
        let asked = tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::KeepVolume(member("bo"), Audio::Voice, 30)),
        );
        let kept = Volume::new(30).unwrap();
        assert!(
            matches!(&asked[..], [
                crate::Ask::At(_, place::Ask::Call(place::Control::Volume { volume: played, .. })),
                crate::Ask::At(_, place::Ask::KeepVolume { server, user_id, audio: Audio::Voice, volume }),
            ] if *played == kept && *server == owls().url && user_id.as_str() == "bo" && *volume == kept),
            "where it was let go, played and kept with this server's address: {asked:?}"
        );
        let past = place::Message::Call(place::CallMessage::Volume(member("bo"), Audio::Voice, 101));
        assert!(tell(&mut window, &clock, place, past).is_empty(), "no volume past full");
        let own = place::Message::Call(place::CallMessage::Volume(member("ada"), Audio::Voice, 40));
        assert!(tell(&mut window, &clock, place, own).is_empty(), "no volume for one's own sound");

        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Leave));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        let todo = tell(
            &mut window,
            &clock,
            place,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::Join { volumes, .. })] if volumes.of(&member("bo"), Audio::Voice) == kept),
            "the next call starts at it: {todo:?}"
        );
    }

    #[test]
    fn a_shares_sound_has_its_own_popover_only_while_someone_else_shares_it() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let share_sound = |window: &mut Window, who: &str| {
            call_says(window, &clock, place, place::CallMessage::ShareSound(member(who)));
            the_call(window).share_sound().map(MemberId::as_str).map(str::to_string)
        };
        assert_eq!(share_sound(&mut window, "bo"), None, "a screen shared with no sound has no volume");
        let sound = |on: bool| {
            let track = Track::ScreenAudio;
            let user_id = member("bo");
            place::CallMessage::Room(if on {
                RoomEvent::Started { user_id, track }
            } else {
                RoomEvent::Stopped { user_id, track }
            })
        };
        call_says(&mut window, &clock, place, sound(true));
        assert!(the_call(&window).shares_sound(&member("bo")));
        assert_eq!(share_sound(&mut window, "bo").as_deref(), Some("bo"));
        assert_eq!(the_call(&window).open(), Some(place::Over::ShareSound));
        assert_eq!(share_sound(&mut window, "bo"), None, "a second press closes it");
        for who in ["ada", "cy", "nobody"] {
            assert_eq!(share_sound(&mut window, who), None, "nor on {who}, who shares no sound here");
        }
        share_sound(&mut window, "bo");
        call_says(&mut window, &clock, place, sound(false));
        assert_eq!(the_call(&window).share_sound(), None, "it closes when the sound stops");
        assert_eq!(the_call(&window).open(), None);
    }

    #[test]
    fn mute_for_me_silences_one_sound_kept_like_any_volume_and_its_undo_brings_back_the_one_before() {
        let clock = Clock(Instant::now());
        let (mut window, place) = moderator_in_call(&clock, proto::rules::Place::BOTTOM, Grants::default());
        let quiet = Volume::new(40).unwrap();
        call_says(&mut window, &clock, place, place::CallMessage::KeepVolume(member("bo"), Audio::Voice, 40));
        let played = |asked: &[crate::Ask]| match asked {
            [
                crate::Ask::At(
                    _,
                    place::Ask::Call(place::Control::Volume { user_id, audio: Audio::Voice, volume: played }),
                ),
                crate::Ask::At(_, place::Ask::KeepVolume { user_id: kept_for, audio: Audio::Voice, volume, .. }),
            ] if user_id.as_str() == "bo" && kept_for.as_str() == "bo" && played == volume => Some(*volume),
            _ => None,
        };
        let mute = place::CallMessage::MuteForMe(member("bo"), Audio::Voice);
        let asked = call_says(&mut window, &clock, place, mute.clone());
        assert_eq!(played(&asked), Some(Volume::SILENT), "silent at once, and kept: {asked:?}");
        let asked = call_says(&mut window, &clock, place, mute.clone());
        assert_eq!(played(&asked), Some(quiet), "back to the volume before: {asked:?}");
        assert_eq!(window.places()[0].volumes().of(&member("bo"), Audio::Voice), quiet);
        let own = place::CallMessage::MuteForMe(member("ada"), Audio::Voice);
        assert!(call_says(&mut window, &clock, place, own).is_empty(), "never one's own sound");
    }

    #[test]
    fn audio_controls_list_every_sound_in_the_call_but_ones_own_and_open_one_thing_at_a_time() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        for (user_id, track) in [("bo", Track::ScreenAudio), ("cy", Track::ScreenVideo)] {
            call_says(
                &mut window,
                &clock,
                place,
                place::CallMessage::Room(RoomEvent::Started { user_id: member(user_id), track }),
            );
        }
        call_says(&mut window, &clock, place, place::CallMessage::AudioControls);
        assert_eq!(the_call(&window).open(), Some(place::Over::Audio));
        let sounds: Vec<(Option<MemberId>, Audio)> =
            the_call(&window).sounds().into_iter().map(|(tile, audio)| (tile.user_id.clone(), audio)).collect();
        let sound = |user_id: &str, audio| (Some(member(user_id)), audio);
        assert_eq!(
            sounds,
            [
                sound("bo", Audio::Voice),
                sound("bo", Audio::Share),
                sound("cy", Audio::Voice),
                sound("di", Audio::Voice)
            ],
            "each voice, a share's sound after its sharer, a share with no sound left out, and never one's own"
        );
        call_says(&mut window, &clock, place, place::CallMessage::ViewMenu);
        assert_eq!(the_call(&window).open(), Some(place::Over::View), "another popover takes its place");
        call_says(&mut window, &clock, place, place::CallMessage::AudioControls);
        call_says(&mut window, &clock, place, place::CallMessage::AudioControls);
        assert_eq!(the_call(&window).open(), None, "a second press closes it");
    }
}
