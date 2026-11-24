//! What the call decides about the connections: the server's events, the looks at the engine, the restarts of a route
//! and the bounds of every step.

use super::own::{OwnAct, Picture, ScreenSent, Turned};
use super::voice::VoiceAct;
use super::watch::WatchAct;
use super::{Act, Mic, State};
use crate::bounded;
use crate::room::lane::{Attempt, Doing, Retry, Stream};
use crate::room::place::Hold;
use crate::room::unreached::Which;
use crate::room::{Over, Report, Step, as_wanted, lane_of, restart_said, steps};
use crate::send::own::Screen;
use crate::send::own_camera::Camera;
use crate::send::sender::{Kind, Stalled};
use crate::sound;
use proto::chatkord_v1::Event;
use screens::{CameraSent, End, Health, MeasuredOn, Reading, RoomEvent, RoundTrip, Shared, Subject};
use std::fmt;
use std::time::Duration;
use terms::Ended;
use tokio::time::Instant;
use webrtc::{Connection, Reach, VoiceRefused};
use words::Failure;

/// What the connections and the clock said.
pub(in crate::room) enum WireInput {
    /// The person left, or the commands ended.
    Leave,
    /// The steps still out at their bounds, if any.
    Overdue(Overdue),
    /// A server event.
    Event(Event),
    /// The connection dropped events it could not hold.
    Behind,
    /// The connection ended.
    Ended(Ended),
    /// The look tick.
    Beat(Beat),
    /// The engine answered a look.
    Answered(Answered),
    /// The send connection's ICE restart came back.
    SendRestarted(Result<(), VoiceRefused>),
    /// The call was joined.
    Joined(Joined),
}

/// What the driver carries out for the connections.
#[derive(Clone, Copy)]
pub(in crate::room) enum WireAct {
    /// Ask the engine how what is sent and the connections are.
    AskLook,
    /// Start the send connection's ICE again.
    RestartSend,
    /// Start the receive connection's ICE again, as a step of the lane.
    MendReceive,
}

impl From<WireAct> for Act {
    fn from(act: WireAct) -> Self {
        Self::Wire(act)
    }
}

/// What `input` changes in `state` at `now`, and the acts that follow.
pub(super) fn decide(state: &mut State, input: WireInput, now: Instant, acts: &mut Vec<Act>) {
    match input {
        WireInput::Leave => acts.push(Act::End(Over::Left)),
        WireInput::Overdue(late) => state.overdue(late, now, acts),
        WireInput::Event(event) => state.heard(event, now, acts),
        WireInput::Behind => acts.push(Act::Report(Report::Behind)),
        WireInput::Ended(ended) => acts.push(Act::End(Over::Link(ended))),
        WireInput::Beat(beat) => state.beat(&beat, now, acts),
        WireInput::Answered(answered) => state.looked(&answered, now, acts),
        WireInput::SendRestarted(restarted) => {
            acts.push(Act::Report(Report::Said(restart_said("send", restarted))));
        }
        WireInput::Joined(joined) => state.joined(joined, now, acts),
    }
}

/// The connections as the engine has them at the moment of an input, and what is on the send connection.
#[derive(Clone, Copy, Debug)]
pub(in crate::room) struct Wire {
    /// The send connection's route.
    pub(in crate::room) send: Reach,
    /// The receive connection's route.
    pub(in crate::room) receive: Reach,
    /// Whether this account's screen is on the send connection.
    pub(in crate::room) screen: bool,
    /// Whether this account's camera is on the send connection.
    pub(in crate::room) camera: bool,
}

/// The call's look at itself, every [`screens::LOOK_EVERY`].
pub(in crate::room) struct Beat {
    pub(in crate::room) wire: Wire,
    /// Whether the voice chain runs, where there are devices to look at.
    pub(in crate::room) chain: Option<bool>,
    /// Whether the send connection's restart is out.
    pub(in crate::room) restarting: bool,
    /// Whether the microphone is held and open.
    pub(in crate::room) capturing: bool,
}

/// The engine's answer to a look.
pub(in crate::room) struct Answered {
    /// A tone's loudness: none outside a tone, then whether the engine had a reading.
    pub(in crate::room) tone: Option<Option<u8>>,
    /// Both connections', where they answered.
    pub(in crate::room) connection: Option<Connection>,
    /// The link's own round trip, for a call with no media on its route.
    pub(in crate::room) heartbeat: Option<Duration>,
}

/// The call joined, as the server gave it.
pub(in crate::room) struct Joined {
    pub(in crate::room) room: RoomEvent,
    pub(in crate::room) streams: Vec<Stream>,
    pub(in crate::room) wire: Wire,
}

