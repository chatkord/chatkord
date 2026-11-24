//! What the call decides about the screens it watches and the streams it takes: the lane's steps, holds, key frames
//! and cards.

use super::{Act, State};
use crate::room::lane::{self, Action, Attempt, Began, Doing, Refusal, Retry, Stream};
use crate::room::unreached::Which;
use crate::room::{Over, Report, restart_said, share_sound_unheard, track_of};
use crate::watch::viewing::{Viewed, Waiting};
use crate::watch::watching::{Reopened, Watching};
use proto::chatkord_v1::TrackKind;
use proto::video::Video;
use screens::{RoomEvent, Take, Watched};
use terms::MemberId;
use tokio::time::Instant;
use webrtc::VoiceRefused;
use words::Failure;

/// What the lane and the screens watched did.
pub(in crate::room) enum WatchInput {
    /// A step of the lane is done.
    Landed(Landed),
    /// A step of the lane ended without an answer.
    LaneLost,
    /// Brings what the call takes a step toward the room while the lane is free.
    Advance,
    /// The server answered the ask to hold a stream.
    Held(Viewed, bool),
    /// A held picture's decoder opened again, or did not.
    Opened(Viewed),
    /// The decoder of a picture needs its sender to send a key frame.
    NeedsKeyFrame(Viewed),
    /// Key frames wanted may be asked for; what each decoder says of its stream.
    KeyFramesDue(Vec<(Viewed, Waiting)>),
    /// Screens unseen for their time are held.
    HoldUnseen,
    /// A picture's first frame was decoded.
    FirstPicture(Viewed),
}

/// What the driver carries out for the lane and the screens watched.
pub(in crate::room) enum WatchAct {
    /// Stop taking a stream.
    Forget(MemberId, TrackKind),
    /// Take a stream on the lane.
    Take(Stream, Began),
    /// Show a picture on the lane, as the person has it, numbered `number`.
    Watch { viewed: Viewed, began: Began, paused: bool, number: u64 },
    /// Let go of a picture on the lane.
    LetGo(Viewed),
    /// Let go of the lane's step, which never came back.
    DropStep,
    /// Tell a picture's decoder whether the window shows it.
    Shown(Viewed, bool),
    /// A picture's held stream is to be started again: its decoder opens, and says when it has
    /// ([`WatchInput::Opened`]).
    Resumed(Viewed),
    /// The stream of this viewing is held at the server: its decoder is closed.
    Close(Viewed),
    /// Ask the server to hold a picture's stream.
    Hold(Viewed),
    /// Ask the server to start a held stream again.
    StartAgain(Viewed),
    /// Ask a picture's sender for a key frame.
    AskKeyFrame(Viewed),
    /// Report a screen's card at its first picture.
    FirstCard(Viewed),
    /// Report how the screens are.
    LookAtScreens,
}

impl From<WatchAct> for Act {
    fn from(act: WatchAct) -> Self {
        Self::Watch(act)
    }
}

/// What `input` changes in `state` at `now`, and the acts that follow.
pub(super) fn decide(state: &mut State, input: WatchInput, now: Instant, acts: &mut Vec<Act>) {
    match input {
        WatchInput::Landed(landed) => state.landed(landed, now, acts),
        WatchInput::LaneLost => state.lane_lost(acts),
        WatchInput::Advance => state.advance(now, acts),
        WatchInput::Held(viewed, held) => state.held(&viewed, held, now, acts),
        WatchInput::Opened(viewed) => state.decoder_opened(viewed, now, acts),
        WatchInput::NeedsKeyFrame(viewed) => {
            if let Some(watching) = state.watching.get_mut(&viewed) {
                watching.want_key_frame(now);
            }
        }
        WatchInput::KeyFramesDue(waiting) => state.key_frames(waiting, now, acts),
        WatchInput::HoldUnseen => state.hold_unseen(now, acts),
        WatchInput::FirstPicture(viewed) => state.first_picture(viewed, acts),
    }
}

