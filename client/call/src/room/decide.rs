//! The call decides here, in one function of its state, an input and the time: what each thing the call hears
//! changes, and the acts that follow. It holds no connection, device, task or channel; the driver in the parent module
//! holds those, turns what it hears into an [`Input`] and carries out each [`Act`] in order. What is heard and done
//! falls in four families, each decided in a module of its own: the voice, this account's own pictures, what is
//! watched, and the connections.

mod own;
mod voice;
mod watch;
mod wire;

pub(super) use own::{OwnAct, OwnInput, Picture, ScreenSent, Turned};
pub(super) use voice::{Levels, Sounded, Synced, VoiceAct, VoiceInput};
pub(super) use watch::{Landed, WatchAct, WatchInput};
pub(super) use wire::{Answered, Beat, Joined, Overdue, Wire, WireAct, WireInput, stalled};

use super::lane::{Lane, Shown, Tried, Wanted};
use super::mend::Mending;
use super::place::{Hold, Place};
use super::unreached::Unreached;
use super::{Over, Report, Source};
use crate::bounded;
use crate::sound::{Looks, Sounds, Standings};
use crate::watch::viewing::Viewed;
use crate::watch::watching::Watching;
use choices::Picked;
use screens::{Jitter, Reading, Tally};
use std::collections::HashMap;
use std::time::Duration;
use terms::MemberId;
use tokio::time::Instant;

/// How the microphone stands. The track itself is the driver's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mic {
    Closed,
    /// Being opened as a task of its own, late at `due`.
    Opening {
        due: Instant,
        within: Duration,
    },
    Open,
}

/// What the last look at the connection saw and when, and what the looks so far kept.
#[derive(Default)]
struct Watch {
    tally: Tally,
    last: Option<(Reading, Instant)>,
}

/// What the call is started with.
pub(super) struct Setup {
    /// This account's id on the server.
    pub(super) me: Option<MemberId>,
    /// The channel the call is in.
    pub(super) channel_id: terms::ChannelId,
    /// What this device sends.
    pub(super) source: Source,
    /// The host's spread of a wait.
    pub(super) jitter: Jitter,
    /// Whether the pictures sent in the call are shown, and so taken.
    pub(super) shows: bool,
    /// Whether the audio devices can be looked at.
    pub(super) has_devices: bool,
    /// The state the place joined in.
    pub(super) start: choices::State,
    /// Whether an administrator holds this account muted already.
    pub(super) held: Hold,
    /// The microphone asked for.
    pub(super) microphone: Option<Picked>,
    /// The speaker asked for.
    pub(super) speaker: Option<Picked>,
    /// The screens the window shows nowhere.
    pub(super) unseen: Vec<Viewed>,
    /// The pictures the person paused.
    pub(super) paused: Vec<Viewed>,
    /// Where the server said its media was: the name or address and the port, for a call that cannot reach it.
    pub(super) voice_at: (String, u16),
}

/// What the call has decided so far: plain data, no connection, device, task, future or channel.
pub(super) struct State {
    me: Option<MemberId>,
    channel_id: terms::ChannelId,
    source: Source,
    /// The host's spread of the wait before a refused stream is tried again.
    jitter: Jitter,
    /// Whether the pictures sent in the call are shown, and so taken.
    shows: bool,
    /// Whether the audio devices can be looked at.
    has_devices: bool,
    wanted: Wanted,
    tried: Tried,
    lane: Lane,
    mic: Mic,
    /// What the person wants, what the server agreed to, and an administrator's hold.
    place: Place,
    sounds: Sounds,
    standings: Standings,
    /// The look at the sound devices.
    settling: Looks,
    /// The screens the window shows nowhere, as it last said.
    unseen: Vec<Viewed>,
    /// The pictures the person paused, as they last said.
    paused: Vec<Viewed>,
    /// Each screen shown in the call.
    watching: HashMap<Viewed, Watching>,
    /// How many screens this call has started watching: each one's health line is numbered by it.
    watched: u64,
    /// The send connection's ICE restarts while its route is off.
    send_mend: Mending,
    /// The receive connection's, whose restart is a step of the lane.
    receive_mend: Mending,
    /// Each connection's first connecting, timed from its first ask.
    unreached: Unreached,
    /// How each lane was last told to the room: the send lane's, then the receive lane's.
    lanes: Option<(screens::Lane, screens::Lane)>,
    /// A tone's loudness as the last look answered it.
    tone: Option<u8>,
    health: Watch,
}