/// The steps of this account's own pictures still out at their bounds, as each sender let them go.
#[derive(Default)]
pub(in crate::room) struct Overdue {
    pub(in crate::room) screen: Option<(Stalled, Duration, Turned<ScreenSent, Shared>)>,
    pub(in crate::room) camera: Option<(Stalled, Duration, Turned<Picture, CameraSent>)>,
}

/// A step that did not come back within its bound, said for an operator and in the log.
pub(in crate::room) fn stalled(what: impl fmt::Display, within: Duration) -> Report {
    report::note!(warn, "a step did not come back within its bound and is let go", within_ms = report::millis(within));
    Report::Said(bounded::stalled_said(what, within))
}

impl State {
    /// What one event asks of the call: the lane catches up with what the room wants at the next advance. It may end
    /// the call.
    fn heard(&mut self, event: Event, now: Instant, acts: &mut Vec<Act>) {
        for step in steps(&self.channel_id, self.me.as_ref(), event) {
            match step {
                Step::Room(room) => {
                    if let RoomEvent::Held { user_id, held } = &room
                        && self.me.as_ref() == Some(user_id)
                    {
                        self.hold(if *held { Hold::Held } else { Hold::Free }, now, acts);
                    }
                    acts.push(Act::Report(Report::Room(as_wanted(room, self.me.as_ref(), self.place.want()))));
                }
                Step::Began(stream) => self.wanted.began(stream),
                Step::Ended(stream) => self.wanted.ended(&stream),
                Step::Whole(room, streams) => {
                    acts.push(Act::Report(Report::Room(as_wanted(room, self.me.as_ref(), self.place.want()))));
                    self.wanted.whole(streams, &mut self.tried);
                }
                Step::Heard(event) => acts.push(Act::Report(Report::Heard(event))),
                Step::Closed(end) => {
                    let reason = match &end {
                        End::Closed(reason) => *reason,
                        End::Link(_) | End::Unreached(_) | End::Failed(_) | End::FellBehind => None,
                    };
                    acts.push(Act::End(match self.unreached.closed(reason) {
                        Some(unreached) => Over::Closed(End::Failed(unreached)),
                        None => Over::Closed(end),
                    }));
                    return;
                }
                Step::Denied(video) => acts.push(OwnAct::Denied(video).into()),
                Step::Watched(video, watched) => {
                    acts.push(OwnAct::OwnWatched(video, watched).into());
                    acts.push(Act::Report(Report::OwnWatched { video, watched: screens::OwnWatch::of(watched) }));
                }
            }
        }
    }

    /// Every [`screens::LOOK_EVERY`]: a connection whose route stays off has its ICE started again, and the person's
    /// own tile says so while it mends; streams refused meanwhile are tried again once the receive route is back. The
    /// voice chain is looked at, and the engine asked how the screens and connections are.
    fn beat(&mut self, beat: &Beat, at: Instant, acts: &mut Vec<Act>) {
        let now = at.into_std();
        let wire = &beat.wire;
        if (wire.send == Reach::Up || !beat.restarting) && self.send_mend.due(wire.send, now) {
            acts.push(WireAct::RestartSend.into());
        }
        let was_mending = self.receive_mend.mending();
        if wire.receive == Reach::Up {
            self.receive_mend.due(wire.receive, now);
            if was_mending {
                // What was refused while the route was off is tried again at once; what cannot pass still stands.
                self.tried
                    .retain(|_, attempt| matches!(attempt, Attempt::Taken(_) | Attempt::Refused(_, Retry::Never)));
            }
        } else if self.lane.is_idle() && self.receive_mend.due(wire.receive, now) {
            self.lane.start(at, Doing::Mending);
            acts.push(WireAct::MendReceive.into());
        }
        self.tell_lanes(wire, at, acts);
        if let Some(alive) = beat.chain
            && let Some(told) = self.standings.stopped(alive)
        {
            self.told(told, beat.capturing, acts);
        }
        acts.push(WatchAct::LookAtScreens.into());
        acts.push(WireAct::AskLook.into());
    }