/// What a step of the lane did.
pub(in crate::room) enum Landed {
    Took(Stream, Began, Result<(), VoiceRefused>),
    /// A picture shown, as the person had it (paused or not), or why not.
    Watched {
        viewed: Viewed,
        began: Began,
        shown: Result<bool, Failure>,
    },
    Unwatched(Viewed),
    /// The receive connection's ICE asked to start again, or why not.
    Mended(Result<(), VoiceRefused>),
}

/// How this call's take of a member's stream stands, for the room: told after the event that lists the stream.
fn taking(user_id: &MemberId, kind: TrackKind, take: Take, acts: &mut Vec<Act>) {
    if let Some(track) = track_of(kind) {
        acts.push(Act::Report(Report::Room(RoomEvent::Taking { user_id: user_id.clone(), track, take })));
    }
}

impl State {
    /// When the first screen unseen for its time is to be held.
    pub(in crate::room) fn hold_at(&self) -> Option<Instant> {
        self.watching.values().filter_map(Watching::hold_at).min()
    }

    /// When the first key frame wanted may be asked for.
    pub(in crate::room) fn key_frame_at(&self) -> Option<Instant> {
        self.watching.values().filter_map(Watching::key_frame_at).min()
    }

    /// Whether the person has the picture of `viewed` paused.
    pub(in crate::room) fn is_paused(&self, viewed: &Viewed) -> bool {
        self.watching.get(viewed).is_some_and(Watching::is_paused)
    }

    /// Whether the card of `viewed` was reported at its first picture.
    pub(in crate::room) fn first_reported(&self, viewed: &Viewed) -> bool {
        self.watching.get(viewed).is_some_and(Watching::first_reported)
    }

    /// When the earliest refused stream still wanted is tried again, while the lane is free to try it.
    pub(in crate::room) fn retry_at(&self) -> Option<Instant> {
        self.lane.is_idle().then(|| lane::due(&self.wanted, &self.tried)).flatten()
    }

    /// Tells the viewing of `viewed` whether the window shows it now, and opens its held stream's decoder again once
    /// it does.
    pub(super) fn see(&mut self, viewed: &Viewed, now: Instant, acts: &mut Vec<Act>) {
        let shown = !self.unseen.contains(viewed);
        let Some(watching) = self.watching.get_mut(viewed) else { return };
        acts.push(WatchAct::Shown(viewed.clone(), shown).into());
        if watching.seen(shown, now) {
            acts.push(WatchAct::Resumed(viewed.clone()).into());
        }
    }

    /// The person pauses the picture of `viewed`, which the server is asked to hold.
    pub(super) fn pause(&mut self, viewed: &Viewed, acts: &mut Vec<Act>) {
        if self.watching.get_mut(viewed).is_some_and(Watching::pause) {
            acts.push(WatchAct::Hold(viewed.clone()).into());
        }
    }

    /// The person starts the picture of `viewed` again.
    pub(super) fn resume(&mut self, viewed: &Viewed, acts: &mut Vec<Act>) {
        if self.watching.get_mut(viewed).is_some_and(Watching::resume) {
            acts.push(WatchAct::Resumed(viewed.clone()).into());
        }
    }

    /// The server answered the ask to hold the stream of `viewed`: held, its decoder is closed until it starts again.
    fn held(&mut self, viewed: &Viewed, held: bool, now: Instant, acts: &mut Vec<Act>) {
        let Some(watching) = self.watching.get_mut(viewed) else { return };
        if watching.hold_answered(held, now) {
            acts.push(WatchAct::Resumed(viewed.clone()).into());
        } else if held && watching.on_hold() {
            acts.push(WatchAct::Close(viewed.clone()).into());
        }
    }

