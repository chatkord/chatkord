//! What the call decides about this account's own pictures: a screen or a camera that took a step, and what the room is
//! told of it.

use super::{Act, State};
use crate::room::Report;
use crate::send::sender::Was;
use proto::video::Video;
use screens::{CameraSent, RoomEvent, Shared, Subject, Track};
use terms::MemberId;
use words::Failure;

/// What this account's own pictures did.
pub(in crate::room) enum OwnInput {
    /// This account's screen took a step.
    Screen(Turned<ScreenSent, Shared>),
    /// This account's camera took a step.
    Camera(Turned<Picture, CameraSent>),
}

/// What the driver carries out for this account's own pictures.
pub(in crate::room) enum OwnAct {
    /// Send the screen asked for.
    ShareAsk(crate::send::own::Asked),
    /// Stop sending the screen.
    ShareStop,
    /// Turn on the camera asked for.
    CameraAsk(crate::camera::Pick),
    /// Turn off the camera.
    CameraStop,
    /// The server ended this account's own picture.
    Denied(Video),
    /// Whether anyone takes this account's own picture now.
    OwnWatched(Video, bool),
    /// Send this application's sound with the screen.
    ShareSoundOf(screens::SoundApp),
}

impl From<OwnAct> for Act {
    fn from(act: OwnAct) -> Self {
        Self::Own(act)
    }
}

/// What `input` changes in `state`, and the acts that follow.
pub(super) fn decide(state: &mut State, input: OwnInput, acts: &mut Vec<Act>) {
    match input {
        OwnInput::Screen(turn) => state.screen_turned(turn, acts),
        OwnInput::Camera(turn) => state.camera_turned(turn, acts),
    }
}

/// What a picture sent from here is sent at.
pub(in crate::room) struct Picture {
    pub(in crate::room) width: u32,
    pub(in crate::room) height: u32,
    pub(in crate::room) encoder: screens::Route,
}

/// This account's screen as it went out.
pub(in crate::room) struct ScreenSent {
    pub(in crate::room) picture: Picture,
    /// What the share's sound is, once it is known.
    pub(in crate::room) sound_said: Option<screens::SoundSaid>,
    /// Why its sound could not be sent.
    pub(in crate::room) sound_refused: Option<Failure>,
}

/// What one step of this account's own screen or camera came to.
pub(in crate::room) enum Turned<Sent, Said> {
    /// It is being sent: the room is told.
    Sent(Sent),
    /// It is over, as the person is told, and whether the room was told it started.
    Over(Said, Was),
    /// It was let go for another ask, which is opening now.
    Replaced(Was),
}

/// The server never announces a person's own stream to them: the room hears it from here.
pub(super) fn own_started(me: Option<&MemberId>, track: Track, acts: &mut Vec<Act>) {
    if let Some(user_id) = me {
        acts.push(Act::Report(Report::Room(RoomEvent::Started { user_id: user_id.clone(), track })));
    }
}

/// The room hears one's own `track` ended, as it heard it start, where it was told it started.
fn own_stopped(me: Option<&MemberId>, was: Was, track: Track, acts: &mut Vec<Act>) {
    if let (Was::Live, Some(user_id)) = (was, me) {
        acts.push(Act::Report(Report::Room(RoomEvent::Stopped { user_id: user_id.clone(), track })));
    }
}

impl State {
    /// This account's own screen took a step.
    pub(super) fn screen_turned(&mut self, turn: Turned<ScreenSent, Shared>, acts: &mut Vec<Act>) {
        match turn {
            Turned::Sent(sent) => {
                own_started(self.me.as_ref(), Track::ScreenVideo, acts);
                let Picture { width, height, encoder } = sent.picture;
                acts.push(Act::Report(Report::Share(Shared::Started { width, height, encoder })));
                if let Some(said) = sent.sound_said {
                    acts.push(Act::Report(Report::Share(Shared::Sound(said))));
                }
                if let Some(failed) = sent.sound_refused {
                    acts.push(Act::Report(Report::Notice(Subject::ShareSound, words::failure::of(&failed))));
                }
            }
            Turned::Over(told, was) => {
                own_stopped(self.me.as_ref(), was, Track::ScreenVideo, acts);
                acts.push(Act::Report(Report::Share(told)));
            }
            Turned::Replaced(was) => own_stopped(self.me.as_ref(), was, Track::ScreenVideo, acts),
        }
    }

    /// This account's own camera took a step.
    pub(super) fn camera_turned(&mut self, turn: Turned<Picture, CameraSent>, acts: &mut Vec<Act>) {
        match turn {
            Turned::Sent(Picture { width, height, encoder }) => {
                own_started(self.me.as_ref(), Track::Camera, acts);
                acts.push(Act::Report(Report::Camera(CameraSent::Started { width, height, encoder })));
            }
            Turned::Over(told, was) => {
                own_stopped(self.me.as_ref(), was, Track::Camera, acts);
                acts.push(Act::Report(Report::Camera(told)));
            }
            Turned::Replaced(was) => own_stopped(self.me.as_ref(), was, Track::Camera, acts),
        }
    }
}