    /// Tells the room how each lane is, whenever either changed. The send connection is made with the first thing
    /// sent over it, so with nothing asked of it (a place that joined muted, a listener) it has nothing to start
    /// and counts as up.
    pub(super) fn tell_lanes(&mut self, wire: &Wire, now: Instant, acts: &mut Vec<Act>) {
        // The send connection's DTLS parameters go with the first thing produced over it, so it is asked then.
        if self.mic == Mic::Open || wire.screen || wire.camera {
            self.unreached.asked(Which::Send, now);
        }
        if let Some(unreached) = self.unreached.read(wire.send, wire.receive, now) {
            acts.push(Act::End(Over::Failed(unreached)));
        }
        let sending = self.mic != Mic::Closed || wire.screen || wire.camera;
        let send = match lane_of(wire.send, self.send_mend.mending()) {
            screens::Lane::Starting if !sending => screens::Lane::Up,
            lane => lane,
        };
        let now = (send, lane_of(wire.receive, self.receive_mend.mending()));
        if self.lanes != Some(now) {
            self.lanes = Some(now);
            acts.push(Act::Report(Report::Room(RoomEvent::Lanes { send: now.0, receive: now.1 })));
        }
    }

    /// The engine answered a look: a tone's loudness is kept, and how the connection is said.
    fn looked(&mut self, answered: &Answered, at: Instant, acts: &mut Vec<Act>) {
        // A look that asked of no tone leaves none to report: its microphone is not open.
        self.tone = answered.tone.flatten();
        if let Some(now) = &answered.connection {
            self.health(now, answered.heartbeat, at, acts);
        }
    }

    /// Reports how the connection is, from the engine's counters `now`. A call with no media on its route has no round
    /// trip of its own, so the link's heartbeat gives one.
    fn health(&mut self, now: &Connection, heartbeat: Option<Duration>, at: Instant, acts: &mut Vec<Act>) {
        let reading = Reading {
            round_trip: now
                .round_trip
                .map(|took| RoundTrip { took, on: MeasuredOn::Route })
                .or_else(|| heartbeat.map(|took| RoundTrip { took, on: MeasuredOn::Heartbeat })),
            heard: now.heard,
            lost: now.lost,
            sent: now.sent,
            sent_bytes: now.sent_bytes,
            received: now.received,
            received_bytes: now.received_bytes,
            up_bytes: now.up_bytes,
            down_bytes: now.down_bytes,
        };
        let elapsed = self.health.last.map_or(Duration::ZERO, |(_, then)| at.duration_since(then));
        let earlier = self.health.last.as_ref().map(|(reading, _)| reading);
        let health = Health::between(earlier, &reading, elapsed, &mut self.health.tally);
        acts.push(Act::Report(Report::Connection(health)));
        self.health.last = Some((reading, at));
    }

    /// The call is joined: the room is told whole, the lane takes what is there from the loop on, and the source is
    /// opened once the place is heard.
    fn joined(&mut self, joined: Joined, now: Instant, acts: &mut Vec<Act>) {
        acts.push(Act::Report(Report::Room(joined.room)));
        self.wanted.whole(joined.streams, &mut self.tried);
        self.open_mic(now, acts);
        self.tell_lanes(&joined.wire, now, acts);
    }

    /// Lets go of each step still out at its bound at `now`, saying which, and telling the person where they asked
    /// for what it was to do. A step of the lane that never came back ends the call: it holds what the call takes,
    /// so nothing more can be taken.
    fn overdue(&mut self, late: Overdue, now: Instant, acts: &mut Vec<Act>) {
        if let Mic::Opening { due, within } = self.mic
            && now >= due
        {
            self.mic = Mic::Closed;
            acts.push(VoiceAct::DropOpening.into());
            acts.push(Act::Report(stalled("starting to send the microphone", within)));
            acts.push(Act::Report(Report::Notice(Subject::Microphone, words::failure::of(&Failure::EngineStalled))));
        }
        if let Some((step, within, turn)) = late.screen {
            acts.push(Act::Report(stalled(format_args!("{step} {}", Screen::WHAT), within)));
            self.screen_turned(turn, acts);
        }
        if let Some((step, within, turn)) = late.camera {
            acts.push(Act::Report(stalled(format_args!("{step} {}", Camera::WHAT), within)));
            self.camera_turned(turn, acts);
        }
        if self.settling.stalled(now) {
            // Whatever was asked meanwhile would wait on the same sound server: the next change asks anew.
            self.settling.again();
            acts.push(VoiceAct::DropGlance.into());
            acts.push(Act::Report(stalled("listing the sound devices", sound::LISTING_WITHIN)));
            acts.push(Act::Report(Report::Notice(Subject::Devices, words::failure::of(&Failure::EngineStalled))));
        }
        if let Some((doing, within)) = self.lane.stalled(now) {
            acts.push(Act::Report(stalled(doing, within)));
            acts.push(WatchAct::DropStep.into());
            acts.push(Act::End(Over::Failed(Failure::EngineStalled)));
        }
    }
}