    /// The decoder of `viewed`, held, opened again: the server is asked to start its stream, or it closes again if
    /// the person paused it meanwhile.
    fn decoder_opened(&mut self, viewed: Viewed, now: Instant, acts: &mut Vec<Act>) {
        let Some(watching) = self.watching.get_mut(&viewed) else { return };
        match watching.opened(now) {
            Some(Reopened::StartAgain) => acts.push(WatchAct::StartAgain(viewed).into()),
            Some(Reopened::Close) => acts.push(WatchAct::Close(viewed).into()),
            None => {}
        }
    }

    /// Holds each picture the window has shown nowhere for [`crate::watch::viewing::UNSEEN`].
    fn hold_unseen(&mut self, now: Instant, acts: &mut Vec<Act>) {
        for (viewed, watching) in &mut self.watching {
            if watching.hold_now(now) {
                acts.push(WatchAct::Hold(viewed.clone()).into());
            }
        }
    }

    /// The decoders say whether their streams wait for a key frame: each one due is asked for it.
    fn key_frames(&mut self, waiting: Vec<(Viewed, Waiting)>, now: Instant, acts: &mut Vec<Act>) {
        for (viewed, waiting) in waiting {
            let Some(watching) = self.watching.get_mut(&viewed) else { continue };
            if watching.key_frame_due(now) && watching.asked_key_frame(waiting.awaiting_key, waiting.behind, now) {
                acts.push(WatchAct::AskKeyFrame(viewed).into());
            }
        }
    }

    /// A picture's first frame was decoded: a screen's card is reported then, not at the next look.
    fn first_picture(&mut self, viewed: Viewed, acts: &mut Vec<Act>) {
        let Some(watching) = self.watching.get_mut(&viewed) else { return };
        watching.report_first();
        if viewed.1 == Video::Screen {
            acts.push(WatchAct::FirstCard(viewed).into());
        }
    }

    /// Brings what this call takes a step toward the room while the lane is free: what needs no wait is done at
    /// once, and a take or a watch is started on the lane and told to the room as asked.
    fn advance(&mut self, now: Instant, acts: &mut Vec<Act>) {
        while self.lane.is_idle() {
            let Some(action) = lane::next(&self.wanted, &self.tried, now) else { return };
            match action {
                Action::Forget((user_id, kind)) => {
                    self.tried.remove(&(user_id.clone(), kind));
                    acts.push(WatchAct::Forget(user_id, kind).into());
                }
                Action::Drop(stream) => {
                    self.tried.remove(&stream);
                }
                Action::Unwatch(viewed) => {
                    self.tried.remove(&(viewed.0.clone(), viewed.1.kind()));
                    if self.watching.remove(&viewed).is_some() {
                        self.lane.start(now, Doing::LettingGo(viewed.1));
                        acts.push(WatchAct::LetGo(viewed).into());
                    } else {
                        acts.push(WatchAct::Forget(viewed.0, viewed.1.kind()).into());
                    }
                }
                Action::Take(stream, began) => {
                    self.asked(&stream, began, acts);
                    self.lane.start(now, Doing::Taking(stream.1));
                    acts.push(WatchAct::Take(stream, began).into());
                }
                Action::Watch(viewed, began) => {
                    if !self.shows {
                        self.tried.insert((viewed.0, viewed.1.kind()), Attempt::Refused(began, Retry::Never));
                        continue;
                    }
                    self.asked(&(viewed.0.clone(), viewed.1.kind()), began, acts);
                    let paused = self.paused.contains(&viewed);
                    self.watched += 1;
                    self.lane.start(now, Doing::Showing(viewed.1));
                    acts.push(WatchAct::Watch { viewed, began, paused, number: self.watched }.into());
                }
            }
        }
    }

