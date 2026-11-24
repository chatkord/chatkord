//! What the person wants of the call they are in that is a state, not an act: whether they are muted and deafened,
//! how loud each member plays, which pictures show nowhere. It is kept whole and the newest holds, so however many
//! changes come before the call can read them, none is lost and none waits in a queue that fills.

use choices::{Audio, State, Volume, Volumes};
use proto::video::Video;
use screens::Control;
use terms::MemberId;

use super::Command;

/// A choice and how many times it was made. Made again the same it is still made: a microphone that would not open
/// is asked for again by the same "unmute".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Said<T> {
    pub(super) what: T,
    pub(super) times: u64,
}

impl<T> Said<T> {
    fn say(&mut self, what: T) {
        self.what = what;
        self.times += 1;
    }
}

/// What the person wants of a running call, whole. A host keeps it in a `tokio::sync::watch` and changes it as the
/// person does; the call carries out what differs from what it last read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    muted: Said<bool>,
    deafened: Said<bool>,
    volumes: Volumes,
    unseen: Vec<(MemberId, Video)>,
    said: Vec<((MemberId, Video), Asked)>,
}

/// The last the person said of a picture: paused, or started again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Paused,
    Resumed,
}

impl Wants {
    /// What a call is joined with: the place's state, and how loud each member plays from the start.
    pub fn starting(start: State, volumes: Volumes) -> Wants {
        Wants {
            muted: Said { what: start.mic_muted, times: 0 },
            deafened: Said { what: start.deafened, times: 0 },
            volumes,
            unseen: Vec::new(),
            said: Vec::new(),
        }
    }

    /// The state the place is joined in.
    pub(super) fn start(&self) -> State {
        State { mic_muted: self.muted.what, deafened: self.deafened.what }
    }

    /// Mute this account's microphone, or unmute it.
    pub fn mute(&mut self, muted: bool) {
        self.muted.say(muted);
    }

    /// Keep the call from the speaker, or let it be heard.
    pub fn deafen(&mut self, deafened: bool) {
        self.deafened.say(deafened);
    }

    /// Play `user_id`'s `audio` at `volume` on this device, now and whenever it is taken again.
    pub fn play(&mut self, user_id: &MemberId, audio: Audio, volume: Volume) {
        self.volumes.set(user_id, audio, volume);
    }

    /// The pictures the window shows nowhere now, by their senders' ids and kinds; one unseen for
    /// [`crate::watch::viewing::UNSEEN`] has its stream held until it is shown again.
    pub fn show_none_of(&mut self, unseen: Vec<(MemberId, Video)>) {
        self.unseen = unseen;
    }

    /// Pause `user_id`'s picture of `video`, watched already or not yet: its stream is held until [`Wants::resume`],
    /// whatever the window shows.
    pub fn pause(&mut self, user_id: &MemberId, video: Video) {
        self.say(user_id, video, Asked::Paused);
    }

    /// Start again a picture [`Wants::pause`] held.
    pub fn resume(&mut self, user_id: &MemberId, video: Video) {
        self.say(user_id, video, Asked::Resumed);
    }

    fn say(&mut self, user_id: &MemberId, video: Video, said: Asked) {
        self.ended(user_id, video);
        self.said.push(((user_id.clone(), video), said));
    }

    /// Forget what was said of `user_id`'s picture of `video`, which is no longer watched: a picture of theirs watched
    /// again is a new one. Whether anything was forgotten.
    pub fn ended(&mut self, user_id: &MemberId, video: Video) -> bool {
        let before = self.said.len();
        self.said.retain(|((who, of), _)| !(who == user_id && *of == video));
        before != self.said.len()
    }

    /// Takes what the person asked of the call: a state is kept here, and an act comes back as the command to send.
    pub fn take(&mut self, control: Control) -> Option<Command> {
        match control {
            Control::Mute(muted) => self.mute(muted),
            Control::Deafen(deafened) => self.deafen(deafened),
            Control::Volume { user_id, audio, volume } => self.play(&user_id, audio, volume),
            Control::Unseen(unseen) => self.show_none_of(unseen),
            Control::Pause { user_id, video } => self.pause(&user_id, video),
            Control::Resume { user_id, video } => self.resume(&user_id, video),
            Control::Microphone(asked) => return Some(Command::Microphone(asked)),
            Control::Speaker(asked) => return Some(Command::Speaker(asked)),
            Control::Processing(asked) => return Some(Command::Processing(asked)),
            Control::ShareSoundOf(app) => return Some(Command::ShareSoundOf(app)),
            Control::StopShare => return Some(Command::StopShare),
            Control::Camera(asked) => return Some(Command::Camera(asked.map(crate::camera::Pick::Device))),
        }
        None
    }

    pub(super) fn muted(&self) -> Said<bool> {
        self.muted
    }

    pub(super) fn deafened(&self) -> Said<bool> {
        self.deafened
    }