/// Each thing the call's loop hears, by the family that decides it.
pub(super) enum Input {
    /// The voice: the person's asks, the place, the microphone and the devices.
    Voice(VoiceInput),
    /// This account's own screen and camera.
    Own(OwnInput),
    /// The lane and the screens watched.
    Watch(WatchInput),
    /// The connections, the server's events and the bounds of every step.
    Wire(WireInput),
}

/// Each thing the driver carries out, in order, by the family that does it.
pub(super) enum Act {
    /// Tell the host.
    Report(Report),
    /// The call is over.
    End(Over),
    /// The voice.
    Voice(VoiceAct),
    /// This account's own screen and camera.
    Own(OwnAct),
    /// The lane and the screens watched.
    Watch(WatchAct),
    /// The connections.
    Wire(WireAct),
}

/// What `input` changes in `state` at `now`, and the acts that follow, in order.
pub(super) fn decide(state: &mut State, input: Input, now: Instant) -> Vec<Act> {
    let mut acts = Vec::new();
    match input {
        Input::Voice(input) => voice::decide(state, input, now, &mut acts),
        Input::Own(input) => own::decide(state, input, &mut acts),
        Input::Watch(input) => watch::decide(state, input, now, &mut acts),
        Input::Wire(input) => wire::decide(state, input, now, &mut acts),
    }
    acts
}

impl State {
    /// A call as the join left it.
    pub(super) fn new(setup: Setup) -> Self {
        Self {
            wanted: Wanted::new(setup.me.clone(), if setup.shows { Shown::Pictures } else { Shown::Nothing }),
            me: setup.me,
            channel_id: setup.channel_id,
            source: setup.source,
            jitter: setup.jitter,
            shows: setup.shows,
            has_devices: setup.has_devices,
            tried: Tried::new(),
            lane: Lane::Idle,
            mic: Mic::Closed,
            place: Place::new(setup.start, setup.held),
            sounds: Sounds::asking(setup.microphone, setup.speaker),
            standings: Standings::default(),
            settling: Looks::default(),
            unseen: setup.unseen,
            paused: setup.paused,
            watching: HashMap::new(),
            watched: 0,
            send_mend: Mending::new(setup.jitter),
            receive_mend: Mending::new(setup.jitter),
            unreached: Unreached::new(setup.voice_at.0, setup.voice_at.1),
            lanes: None,
            tone: None,
            health: Watch::default(),
        }
    }