    /// A take of `stream` starts: the room is told it is asked for, and an operator which try it is when it was
    /// refused before.
    fn asked(&self, stream: &Stream, began: Began, acts: &mut Vec<Act>) {
        let refused = lane::refusals(&self.tried, stream, began);
        if refused > 0 {
            let (user_id, kind) = (stream.0.as_str(), stream.1);
            let said = format!("Taking {user_id}'s {kind:?} again, try {}.", refused.saturating_add(1));
            acts.push(Act::Report(Report::Said(said)));
        }
        taking(&stream.0, stream.1, Take::Asked, acts);
    }

    /// `stream` was refused at `now`: when it is tried again, and the room is told it stands refused.
    fn refused(&mut self, stream: Stream, began: Began, refusal: Refusal, now: Instant, acts: &mut Vec<Act>) {
        let retry = Retry::after(lane::refusals(&self.tried, &stream, began), refusal, now, self.jitter);
        taking(&stream.0, stream.1, Take::Refused, acts);
        self.tried.insert(stream, Attempt::Refused(began, retry));
    }

    /// A step of the lane is done at `now`: what it took is kept and told to the room, and a refusal ends only that
    /// try at that stream and is said.
    fn landed(&mut self, landed: Landed, now: Instant, acts: &mut Vec<Act>) {
        self.lane.landed();
        match landed {
            Landed::Took((user_id, kind), began, took) => match took {
                Ok(()) => {
                    // The first negotiation of the receive connection sent its DTLS parameters.
                    self.unreached.asked(Which::Receive, now);
                    acts.push(Act::Report(Report::Said(format!("Hearing {}.", user_id.as_str()))));
                    if kind == TrackKind::ScreenAudio {
                        let sound = Watched::Sound { user_id: user_id.clone(), refused: None };
                        acts.push(Act::Report(Report::Watch(sound)));
                    }
                    taking(&user_id, kind, Take::Taken, acts);
                    self.tried.insert((user_id, kind), Attempt::Taken(began));
                }
                Err(refused) => {
                    acts.push(Act::Report(Report::Said(format!("{} not heard: {refused:?}", user_id.as_str()))));
                    let refusal = Refusal::of_take(&refused);
                    if let Some(failed) = share_sound_unheard(kind, refused) {
                        let sound = Watched::Sound { user_id: user_id.clone(), refused: Some(failed) };
                        acts.push(Act::Report(Report::Watch(sound)));
                    }
                    self.refused((user_id, kind), began, refusal, now, acts);
                }
            },
            Landed::Watched { viewed: (user_id, video), began, shown } => match shown {
                Ok(paused) => {
                    self.unreached.asked(Which::Receive, now);
                    let viewed = (user_id.clone(), video);
                    if paused {
                        acts.push(Act::Report(Report::Watch(Watched::Paused { user_id: user_id.clone(), video })));
                    }
                    self.watching.insert(viewed.clone(), Watching::new(paused, now));
                    self.see(&viewed, now, acts);
                    taking(&user_id, video.kind(), Take::Taken, acts);
                    self.tried.insert((user_id, video.kind()), Attempt::Taken(began));
                }
                Err(failed) => {
                    let refusal = Refusal::of_watch(&failed);
                    acts.push(Act::Report(Report::Watch(Watched::Failed { user_id: user_id.clone(), video, failed })));
                    self.refused((user_id, video.kind()), began, refusal, now, acts);
                }
            },
            Landed::Unwatched((user_id, video)) => {
                acts.push(Act::Report(Report::Watch(Watched::Ended { user_id, video })))
            }
            Landed::Mended(mended) => acts.push(Act::Report(Report::Said(restart_said("receive", mended)))),
        }
    }

    /// A step of the lane ended without an answer, and what the lane takes went with it: the call cannot go on.
    fn lane_lost(&mut self, acts: &mut Vec<Act>) {
        let Some(doing) = self.lane.lost() else { return };
        report::note!(warn, "a step of the lane ended without an answer", doing = doing);
        acts.push(Act::Report(Report::Said(format!("The call cannot go on: {doing} ended without an answer."))));
        acts.push(Act::End(Over::Fault));
    }
}