    pub(super) fn volumes(&self) -> &Volumes {
        &self.volumes
    }

    pub(super) fn unseen(&self) -> &[(MemberId, Video)] {
        &self.unseen
    }

    pub(super) fn paused(&self) -> Vec<(MemberId, Video)> {
        self.saying(Asked::Paused)
    }

    pub(super) fn resumed(&self) -> Vec<(MemberId, Video)> {
        self.saying(Asked::Resumed)
    }

    fn saying(&self, what: Asked) -> Vec<(MemberId, Video)> {
        self.said.iter().filter(|(_, said)| *said == what).map(|(picture, _)| picture.clone()).collect()
    }

    /// Each sound that plays at another volume than in `was`, with the volume it plays at now.
    pub(super) fn played_otherwise_than(&self, was: &Wants) -> Vec<(MemberId, Audio, Volume)> {
        let members: std::collections::BTreeSet<&MemberId> =
            self.volumes.iter().chain(was.volumes.iter()).map(|(user_id, _)| user_id).collect();
        members
            .into_iter()
            .flat_map(|user_id| [Audio::Voice, Audio::Share].map(|audio| (user_id, audio)))
            .filter_map(|(user_id, audio)| {
                let now = self.volumes.of(user_id, audio);
                (now != was.volumes.of(user_id, audio)).then(|| (user_id.clone(), audio, now))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";
    const BEA: &str = "bea";

    fn member(id: &str) -> MemberId {
        MemberId::parse(id).expect("a member id")
    }

    fn volume(percent: u16) -> Volume {
        Volume::new(percent).unwrap()
    }

    #[test]
    fn a_state_is_kept_and_an_act_is_handed_on_as_its_command() {
        let mut wants = Wants::default();
        let kept = [
            Control::Mute(true),
            Control::Deafen(true),
            Control::Volume { user_id: member(ADA), audio: Audio::Voice, volume: volume(40) },
            Control::Unseen(vec![(member(BEA), Video::Screen)]),
            Control::Pause { user_id: member(BEA), video: Video::Camera },
            Control::Pause { user_id: member(BEA), video: Video::Camera },
        ];
        for control in kept {
            assert!(wants.take(control.clone()).is_none(), "{control:?} is a state, kept");
        }
        assert_eq!(wants.start(), State { mic_muted: true, deafened: true });
        assert_eq!(wants.volumes().of(&member(ADA), Audio::Voice), volume(40));
        assert_eq!(wants.unseen(), [(member(BEA), Video::Screen)]);
        assert_eq!(wants.paused(), [(member(BEA), Video::Camera)], "paused once, however often said");
        assert!(wants.take(Control::Resume { user_id: member(BEA), video: Video::Camera }).is_none());
        assert!(wants.paused().is_empty());
        assert_eq!(wants.resumed(), [(member(BEA), Video::Camera)], "started again");
        assert!(matches!(wants.take(Control::StopShare), Some(Command::StopShare)));
        assert!(matches!(wants.take(Control::Speaker(None)), Some(Command::Speaker(None))));
    }

    #[test]
    fn a_picture_that_ends_is_forgotten_so_the_next_of_it_is_a_new_one() {
        let mut wants = Wants::default();
        wants.pause(&member(ADA), Video::Screen);
        wants.resume(&member(BEA), Video::Camera);
        assert!(wants.ended(&member(ADA), Video::Screen));
        assert!(wants.ended(&member(BEA), Video::Camera));
        assert!(!wants.ended(&member(BEA), Video::Camera), "nothing left to forget");
        assert!(wants.paused().is_empty() && wants.resumed().is_empty());
        wants.pause(&member(ADA), Video::Screen);
        wants.resume(&member(ADA), Video::Screen);
        wants.pause(&member(ADA), Video::Screen);
        assert_eq!((wants.paused().len(), wants.resumed().len()), (1, 0), "the last word on a picture holds");
    }

    #[test]
    fn the_newest_of_many_changes_is_what_holds_and_the_same_said_again_is_still_said() {
        let mut wants = Wants::starting(State::default(), Volumes::default());
        let was = wants.clone();
        for percent in [90, 70, 50, 30] {
            wants.play(&member(ADA), Audio::Voice, volume(percent));
        }
        wants.mute(true);
        wants.mute(false);
        assert_eq!(wants.played_otherwise_than(&was), [(member(ADA), Audio::Voice, volume(30))]);
        assert_eq!(wants.muted().what, was.muted().what, "unmuted again");
        assert_ne!(wants.muted().times, was.muted().times, "but said, so a microphone that failed is tried again");
        assert_eq!(wants.deafened(), was.deafened(), "nothing was said of the speaker");
        wants.play(&member(ADA), Audio::Voice, Volume::default());
        assert!(wants.played_otherwise_than(&was).is_empty(), "back where it was: nothing to carry out");
    }
}