    /// When the earliest step under way is late: the microphone opening, the lane's step, the look at the devices.
    pub(super) fn due(&self) -> Option<Instant> {
        let opening = match self.mic {
            Mic::Opening { due, .. } => Some(due),
            Mic::Closed | Mic::Open => None,
        };
        bounded::earliest([opening, self.lane.due(), self.settling.due()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure;
    use crate::room::Wants;
    use crate::room::lane::{Began, Doing, Stream};
    use proto::chatkord_v1::{CloseReason, Event, ProducerJoined, TrackKind, VoiceClosed, event};
    use proto::video::Video;
    use screens::{End, RoomEvent, Subject};
    use webrtc::{DevicesRefused, Sounding, Standing};
    use words::Failure;

    const BOOTH: &str = "booth";
    const ME: &str = "me";
    const ADA: &str = "ada";
    const BEA: &str = "bea";

    fn member(id: &str) -> MemberId {
        MemberId::parse(id).expect("a member id")
    }

    fn call() -> State {
        State::new(Setup {
            me: Some(member(ME)),
            channel_id: terms::ChannelId::parse(BOOTH).expect("a channel id"),
            source: Source::Microphone,
            jitter: Jitter::from_bits(1 << 63),
            shows: true,
            has_devices: true,
            start: choices::State::default(),
            held: Hold::Free,
            microphone: None,
            speaker: None,
            unseen: Vec::new(),
            paused: Vec::new(),
            voice_at: ("voice.example.invalid".to_string(), 40_000),
        })
    }

    fn reports(acts: &[Act]) -> Vec<&Report> {
        acts.iter().filter_map(|act| if let Act::Report(report) = act { Some(report) } else { None }).collect()
    }

    fn asks(acts: &[Act]) -> Vec<choices::State> {
        acts.iter()
            .filter_map(|act| if let Act::Voice(VoiceAct::AskState(state)) = act { Some(*state) } else { None })
            .collect()
    }

    fn began(id: &str) -> Input {
        let started =
            ProducerJoined { channel_id: BOOTH.into(), user_id: id.into(), kind: i32::from(TrackKind::Microphone) };
        Input::Wire(WireInput::Event(Event { cursor: Vec::new(), kind: Some(event::Kind::ProducerJoined(started)) }))
    }

    fn taken(acts: &[Act]) -> Vec<(Stream, Began)> {
        acts.iter()
            .filter_map(|act| {
                if let Act::Watch(WatchAct::Take(stream, began)) = act { Some((stream.clone(), *began)) } else { None }
            })
            .collect()
    }

    /// A stream of Bo's screen the window showed nowhere for its time, whose hold was asked at `now`.
    fn asked_to_hold(state: &mut State, now: Instant) -> Viewed {
        let viewed = (member("bo"), Video::Screen);
        let mut watching = Watching::new(false, now);
        watching.seen(false, now);
        assert!(watching.hold_now(now + crate::watch::viewing::UNSEEN));
        state.watching.insert(viewed.clone(), watching);
        viewed
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_the_server_held_has_its_decoder_closed_and_one_shown_again_meanwhile_keeps_it() {
        let now = Instant::now();
        let later = now + crate::watch::viewing::UNSEEN;
        let mut state = call();
        let viewed = asked_to_hold(&mut state, now);
        let acts = decide(&mut state, Input::Watch(WatchInput::Held(viewed.clone(), true)), later);
        assert!(
            acts.iter().any(|act| matches!(act, Act::Watch(WatchAct::Close(closed)) if *closed == viewed)),
            "closed once held"
        );
        assert!(
            !acts
                .iter()
                .any(|act| matches!(act, Act::Watch(WatchAct::Resumed(_)) | Act::Watch(WatchAct::StartAgain(_))))
        );

        let mut state = call();
        let viewed = asked_to_hold(&mut state, now);
        state.watching.get_mut(&viewed).expect("watched").seen(true, later);
        let acts = decide(&mut state, Input::Watch(WatchInput::Held(viewed.clone(), true)), later);
        assert!(
            !acts.iter().any(|act| matches!(act, Act::Watch(WatchAct::Close(_)))),
            "shown again before the answer: kept open"
        );
        assert!(acts.iter().any(|act| matches!(act, Act::Watch(WatchAct::Resumed(resumed)) if *resumed == viewed)));

        let mut state = call();
        let viewed = asked_to_hold(&mut state, now);
        let acts = decide(&mut state, Input::Watch(WatchInput::Held(viewed, false)), later);
        assert!(
            !acts.iter().any(|act| matches!(act, Act::Watch(WatchAct::Close(_)))),
            "a hold the server refused closes nothing"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_mute_while_an_ask_is_in_flight_is_asked_again_after_the_answer() {
        let now = Instant::now();
        let mut state = call();
        let (mut earlier, mut wants) = (Wants::default(), Wants::default());
        wants.mute(true);
        let acts = decide(&mut state, Input::Voice(VoiceInput::Wants { was: earlier.clone(), is: wants.clone() }), now);
        let muted = choices::State { mic_muted: true, deafened: false };
        assert_eq!(asks(&acts), [muted], "asked at once");
        earlier = wants.clone();
        wants.deafen(true);
        let acts = decide(&mut state, Input::Voice(VoiceInput::Wants { was: earlier, is: wants }), now);
        assert!(asks(&acts).is_empty(), "one ask at a time");
        let both = choices::State { mic_muted: true, deafened: true };
        assert!(
            reports(&acts).contains(&&Report::Room(RoomEvent::State { user_id: member(ME), state: both })),
            "and shown as the person's at once"
        );
        let acts = decide(&mut state, Input::Voice(VoiceInput::Asked(Ok(()))), now);
        assert_eq!(asks(&acts), [both], "what changed meanwhile is asked once the first is answered");
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_microphone_mutes_the_place_and_reports_the_notice() {
        let now = Instant::now();
        let mut state = call();
        decide(&mut state, Input::Voice(VoiceInput::MicOpened(Ok(()))), now);
        let refused =
            Sounding { microphone: Standing::Refused(DevicesRefused::Unavailable), speaker: Standing::Closed };
        let acts = decide(
            &mut state,
            Input::Voice(VoiceInput::Sounded(Sounded { now: refused, alive: true, capturing: false })),
            now,
        );
        let said = words::failure::of(&failure::devices(&DevicesRefused::Unavailable));
        assert!(reports(&acts).contains(&&Report::Notice(Subject::Microphone, said)), "the person is told");
        assert!(acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::MuteMic))), "the track sends silence");
        assert!(acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::SyncMic { silenced: true }))));
        let acts = decide(&mut state, Input::Voice(VoiceInput::MicSynced(Synced::Set)), now);
        let muted = choices::State { mic_muted: true, deafened: false };
        assert_eq!(asks(&acts), [muted], "and the server is told it is muted");
    }

    fn held(held: bool) -> Input {
        let state = proto::chatkord_v1::MemberState {
            channel_id: BOOTH.into(),
            user_id: ME.into(),
            admin_muted: held,
            ..Default::default()
        };
        Input::Wire(WireInput::Event(Event { cursor: Vec::new(), kind: Some(event::Kind::MemberState(state)) }))
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_members_own_mute_is_asked_and_the_lift_sends_the_microphone_again_when_it_is_off() {
        let now = Instant::now();
        let mut state = call();
        decide(&mut state, Input::Voice(VoiceInput::MicOpened(Ok(()))), now);
        let acts = decide(&mut state, held(true), now);
        assert!(acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::MuteMic))), "silenced at once");
        let (mut earlier, mut wants) = (Wants::default(), Wants::default());
        wants.mute(true);
        let acts = decide(&mut state, Input::Voice(VoiceInput::Wants { was: earlier.clone(), is: wants.clone() }), now);
        assert!(
            acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::SyncMic { silenced: true }))),
            "the microphone synced silenced"
        );
        decide(&mut state, Input::Voice(VoiceInput::MicSynced(Synced::Set)), now);
        decide(&mut state, Input::Voice(VoiceInput::Asked(Ok(()))), now);
        earlier = wants.clone();
        wants.mute(false);
        decide(&mut state, Input::Voice(VoiceInput::Wants { was: earlier, is: wants }), now);
        let acts = decide(&mut state, Input::Voice(VoiceInput::MicSynced(Synced::Set)), now);
        assert_eq!(asks(&acts), [choices::State::default()], "the own unmute is asked while held");
        assert!(
            reports(&acts)
                .contains(&&Report::Room(RoomEvent::State { user_id: member(ME), state: Default::default() })),
            "shown as the person's own, held or not"
        );
        decide(&mut state, Input::Voice(VoiceInput::Asked(Ok(()))), now);
        let acts = decide(&mut state, held(false), now);
        assert!(acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::SyncMic { silenced: false }))), "heard again");
    }

    #[tokio::test(start_paused = true)]
    async fn an_event_that_closes_the_place_ends_the_call_with_its_reason() {
        let now = Instant::now();
        let mut state = call();
        let over = VoiceClosed { channel_id: BOOTH.into(), reason: CloseReason::TakenOut.into() };
        let event = Event { cursor: Vec::new(), kind: Some(event::Kind::VoiceClosed(over)) };
        let acts = decide(&mut state, Input::Wire(WireInput::Event(event)), now);
        assert!(
            acts.iter().any(|act| matches!(act, Act::End(Over::Closed(End::Closed(Some(CloseReason::TakenOut)))))),
            "the server's reason is kept"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_that_begins_is_taken_only_when_the_lane_is_idle() {
        let now = Instant::now();
        let mut state = call();
        decide(&mut state, began(ADA), now);
        let first = taken(&decide(&mut state, Input::Watch(WatchInput::Advance), now));
        assert_eq!(first.len(), 1, "taken at once");
        assert_eq!(first[0].0, (member(ADA), TrackKind::Microphone));
        decide(&mut state, began(BEA), now);
        assert!(
            taken(&decide(&mut state, Input::Watch(WatchInput::Advance), now)).is_empty(),
            "the lane is busy with the first"
        );
        let landed = Landed::Took(first[0].0.clone(), first[0].1, Ok(()));
        decide(&mut state, Input::Watch(WatchInput::Landed(landed)), now);
        let next = taken(&decide(&mut state, Input::Watch(WatchInput::Advance), now));
        assert_eq!(next.iter().map(|(stream, _)| stream.0.clone()).collect::<Vec<_>>(), [member(BEA)], "then the next");
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_of_the_lane_that_never_returns_is_said_by_name_at_its_bound_and_ends_the_call_as_a_stall() {
        let asked = Instant::now();
        let mut state = call();
        decide(&mut state, began(ADA), asked);
        decide(&mut state, Input::Watch(WatchInput::Advance), asked);
        let doing = Doing::Taking(TrackKind::Microphone);
        let early = decide(
            &mut state,
            Input::Wire(WireInput::Overdue(Overdue::default())),
            asked + doing.within() - Duration::from_secs(1),
        );
        assert!(early.is_empty(), "not before its bound");
        let acts = decide(&mut state, Input::Wire(WireInput::Overdue(Overdue::default())), asked + doing.within());
        let said = Report::Said(format!("The engine did not answer within {:?}: taking a voice.", doing.within()));
        assert_eq!(reports(&acts), [&said]);
        assert!(acts.iter().any(|act| matches!(act, Act::End(Over::Failed(Failure::EngineStalled)))));
        let later = decide(&mut state, Input::Wire(WireInput::Overdue(Overdue::default())), asked + doing.within() * 2);
        assert!(later.is_empty(), "once");
    }
}
