//! Joining a voice channel and being in it: the room said as the model's own events and what the engine did as lines
//! for an operator, the person's commands carried out, and the voice place always given back when this side ends it.

mod decide;
pub(crate) mod lane;
mod look;
mod mend;
mod place;
mod unreached;
mod wants;

pub use crate::broadcast::SoundAsked;
pub use wants::Wants;

use crate::bounded::{self, Panicked, Task};
use crate::failure;
use crate::screen::Pick;
use crate::send::own::Screen;
use crate::send::own_camera::Camera;
use crate::send::sender::{Sender, Turn};
use crate::sound::{self, Direction, Glance, Looking, Sounds, Told};
use crate::watch::viewing::{self, Viewed, Viewing};
use choices::{Audio, ShareQuality, State};
use decide::{
    Act, Answered, Beat, Input, Joined, Landed, Levels, Overdue, OwnAct, OwnInput, Picture, ScreenSent, Sounded,
    Synced, Turned, VoiceAct, VoiceInput, WatchAct, WatchInput, Wire, WireAct, WireInput,
};
use lane::{Began, Done, Flight, Steps, Stream};
use link::{Events, Link};
use look::{Look, Looked};
use picture::{Painting, Slot};
use place::Hold;
use proto::chatkord_v1::{
    ChannelsList, CloseReason, Event, MemberSummary, StreamRef, TrackKind, VoiceJoin, VoiceLeave, VoiceSetState,
    VoiceState, event,
};
use proto::video::Video;
use screens::{CameraSent, End, Health, LOOK_EVERY, Person, RoomEvent, Shared, Sound, Subject, Track, Watched};
use terms::{ChannelId, Ended, Heard, LinkError, MemberId};
use words::{Failure, Sentence};

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use webrtc::{Devices, Engine, Hearing, Reach, Sounding, Speaking, Voice, VoiceRefused, Watching};

/// What the loop reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// What the room is, for the model.
    Room(RoomEvent),
    /// What the engine did, for an operator: a line of text.
    Said(String),
    /// The call was left as asked, or why leaving failed.
    Left(Result<(), Sentence>),
    /// What a person is told because the call could not do what they asked, about its subject: a microphone that would
    /// not start, a device that is gone. One short sentence.
    Notice(Subject, Sentence),
    /// A device of `Subject` that was said not to answer opened, with nothing to say of it: what was marked of it
    /// is cleared, with no toast.
    Answered(Subject),
    /// An event that is not the room's, for the model to read: the account's roles, the server's channels.
    Heard(Event),
    /// The connection dropped events it could not hold: the room follows whole, and the rest is the model's to ask
    /// for again.
    Behind,
    /// How the connection is, every [`LOOK_EVERY`].
    Connection(Health),
    /// The microphones and speakers there are and which the call uses, when the call starts and whenever a device
    /// comes, goes or is chosen.
    Sound(Sound),
    /// How this account's own screen share is going.
    Share(Shared),
    /// How a screen shared in the call is going.
    Watch(Watched),
    /// How this account's own camera is going.
    Camera(CameraSent),
    /// Whether anyone takes this account's own screen or camera now, told by the server when someone starts or stops
    /// and whole when the call is joined. An unwatched one rests, and its card says so instead of a rate of zero.
    OwnWatched {
        /// Which of this account's own pictures.
        video: Video,
        /// Whether anyone takes it.
        watched: screens::OwnWatch,
    },
}

/// What a call sends of this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Only listens.
    Nothing,
    /// The chosen microphone.
    Microphone,
    /// A generated tone, for a device or a test with no microphone.
    Tone,
}

/// Where each picture sent in a call, a screen or a camera, is put for the host to paint, by its sender's id and
/// kind: the host makes the slots.
#[derive(Clone)]
pub struct Screens(Arc<SlotOf>);

/// What makes the slot of one sender's picture of one kind.
type SlotOf = dyn Fn(&MemberId, Video) -> Slot + Send + Sync;

impl Screens {
    /// The slots `slot_of` makes, by sender's id and kind.
    pub fn new(slot_of: impl Fn(&MemberId, Video) -> Slot + Send + Sync + 'static) -> Self {
        Self(Arc::new(slot_of))
    }
}

impl fmt::Debug for Screens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Screens")
    }
}

/// The GPU the host paints with now, read each time a decoder or a send starts, so one the host made again after
/// its device was lost reaches the next.
#[derive(Clone)]
pub struct Paints(Arc<dyn Fn() -> Painting + Send + Sync>);

impl Paints {
    /// The painting `now` answers, read at each start.
    pub fn new(now: impl Fn() -> Painting + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    pub(crate) fn now(&self) -> Painting {
        (self.0)()
    }
}

impl fmt::Debug for Paints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Paints")
    }
}

/// What a call is joined with.
#[derive(Clone, Debug)]
pub struct Join {
    /// The voice channel to join.
    pub channel_id: ChannelId,
    /// This account's id on the server (`LoginAck.user_id`): the server does not tell a person of their own change.
    pub me: Option<MemberId>,
    /// What this device sends.
    pub source: Source,
    /// The microphone to use, none for the system's default; one not here is said and the default used.
    pub microphone: Option<choices::Picked>,
    /// The speaker to use, as `microphone` is.
    pub speaker: Option<choices::Picked>,
    /// How the microphone is processed.
    pub processing: choices::VoiceProcessing,
    /// How often the loudness of each person heard is reported.
    pub level_every: Duration,
    /// Where the pictures of each screen shared arrive, for the host to paint; none where nothing shows them, and then
    /// no screen is taken.
    pub screens: Option<Screens>,
    /// Where this account's own screen is lent while it is sent, for its card to paint; none where nothing shows it.
    pub own_screen: Option<Slot>,
    /// Where this account's own camera's pictures are put while it is on, for its card to paint.
    pub own_camera: Option<Slot>,
    /// The GPU the host paints with, for a screen's decoder; none where no window is, and then it is looked up.
    pub painting: Option<Paints>,
    /// The host's spread of a wait, so the clients of one server that lost their routes together do not ask together.
    pub jitter: screens::Jitter,
    /// The encoder survey the host keeps ([`crate::Engines::encoders`]), which every share and camera is encoded from.
    pub encoders: Arc<media::Survey>,
    /// The watch on the engine's voice chain the host keeps ([`crate::Engines::chain_watch`]), which this call
    /// looks at with whoever else does.
    pub chain: crate::ChainWatch,
}

/// What the host asks of a call that is running, each an act or a setting carried out once, in the order asked; what
/// is a state the person wants is [`Wants`].
#[derive(Debug)]
pub enum Command {
    /// Use this microphone, or the system's default for none.
    Microphone(Option<choices::Picked>),
    /// Use this speaker, or the system's default for none.
    Speaker(Option<choices::Picked>),
    /// Process the microphone so.
    Processing(choices::VoiceProcessing),
    /// The system's sound devices came or went: look again.
    DevicesChanged,
    /// Send the screen the person picked, as `ShareQuality` says, with the sound `SoundAsked` names. A share already
    /// running or beginning is kept, and this one is let go.
    Share(Pick, ShareQuality, SoundAsked),
    /// Stop sending the screen, or the one still being begun.
    StopShare,
    /// Turn on this camera, or turn off the one on (none); one already on stays as it is.
    Camera(Option<crate::camera::Pick>),
    /// Send this application's sound with the screen, picked for a window whose owner could not be found.
    ShareSoundOf(screens::SoundApp),
    /// Report the loudness of who is heard while the window shows (true), or not at all while it does not: a hidden
    /// window is told none. A call starts reporting it.
    Levels(bool),
}

/// Leaves the call it was given to: apart from the commands, so no full queue can drop it. A leave asked before the
/// loop waits for it is kept, and a closed command channel means the same.
#[derive(Clone, Debug, Default)]
pub struct Leaving(Arc<Notify>);

impl Leaving {
    /// Leaves the call, whenever its loop comes to wait for it.
    pub fn leave(&self) {
        self.0.notify_one();
    }
}

/// What a screen's card says of its sound that could not be taken: only that the server cannot carry it now, which is
/// the sharer's or the server's state; any other refusal of a sound is the log's.
fn share_sound_unheard(kind: TrackKind, refused: VoiceRefused) -> Option<Failure> {
    (kind == TrackKind::ScreenAudio && failure::watch(refused) == Failure::ScreenNotCarried)
        .then_some(Failure::ShareSoundNotCarried)
}

/// The track a member's sound arrives on.
fn heard_on(audio: Audio) -> TrackKind {
    match audio {
        Audio::Voice => TrackKind::Microphone,
        Audio::Share => TrackKind::ScreenAudio,
    }
}

fn track(kind: i32) -> Option<Track> {
    TrackKind::try_from(kind).ok().and_then(track_of)
}

fn track_of(kind: TrackKind) -> Option<Track> {
    match kind {
        TrackKind::Microphone => Some(Track::Microphone),
        TrackKind::ScreenVideo => Some(Track::ScreenVideo),
        TrackKind::ScreenAudio => Some(Track::ScreenAudio),
        TrackKind::Camera => Some(Track::Camera),
        TrackKind::Unspecified => None,
    }
}

/// A member of the room as the wire states them; one with an empty id is a server not keeping the protocol, and is
/// passed over.
fn person(member: &MemberSummary) -> Option<Person> {
    Some(Person {
        user_id: MemberId::parse(&member.user_id)?,
        name: member.display_name.clone(),
        colour: screens::wire_colour(member.colour.as_deref()),
        state: State { mic_muted: member.mic_muted, deafened: member.deafened },
        held: member.admin_muted,
    })
}

/// The room whole, as a join or a snapshot gives it.
fn joined(members: &[MemberSummary], streams: &[StreamRef]) -> RoomEvent {
    RoomEvent::Joined {
        members: members.iter().filter_map(person).collect(),
        streams: streams
            .iter()
            .filter_map(|stream| Some((MemberId::parse(&stream.user_id)?, track(stream.kind)?)))
            .collect(),
    }
}

/// `room` with this account's own state as the person wants it: the server's word of it may be behind a press.
fn as_wanted(mut room: RoomEvent, me: Option<&MemberId>, want: State) -> RoomEvent {
    match &mut room {
        RoomEvent::State { user_id, state } if me == Some(user_id) => *state = want,
        RoomEvent::Joined { members, .. } => {
            members.iter_mut().filter(|person| me == Some(&person.user_id)).for_each(|own| own.state = want);
        }
        _ => {}
    }
    room
}

/// Why a call ended other than by being left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// The connection ended, as it ended.
    Disconnected(Ended),
    /// The join or the room ended it.
    Ended(End),
    /// A step of the call panicked: the call is over, its place given back, and its asker closes the link and
    /// connects again, as [`Failure::CallFaulted`] says.
    Fault,
}

impl Stopped {
    /// What a person reads of it.
    pub fn sentence(&self) -> Sentence {
        match self {
            Stopped::Disconnected(ended) => words::failure::ended(ended),
            Stopped::Ended(end) => end.sentence(),
            Stopped::Fault => words::failure::of(&Failure::CallFaulted),
        }
    }
}

impl From<LinkError> for Stopped {
    fn from(why: LinkError) -> Self {
        match why {
            LinkError::Closed(ended) => Stopped::Disconnected(ended),
            other => Stopped::Ended(End::Unreached(other)),
        }
    }
}

/// How long a join may take: the server opens the member's transports before it answers.
const VOICE_JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a call's ending may take, the server told and everything it held on this device let go: some 25 times
/// what letting go of eight watched screens took on Machine A (120 ms). Past it the call is over all the same, and
/// what had not ended ends behind it.
pub const LEAVE_WITHIN: Duration = Duration::from_secs(3);

/// Joins `channel_id`, with the place starting in `start`, and returns what the server says of it.
pub async fn join_as(link: &Link, channel_id: &ChannelId, start: State) -> Result<VoiceState, LinkError> {
    let join =
        VoiceJoin { channel_id: channel_id.as_str().to_owned(), mic_muted: start.mic_muted, deafened: start.deafened };
    {
        let state = link.ask_within(join, VOICE_JOIN_TIMEOUT).await?;
        Ok(state)
    }
}

/// Joins `channel_id` neither muted nor deafened.
pub async fn join(link: &Link, channel_id: &ChannelId) -> Result<VoiceState, LinkError> {
    join_as(link, channel_id, State::default()).await
}

/// Leaves the voice channel this connection is in.
pub async fn leave(link: &Link) -> Result<(), LinkError> {
    link.ask(VoiceLeave {}).await.map(|_| ())
}

/// The channels of the server the link is signed in to, as the list shows them.
pub async fn channels(link: &Link) -> Result<screens::ChannelList, LinkError> {
    let summary = link.ask(ChannelsList {}).await?;
    Ok(screens::listed(&summary))
}

/// What one server event asks of the loop.
#[derive(Debug, PartialEq)]
enum Step {
    Room(RoomEvent),
    /// This member's stream began: it is wanted from now.
    Began(Stream),
    Ended(Stream),
    /// The room whole, and every stream in it.
    Whole(RoomEvent, Vec<Stream>),
    /// An event that is not the room's.
    Heard(Event),
    /// The server ended this place.
    Closed(End),
    /// The server ended this account's own picture of that kind: its roles no longer allow one.
    Denied(Video),
    /// Whether anyone takes this account's own picture of that kind now.
    Watched(Video, bool),
}

/// What the server answered of a hold or a start of a watched stream.
enum Held {
    /// The ask to hold the stream of this viewing, and whether the server held it.
    Asked(Viewed, bool),
    /// A held stream was asked to start again.
    Started,
}

/// The kinds a stream is.
fn kind_of(kind: i32) -> Option<TrackKind> {
    match TrackKind::try_from(kind) {
        Ok(kind @ (TrackKind::Microphone | TrackKind::ScreenAudio | TrackKind::ScreenVideo | TrackKind::Camera)) => {
            Some(kind)
        }
        Ok(TrackKind::Unspecified) | Err(_) => None,
    }
}

fn stream_of(stream: &StreamRef) -> Option<Stream> {
    Some((MemberId::parse(&stream.user_id)?, kind_of(stream.kind)?))
}

/// How a close reason ends a call.
fn closed(reason: i32) -> End {
    End::Closed(CloseReason::try_from(reason).ok())
}

/// What `event` asks of a call in `channel_id` by the account `me`. What names another channel is not this call's.
fn steps(channel_id: &ChannelId, me: Option<&MemberId>, event: Event) -> Vec<Step> {
    let channel_id = channel_id.as_str();
    match event.kind {
        Some(event::Kind::MemberJoined(who)) if who.channel_id == channel_id => {
            let Some(user_id) = MemberId::parse(&who.user_id) else { return Vec::new() };
            vec![Step::Room(RoomEvent::MemberJoined {
                user_id,
                name: who.display_name,
                colour: screens::wire_colour(who.colour.as_deref()),
            })]
        }
        Some(event::Kind::MemberLeft(who)) if who.channel_id == channel_id => MemberId::parse(&who.user_id)
            .map(|user_id| Step::Room(RoomEvent::MemberLeft { user_id }))
            .into_iter()
            .collect(),
        Some(event::Kind::ProducerJoined(started)) if started.channel_id == channel_id => {
            let Some(user_id) = MemberId::parse(&started.user_id) else { return Vec::new() };
            let mut steps = Vec::new();
            if let Some(track) = track(started.kind) {
                steps.push(Step::Room(RoomEvent::Started { user_id: user_id.clone(), track }));
            }
            if let Some(kind) = kind_of(started.kind) {
                steps.push(Step::Began((user_id, kind)));
            }
            steps
        }
        // The server tells this account of its own stream only when its roles ended its share.
        Some(event::Kind::ProducerClosed(ended))
            if ended.channel_id == channel_id && me.is_some_and(|me| me.as_str() == ended.user_id) =>
        {
            let denied = ended.reason == i32::from(CloseReason::NotAllowed);
            let video = TrackKind::try_from(ended.kind).ok().and_then(Video::of);
            video.filter(|_| denied).map(Step::Denied).into_iter().collect()
        }
        Some(event::Kind::ProducerClosed(ended)) if ended.channel_id == channel_id => {
            let Some(user_id) = MemberId::parse(&ended.user_id) else { return Vec::new() };
            let mut steps = Vec::new();
            if let Some(track) = track(ended.kind) {
                steps.push(Step::Room(RoomEvent::Stopped { user_id: user_id.clone(), track }));
            }
            if let Some(kind) = kind_of(ended.kind) {
                steps.push(Step::Ended((user_id, kind)));
            }
            steps
        }
        Some(event::Kind::RoomSnapshot(whole)) if whole.channel_id == channel_id => {
            let streams = whole.streams.iter().filter_map(stream_of).collect();
            let mut steps = vec![Step::Whole(joined(&whole.members, &whole.streams), streams)];
            for video in [Video::Screen, Video::Camera] {
                steps.push(Step::Watched(video, whole.watched.contains(&i32::from(video.kind()))));
            }
            steps
        }
        // Told to this account alone, of its own tracks, in the call it is in.
        Some(event::Kind::ShareWatched(told)) => match TrackKind::try_from(told.kind).ok().and_then(Video::of) {
            Some(video) => vec![Step::Watched(video, told.watched)],
            None => Vec::new(),
        },
        Some(event::Kind::MemberState(state)) if state.channel_id == channel_id => {
            let state_of = State { mic_muted: state.mic_muted, deafened: state.deafened };
            let Some(user_id) = MemberId::parse(&state.user_id) else { return Vec::new() };
            let held = RoomEvent::Held { user_id: user_id.clone(), held: state.admin_muted };
            vec![Step::Room(RoomEvent::State { user_id, state: state_of }), Step::Room(held)]
        }
        Some(event::Kind::VoiceClosed(over)) if over.channel_id == channel_id => {
            vec![Step::Closed(closed(over.reason))]
        }
        kind @ Some(
            event::Kind::StandingChanged(_)
            | event::Kind::ChannelAdded(_)
            | event::Kind::ChannelRenamed(_)
            | event::Kind::ChannelRemoved(_)
            | event::Kind::ChannelStanding(_)
            | event::Kind::VoiceOccupancy(_)
            | event::Kind::MemberRenamed(_)
            | event::Kind::MemberRecoloured(_)
            | event::Kind::MemberPictured(_)
            | event::Kind::ServerChanged(_)
            | event::Kind::MessageChanged(_)
            | event::Kind::MessagesWithdrawn(_)
            | event::Kind::PersonArrived(_)
            | event::Kind::PersonLeft(_)
            | event::Kind::Rewound(_)
            | event::Kind::RolesChanged(_)
            | event::Kind::CategoryAdded(_)
            | event::Kind::CategoryRenamed(_)
            | event::Kind::CategoryRemoved(_)
            | event::Kind::CategoryMoved(_)
            | event::Kind::ChannelMoved(_)
            | event::Kind::MemberRolesChanged(_)
            | event::Kind::PresenceChanged(_)
            | event::Kind::ChannelAccessChanged(_)
            | event::Kind::ReactionsOfYours(_)
            | event::Kind::TypingNow(_)
            | event::Kind::EmojiAdded(_)
            | event::Kind::EmojiRenamed(_)
            | event::Kind::EmojiRemoved(_)
            | event::Kind::NotifyChanged(_)
            | event::Kind::ConversationOpened(_)
            | event::Kind::TimelineStanding(_)
            | event::Kind::DeviceInUse(_)
            | event::Kind::StatusChosen(_),
        ) => vec![Step::Heard(Event { cursor: Vec::new(), kind })],
        Some(
            event::Kind::MemberJoined(_)
            | event::Kind::MemberLeft(_)
            | event::Kind::ProducerJoined(_)
            | event::Kind::ProducerClosed(_)
            | event::Kind::RoomSnapshot(_)
            | event::Kind::MemberState(_)
            | event::Kind::VoiceClosed(_),
        )
        | None => Vec::new(),
    }
}

/// How the body of a call ended, which decides whether the seat is given back.
enum Over {
    /// Asked to leave.
    Left,
    /// The server ended the place: it is free already.
    Closed(End),
    /// The connection ended: the server frees the place with it.
    Link(Ended),
    /// This side could not go on; the place is still held.
    Failed(Failure),
    /// A step of the call panicked: what it held is unknown, so the call ends, the place is still held, and its
    /// asker builds the call again.
    Fault,
}

/// Whether the place is still held, so this side gives it back: unless the server ended it or the connection went.
fn gives_back(over: &Over) -> bool {
    match over {
        Over::Left | Over::Failed(_) | Over::Fault => true,
        Over::Closed(_) | Over::Link(_) => false,
    }
}

/// How a lane is, for the room: starting until its connection is first up, mending while its ICE is being started
/// again, and up otherwise, a route the engine is still trying by itself included.
fn lane_of(reach: Reach, mending: bool) -> screens::Lane {
    match (reach, mending) {
        (Reach::Starting, _) => screens::Lane::Starting,
        (Reach::Up | Reach::Down | Reach::Lost, true) => screens::Lane::Mending,
        (Reach::Up | Reach::Down | Reach::Lost, false) => screens::Lane::Up,
    }
}

/// What the loop reads: the connection's events, what the person wants and asks, and the leave.
struct Inbox<'a> {
    events: &'a mut Events,
    wants: &'a mut watch::Receiver<Wants>,
    commands: &'a mut mpsc::Receiver<Command>,
    leaving: &'a Leaving,
}

/// A call's ending: how its loop ended, and whether the server was told the place is free where it had to be.
struct Ending {
    over: Over,
    freed: Result<(), LinkError>,
}

/// How an act was carried out.
enum Carried {
    Done,
    /// The call is over.
    Over(Over),
    /// What the act came to, to be decided at once, before the acts that follow it.
    Then(Box<Input>),
}

/// A call under way: the engine's side of it, every handle the call holds. It decides nothing: [`decide::decide`]
/// does, from what is heard here as an [`Input`], and each [`Act`] it returns is carried out here, in order.
struct Driver<'a> {
    link: &'a Link,
    state: decide::State,
    /// What this account sends and plays, and what it takes of the others.
    voice: Voice,
    /// What it takes of the others, toward what the room wants of it.
    lane: Steps,
    source: Source,
    /// The ask of the server for the place's state, until it answers, and the one that came meanwhile.
    asking: Asking<State, Result<(), LinkError>>,
    /// The microphone being opened, and the one open.
    opening: Option<Task<Result<Speaking, VoiceRefused>>>,
    speaking: Option<Speaking>,
    devices: Option<Devices>,
    /// The look at the sound devices, as a task.
    glance: Option<Glance>,
    /// What became of the devices asked for, as the chain tells it.
    sounding: Option<watch::Receiver<Sounding>>,
    /// Where the pictures of each screen shared are put.
    screens: Option<Screens>,
    /// The GPU the host paints with now.
    paints: Option<Paints>,
    /// With no host, the GPU found once at the first screen shown.
    found: Option<Painting>,
    /// Where this account's own screen is lent.
    own_screen: Option<Slot>,
    own: Sender<Screen>,
    camera: Sender<Camera>,
    /// Each screen shared in the call, by its sharer's id.
    viewings: HashMap<Viewed, Viewing>,
    /// The holds and resumes of streams asked of the server.
    holding: JoinSet<Held>,
    /// The send connection's ICE restart asked.
    send_restart: JoinSet<Result<(), VoiceRefused>>,
    /// The engine's report asked for the last look, until it answers.
    looking: Option<Look>,
    /// How each picture watched was taken, as the last look that asked answered.
    watching: Watching,
    /// The loudness tick, while the window shows the call's levels.
    levels: Levelling,
}

impl Driver<'_> {
    /// Decides `input` at `now` and carries out what follows, in order. `Some` ends the call.
    fn run(&mut self, input: Input, now: Instant, report: &mut impl FnMut(Report)) -> Option<Over> {
        in_order(
            self,
            input,
            |driver, input| decide::decide(&mut driver.state, input, now),
            |driver, act| driver.carry(act, now, report),
        )
    }

    fn carry(&mut self, act: Act, now: Instant, report: &mut impl FnMut(Report)) -> Carried {
        match act {
            Act::Report(told) => {
                report(told);
                Carried::Done
            }
            Act::End(over) => Carried::Over(over),
            Act::Voice(act) => self.carry_voice(act, now),
            Act::Own(act) => self.carry_own(act, report),
            Act::Watch(act) => self.carry_watch(act, report),
            Act::Wire(act) => self.carry_wire(act, now, report),
        }
    }

    fn carry_voice(&mut self, act: VoiceAct, now: Instant) -> Carried {
        match act {
            VoiceAct::Deafen(deafened) => self.voice.set_deafened(deafened),
            VoiceAct::Volume { user_id, kind, gain } => self.voice.set_volume(user_id.as_str(), kind, gain),
            VoiceAct::Process(asked) => {
                if let Some(held) = &self.devices {
                    held.process(crate::processing::of(&asked));
                }
            }
            VoiceAct::MuteMic => {
                if let Some(speaking) = &mut self.speaking {
                    speaking.mute();
                }
            }
            VoiceAct::SyncMic { silenced } => {
                return Carried::Then(Box::new(Input::Voice(VoiceInput::MicSynced(self.sync_mic(silenced)))));
            }
            VoiceAct::OpenMic(source) => {
                self.opening = match source {
                    Source::Microphone => Some(Task::spawn(self.voice.speak(self.link))),
                    Source::Tone => Some(Task::spawn(self.voice.speak_tone(self.link))),
                    Source::Nothing => None,
                };
            }
            VoiceAct::DropOpening => self.opening = None,
            VoiceAct::Settle { looking, sounds } => match self.voice.devices() {
                // A hold of its own, for the look's thread.
                Ok(devices) => self.glance = Some(Glance::start(sounds, devices, looking)),
                Err(_) => return Carried::Then(Box::new(Input::Voice(VoiceInput::SettleFailed))),
            },
            VoiceAct::DropGlance => self.glance = None,
            VoiceAct::AskState(state) => self.asking.ask(state, |state| ask_state(self.link, state)),
            VoiceAct::Levels(shown) => self.levels.show(shown, now),
        }
        Carried::Done
    }

    fn carry_own(&mut self, act: OwnAct, report: &mut impl FnMut(Report)) -> Carried {
        match act {
            OwnAct::ShareAsk(asked) => self.own.ask(asked),
            OwnAct::ShareStop => self.own.stop(),
            OwnAct::CameraAsk(pick) => self.camera.ask(pick),
            OwnAct::CameraStop => self.camera.stop(),
            OwnAct::Denied(Video::Screen) => self.own.denied(),
            OwnAct::Denied(Video::Camera) => self.camera.denied(),
            OwnAct::OwnWatched(Video::Screen, watched) => self.own.watched(watched),
            OwnAct::OwnWatched(Video::Camera, watched) => self.camera.watched(watched),
            OwnAct::ShareSoundOf(app) => self.share_sound_of(app, report),
        }
        Carried::Done
    }

    fn carry_watch(&mut self, act: WatchAct, report: &mut impl FnMut(Report)) -> Carried {
        match act {
            WatchAct::Forget(user_id, kind) => {
                if let Some(hearing) = self.lane.idle() {
                    hearing.forget(user_id.as_str(), kind);
                }
            }
            WatchAct::Take(stream, began) => {
                let link = self.link.clone();
                self.lane.start(move |mut hearing| {
                    Box::pin(async move {
                        let took = hearing.take(&link, stream.0.as_str(), stream.1).await;
                        (hearing, Done::Took(stream, began, took))
                    })
                });
            }
            WatchAct::Watch { viewed, began, paused, number } => return self.watch(viewed, began, paused, number),
            WatchAct::LetGo(viewed) => {
                if let Some(viewing) = self.viewings.remove(&viewed) {
                    let link = self.link.clone();
                    self.lane.start(move |hearing| unwatch(hearing, link, viewed, viewing));
                }
            }
            WatchAct::DropStep => self.lane.drop_step(),
            WatchAct::Shown(viewed, shown) => {
                if let Some(viewing) = self.viewings.get_mut(&viewed) {
                    viewing.shown(shown);
                }
            }
            WatchAct::Resumed(viewed) => {
                if let Some(viewing) = self.viewings.get(&viewed) {
                    viewing.resumed();
                }
            }
            WatchAct::Close(viewed) => {
                if let Some(viewing) = self.viewings.get(&viewed) {
                    viewing.close();
                }
            }
            WatchAct::Hold(viewed) => self.hold_viewed(viewed),
            WatchAct::StartAgain(viewed) => self.start_again(viewed),
            WatchAct::AskKeyFrame(viewed) => {
                if let Some(viewing) = self.viewings.get_mut(&viewed) {
                    viewing.ask_key_frame();
                }
            }
            WatchAct::FirstCard(viewed) => {
                let (reach, paused) = (self.voice.receive_reach(), self.state.is_paused(&viewed));
                if let Some(card) = self.viewings.get(&viewed).and_then(|viewing| viewing.first_card(reach, paused)) {
                    report(Report::Watch(Watched::Card(card)));
                }
            }
            WatchAct::LookAtScreens => self.look_at_screens(report),
        }
        Carried::Done
    }

    fn carry_wire(&mut self, act: WireAct, now: Instant, report: &mut impl FnMut(Report)) -> Carried {
        match act {
            WireAct::AskLook => self.ask_look(now, report),
            WireAct::RestartSend => {
                self.send_restart.spawn(self.voice.restart_send(self.link));
            }
            WireAct::MendReceive => {
                let link = self.link.clone();
                self.lane.start(move |mut hearing| {
                    Box::pin(async move {
                        let mended = hearing.restart(&link).await;
                        (hearing, Done::Mended(mended))
                    })
                });
            }
        }
        Carried::Done
    }

    /// Sets the microphone track to the place's state: silent, or sending again.
    fn sync_mic(&mut self, silenced: bool) -> Synced {
        let Some(speaking) = &mut self.speaking else { return Synced::Set };
        if silenced {
            speaking.mute();
            Synced::Set
        } else if speaking.unmute().is_err() {
            Synced::Refused
        } else {
            Synced::Set
        }
    }

    /// The GPU to start a decoder or a send on: the host's as it is now, else the one found once.
    fn painting(&self) -> Option<Painting> {
        self.paints.as_ref().map(Paints::now).or_else(|| self.found.clone())
    }

    /// The GPU a send reads at each lend: the host's as it is then, else the one found once.
    fn paints(&self) -> Option<Paints> {
        self.paints.clone().or_else(|| self.found.clone().map(|found| Paints::new(move || found.clone())))
    }

    /// Starts showing a picture on the lane, `paused` as the person has it.
    fn watch(&mut self, viewed: Viewed, began: Began, paused: bool, number: u64) -> Carried {
        let Some(screens) = &self.screens else {
            let shown = Err(Failure::CannotDecode);
            return Carried::Then(Box::new(Input::Watch(WatchInput::Landed(Landed::Watched { viewed, began, shown }))));
        };
        let slot = (screens.0)(&viewed.0, viewed.1);
        let (link, painting) = (self.link.clone(), self.painting());
        self.lane.start(move |mut hearing| {
            Box::pin(async move {
                let painting = match painting {
                    Some(painting) => painting,
                    None => tokio::task::spawn_blocking(Painting::found).await.unwrap_or_default(),
                };
                let shown = Viewing::start(&mut hearing, &link, viewed.clone(), number, slot, painting.clone(), paused)
                    .await
                    .map(Box::new);
                (hearing, Done::Watched(viewed, began, shown, painting))
            })
        });
        Carried::Done
    }

    /// What a step of the lane did, with what it made kept here.
    fn landed(&mut self, done: Done) -> Landed {
        match done {
            Done::Took(stream, began, took) => Landed::Took(stream, began, took),
            Done::Watched(viewed, began, shown, painting) => {
                if self.paints.is_none() {
                    self.found = Some(painting);
                }
                let shown = shown.map(|viewing| {
                    let paused = viewing.started_held;
                    self.viewings.insert(viewed.clone(), *viewing);
                    paused
                });
                Landed::Watched { viewed, began, shown }
            }
            Done::Unwatched(viewed) => Landed::Unwatched(viewed),
            Done::Mended(mended) => Landed::Mended(mended),
        }
    }

    /// The connections and what is on the send one, as the engine has them now.
    fn wire(&self) -> Wire {
        Wire {
            send: self.voice.send_reach(),
            receive: self.voice.receive_reach(),
            screen: self.own.on_the_wire(),
            camera: self.camera.on_the_wire(),
        }
    }

    /// Whether the microphone is held and open now.
    fn capturing(&self) -> bool {
        self.speaking.as_ref().is_some_and(Speaking::is_capturing)
    }

    /// What the chain told of the devices, with whether its thread runs and the microphone is open.
    fn sounded(&self, now: Sounding) -> Input {
        let alive = self.devices.as_ref().is_none_or(|devices| devices.alive());
        Input::Voice(VoiceInput::Sounded(Sounded { now, alive, capturing: self.capturing() }))
    }

    /// The look at the call every [`LOOK_EVERY`], with the voice chain looked at with whoever else does.
    fn beat(&self, chain: &crate::ChainWatch, now: Instant) -> Input {
        Input::Wire(WireInput::Beat(Beat {
            wire: self.wire(),
            chain: self.devices.as_ref().map(|devices| chain.look(devices, now).alive),
            restarting: !self.send_restart.is_empty(),
            capturing: self.capturing(),
        }))
    }

    /// The steps of this account's own pictures still out at their bounds at `now`, as their senders let them go.
    fn overdue(&mut self, now: Instant) -> Overdue {
        let screen =
            self.own.stalled(now).and_then(|(step, within, turn)| Some((step, within, self.screen_turn(turn)?)));
        let camera =
            self.camera.stalled(now).and_then(|(step, within, turn)| Some((step, within, self.camera_turn(turn)?)));
        Overdue { screen, camera }
    }

    /// This account's own screen took a step: one that opened is sent over the voice, the others are told.
    fn screen_turn(&mut self, turn: Turn<Screen>) -> Option<Turned<ScreenSent, Shared>> {
        match turn {
            Turn::Begun(begun) => {
                let (preview, painting, watched) = (self.own_screen.clone(), self.paints(), self.own.watched_now());
                let sent = begun.send(&self.voice, self.link.clone(), preview, painting, watched);
                self.own.send(Box::pin(sent));
                None
            }
            Turn::Sent => {
                let broadcast = self.own.live()?;
                let (width, height, encoder) = (broadcast.width, broadcast.height, broadcast.encoder.clone());
                Some(Turned::Sent(ScreenSent {
                    picture: Picture { width, height, encoder },
                    sound_said: broadcast.sound_said.clone(),
                    sound_refused: broadcast.sound_refused.clone(),
                }))
            }
            Turn::Over(told, was) => Some(Turned::Over(told, was)),
            Turn::Replaced(was) => Some(Turned::Replaced(was)),
        }
    }

    /// This account's own camera took a step, as [`Driver::screen_turn`] has the screen's.
    fn camera_turn(&mut self, turn: Turn<Camera>) -> Option<Turned<Picture, CameraSent>> {
        match turn {
            Turn::Begun(begun) => {
                let live = begun.send(&self.voice, self.link.clone());
                self.camera.send(Box::pin(live));
                None
            }
            Turn::Sent => {
                let live = self.camera.live()?;
                let (mode, encoder) = (live.mode(), live.encoder().clone());
                Some(Turned::Sent(Picture { width: mode.width, height: mode.height, encoder }))
            }
            Turn::Over(told, was) => Some(Turned::Over(told, was)),
            Turn::Replaced(was) => Some(Turned::Replaced(was)),
        }
    }

    /// The sharer picked the application whose sound their window share sends; said as the share's sound now.
    #[cfg(target_os = "linux")]
    fn share_sound_of(&mut self, app: screens::SoundApp, report: &mut impl FnMut(Report)) {
        let Some(broadcast) = self.own.live() else { return };
        broadcast.hear(::sound::share::App { name: app.name, binary: app.binary });
        if let Some(said) = broadcast.sound_said.clone() {
            report(Report::Share(Shared::Sound(said)));
        }
    }

    /// Elsewhere a window's owner is always found, so nothing is picked.
    #[cfg(not(target_os = "linux"))]
    fn share_sound_of(&mut self, _app: screens::SoundApp, _report: &mut impl FnMut(Report)) {}

    /// Asks the server to hold the stream of `viewed`; the answer is told to the call, which holds it only once the
    /// server did.
    fn hold_viewed(&mut self, viewed: Viewed) {
        let link = self.link.clone();
        self.holding.spawn(async move {
            let held = viewing::hold(&link, &viewed).await;
            Held::Asked(viewed, held)
        });
    }

    /// Asks the server to start the held stream of `viewed` again.
    fn start_again(&mut self, viewed: Viewed) {
        let link = self.link.clone();
        self.holding.spawn(async move {
            if !viewing::unhold(&link, &viewed).await {
                report::note!(warn, "a held picture was not started again");
            }
            Held::Started
        });
    }

    /// Asks the engine how what is sent and the connections are, unless the last look is still out, which is said
    /// once it is late, and dropped for a new one at its bound.
    fn ask_look(&mut self, now: Instant, report: &mut impl FnMut(Report)) {
        if let Some(look) = &mut self.looking {
            let Some(waiting) = look.stalled(now) else {
                if let Some(waiting) = look.late(now) {
                    report::note!(
                        warn,
                        "the engine has not answered a look in its time",
                        parts_waiting = waiting.len(),
                        late_ms = report::millis(look::LATE),
                    );
                }
                return;
            };
            report(decide::stalled(format_args!("a look ({})", waiting.join(", ")), look::within()));
        }
        let share = self.own.live().map(|broadcast| broadcast.look_ask());
        let camera = self.camera.live().map(|live| live.look_ask());
        let tone = match (&self.speaking, self.source) {
            (Some(speaking), Source::Tone) => Some(speaking.level_ask()),
            (Some(_), Source::Microphone | Source::Nothing) | (None, _) => None,
        };
        let watching = (!self.viewings.is_empty()).then(|| self.voice.watch_stats_ask());
        self.looking = Some(Look::ask(now, share, camera, tone, self.voice.connection_ask(), watching));
    }

    /// The engine answered a look: what is sent is said, and the rest goes to the call.
    fn looked(&mut self, looked: &Looked, report: &mut impl FnMut(Report)) -> Input {
        let (screen_watched, camera_watched) = (self.own.is_watched(), self.camera.is_watched());
        if let (Some((of, engine)), Some(broadcast)) = (looked.share, self.own.live())
            && let Some(outgoing) = broadcast.looked(of, engine, screen_watched)
        {
            report(Report::Share(Shared::Looked(outgoing)));
        }
        if let Some((of, engine)) = looked.camera
            && let Some(outgoing) = self.camera.live().and_then(|live| live.looked(of, engine, camera_watched))
        {
            report(Report::Camera(CameraSent::Looked(outgoing)));
        }
        if let Some(watching) = &looked.watching {
            self.watching.clone_from(watching);
        }
        Input::Wire(WireInput::Answered(Answered {
            tone: looked.tone,
            connection: looked.connection.as_ref().ok().copied(),
            heartbeat: self.link.round_trip(),
        }))
    }

    /// How the screens are, every [`LOOK_EVERY`]: the one sent and each shown, from what needs no ask.
    fn look_at_screens(&mut self, report: &mut impl FnMut(Report)) {
        if let Some(broadcast) = self.own.live() {
            if let Some(route) = broadcast.handed_over() {
                report(Report::Share(Shared::HandedOver(route)));
            }
            if broadcast.first_unshown() {
                report(Report::Share(Shared::Unshown));
            }
            if broadcast.first_sound_stopped() {
                report(Report::Notice(Subject::ShareSound, words::failure::of(&Failure::ShareSoundStopped)));
            }
        }
        let reach = self.voice.receive_reach();
        for (viewed, viewing) in &mut self.viewings {
            let taken = self.watching.get(&(viewing.user_id.as_str().to_owned(), viewing.video));
            let card = viewing.card(LOOK_EVERY, reach, self.state.is_paused(viewed), taken);
            if let Some(card) = card.filter(|_| viewing.video == Video::Screen) {
                report(Report::Watch(Watched::Card(card)));
            }
            if let Some(route) = viewing.handed_over() {
                let (user_id, video) = (viewing.user_id.clone(), viewing.video);
                report(Report::Watch(Watched::HandedOver { user_id, video, route }));
            }
            if viewing.unopened() {
                let (user_id, video) = (viewing.user_id.clone(), viewing.video);
                report(Report::Watch(Watched::Failed { user_id, video, failed: Failure::CannotDecode }));
            }
        }
    }

    /// The call is over: what it sends and shows goes with it, each beside the others. What it takes goes before the
    /// pictures it fed, a step under way with it, so no screen is fed after its slot is emptied.
    async fn end(&mut self, report: &mut impl FnMut(Report)) {
        tokio::join!(self.own.end(), self.camera.end(), self.lane.end());
        // Each a task of its own, never a set that ends them when dropped: past the deadline they end behind.
        let ending: Vec<_> =
            self.viewings.drain().map(|(viewed, viewing)| (viewed, tokio::spawn(ended_shown(viewing)))).collect();
        for ((user_id, video), ended) in ending {
            let _ = ended.await;
            report(Report::Watch(Watched::Ended { user_id, video }));
        }
    }
}

/// The call from the engine's setup to its end. Ending, it goes silent at once, tells the server before anything is
/// taken down where the place is this side's to give back, and takes everything down together within
/// [`LEAVE_WITHIN`].
async fn serve(
    link: &Link,
    engine: &Engine,
    inbox: Inbox<'_>,
    join: &Join,
    (state, mut carried): (&VoiceState, Wants),
    report: &mut impl FnMut(Report),
) -> Result<Ending, Over> {
    let Inbox { events, wants, commands, leaving } = inbox;
    // The link beats every few seconds while the call lasts, so its round trip is always there to read.
    let _quickened = link.quicken();
    let located = match webrtc::locate(state, link.peer()).await {
        Ok(located) => located,
        Err(why) => return Err(Over::Failed(failure::voice(why))),
    };
    let voice_at = {
        let (host, port) = located.announced();
        (host.to_string(), port)
    };
    let (voice, hearing) = match Voice::new(engine, state, located) {
        Ok(halves) => halves,
        Err(why) => return Err(Over::Failed(failure::voice(why))),
    };
    for (user_id, loudness) in carried.volumes().iter() {
        voice.set_volume(user_id.as_str(), heard_on(Audio::Voice), loudness.voice.fraction());
        voice.set_volume(user_id.as_str(), heard_on(Audio::Share), loudness.share.fraction());
    }
    // A place that joins held was started muted by the server whatever the join asked.
    let held = state
        .participants
        .iter()
        .any(|member| join.me.as_ref().is_some_and(|me| me.as_str() == member.user_id) && member.admin_muted);
    let devices = engine.devices().ok().inspect(|devices| devices.process(crate::processing::of(&join.processing)));
    let mut driver = Driver {
        link,
        state: decide::State::new(decide::Setup {
            me: join.me.clone(),
            channel_id: join.channel_id.clone(),
            source: join.source,
            jitter: join.jitter,
            shows: join.screens.is_some(),
            has_devices: devices.is_some(),
            start: carried.start(),
            held: if held { Hold::Held } else { Hold::Free },
            microphone: join.microphone.clone(),
            speaker: join.speaker.clone(),
            unseen: carried.unseen().to_vec(),
            paused: carried.paused().to_vec(),
            voice_at,
        }),
        voice,
        lane: Steps::new(hearing),
        source: join.source,
        asking: Asking::default(),
        opening: None,
        speaking: None,
        sounding: devices.as_ref().map(Devices::sounding),
        devices,
        glance: None,
        screens: join.screens.clone(),
        paints: join.painting.clone(),
        found: None,
        own_screen: join.own_screen.clone(),
        own: Sender::new(Screen {
            encoders: Arc::clone(&join.encoders),
            own: join.own_screen.clone(),
            link: link.clone(),
        }),
        camera: Sender::new(Camera {
            encoders: Arc::clone(&join.encoders),
            own: join.own_camera.clone(),
            link: link.clone(),
        }),
        viewings: HashMap::new(),
        holding: JoinSet::new(),
        send_restart: JoinSet::new(),
        looking: None,
        watching: Watching::new(),
        levels: Levelling::new(join.level_every, Instant::now()),
    };
    // Before anything is opened, so each device starts as the one that is meant and is not moved after.
    driver.run(Input::Voice(VoiceInput::Settle(Looking::Forced)), Instant::now(), report);
    if let Some(due) = driver.state.settling_due() {
        let input = match tokio::time::timeout_at(due, glanced(&mut driver.glance)).await {
            Ok(Ok(looked)) => Input::Voice(VoiceInput::Settled(Some(looked))),
            Ok(Err(Panicked)) => return Err(Over::Fault),
            Err(_) => Input::Wire(WireInput::Overdue(Overdue::default())),
        };
        driver.run(input, Instant::now(), report);
    }
    // Asked only: a speaker that would not open is told with what became of it, as one refused already is here.
    let _ = driver.voice.play();
    let sounded = driver.sounding.as_mut().map(|told| *told.borrow_and_update());
    if let Some(now) = sounded {
        let input = driver.sounded(now);
        driver.run(input, Instant::now(), report);
    }
    driver.voice.set_deafened(carried.start().deafened);
    // The lane takes what is there from the loop on, so a leave is heard while it does.
    let room = Joined {
        room: joined(&state.participants, &state.streams),
        streams: state.streams.iter().filter_map(stream_of).collect(),
        wire: driver.wire(),
    };
    driver.run(Input::Wire(WireInput::Joined(room)), Instant::now(), report);

    let mut health = interval_at(Instant::now() + LOOK_EVERY, LOOK_EVERY);
    health.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // No arm awaits an ask: each starts one, as a task of its own, or takes its answer, so the call reads, plays and
    // leaves meanwhile, and a step still out at its bound is let go by the arm that waits for the earliest.
    let over = loop {
        if let Some(over) = driver.run(Input::Watch(WatchInput::Advance), Instant::now(), report) {
            break over;
        }
        let asks = driver.link;
        let due = bounded::earliest([driver.state.due(), driver.own.due(), driver.camera.due()]);
        // With the lane free everything due was started, so this is in the future; with a step under way its
        // landing brings the lane here again.
        let retry = driver.state.retry_at();
        let ended: Option<Over> = tokio::select! {
            biased;
            () = leaving.0.notified() => driver.run(Input::Wire(WireInput::Leave), Instant::now(), report),
            asked = commands.recv() => match asked {
                // A closed command channel is a leave.
                None => driver.run(Input::Wire(WireInput::Leave), Instant::now(), report),
                Some(first) => {
                    let mut all = vec![first];
                    while let Ok(next) = commands.try_recv() {
                        all.push(next);
                    }
                    driver.run(Input::Voice(VoiceInput::Commands(all)), Instant::now(), report)
                }
            },
            // A host that let go of what is wanted wants nothing more: the arm is off, and the call goes on.
            Ok(()) = wants.changed() => {
                let is = wants.borrow_and_update().clone();
                let was = std::mem::replace(&mut carried, is.clone());
                driver.run(Input::Voice(VoiceInput::Wants { was, is }), Instant::now(), report)
            }
            told = driver.asking.answered(|state| ask_state(asks, state)) => driver.run(Input::Voice(VoiceInput::Asked(told)), Instant::now(), report),
            opened = mic_opened(&mut driver.opening) => match opened {
                Ok(opened) => {
                    let opened = opened.map(|speaking| driver.speaking = Some(speaking));
                    driver.run(Input::Voice(VoiceInput::MicOpened(opened)), Instant::now(), report)
                }
                Err(Panicked) => Some(Over::Fault),
            },
            turn = driver.own.next() => match driver.screen_turn(turn) {
                Some(turned) => driver.run(Input::Own(OwnInput::Screen(turned)), Instant::now(), report),
                None => None,
            },
            turn = driver.camera.next() => match driver.camera_turn(turn) {
                Some(turned) => driver.run(Input::Own(OwnInput::Camera(turned)), Instant::now(), report),
                None => None,
            },
            done = driver.lane.done() => {
                let input = match done {
                    Ok(done) => Input::Watch(WatchInput::Landed(driver.landed(done))),
                    Err(Panicked) => Input::Watch(WatchInput::LaneLost),
                };
                driver.run(input, Instant::now(), report)
            }
            () = bounded::at(due) => {
                let now = Instant::now();
                let late = driver.overdue(now);
                driver.run(Input::Wire(WireInput::Overdue(late)), now, report)
            }
            // A refused stream is due again: the lane takes it up at the top of the loop.
            () = bounded::at(retry) => None,
            looked = glanced(&mut driver.glance) => match looked {
                Ok(looked) => driver.run(Input::Voice(VoiceInput::Settled(Some(looked))), Instant::now(), report),
                Err(Panicked) => Some(Over::Fault),
            },
            now = sound::sounding(&mut driver.sounding) => {
                let input = driver.sounded(now);
                driver.run(input, Instant::now(), report)
            }
            viewed = Viewing::needs_key_frame(&driver.viewings) => {
                driver.run(Input::Watch(WatchInput::NeedsKeyFrame(viewed)), Instant::now(), report)
            }
            viewed = Viewing::reopened(&driver.viewings) => {
                driver.run(Input::Watch(WatchInput::Opened(viewed)), Instant::now(), report)
            }
            () = bounded::at(driver.state.hold_at()) => driver.run(Input::Watch(WatchInput::HoldUnseen), Instant::now(), report),
            viewed = Viewing::first_picture(&driver.viewings, |viewed| driver.state.first_reported(viewed)) => {
                driver.run(Input::Watch(WatchInput::FirstPicture(viewed)), Instant::now(), report)
            }
            Some(held) = driver.holding.join_next() => match held {
                Ok(Held::Asked(viewed, held)) => driver.run(Input::Watch(WatchInput::Held(viewed, held)), Instant::now(), report),
                Ok(Held::Started) | Err(_) => None,
            },
            () = bounded::at(driver.state.key_frame_at()) => {
                let waiting =
                    driver.viewings.iter().map(|(viewed, viewing)| (viewed.clone(), viewing.waiting())).collect();
                driver.run(Input::Watch(WatchInput::KeyFramesDue(waiting)), Instant::now(), report)
            }
            () = driver.levels.tick() => {
                let heard: Vec<(MemberId, u8)> = driver
                    .voice
                    .levels()
                    .into_iter()
                    .filter_map(|(user_id, level)| Some((MemberId::parse(&user_id)?, level)))
                    .collect();
                // The chain's own count, read with no ask of the engine: this never waits.
                let microphone = match (&driver.speaking, driver.source) {
                    (Some(speaking), Source::Microphone) => speaking.level().await,
                    (Some(_), Source::Tone | Source::Nothing) | (None, _) => None,
                };
                let levels = Levels { heard, microphone };
                driver.run(Input::Voice(VoiceInput::Levels(levels)), Instant::now(), report)
            }
            Some(restarted) = driver.send_restart.join_next() => {
                let restarted = restarted.unwrap_or(Err(VoiceRefused::Engine));
                driver.run(Input::Wire(WireInput::SendRestarted(restarted)), Instant::now(), report)
            }
            _ = health.tick() => {
                let now = Instant::now();
                let beat = driver.beat(&join.chain, now);
                driver.run(beat, now, report)
            }
            looked = look::answered(&mut driver.looking) => {
                driver.looking = None;
                match looked {
                    Ok(looked) => {
                        let input = driver.looked(&looked, report);
                        driver.run(input, Instant::now(), report)
                    }
                    Err(Panicked) => Some(Over::Fault),
                }
            }
            heard = events.next() => match heard {
                Some(Heard::Event(event)) => driver.run(Input::Wire(WireInput::Event(event)), Instant::now(), report),
                Some(Heard::Behind) => driver.run(Input::Wire(WireInput::Behind), Instant::now(), report),
                Some(Heard::Ended(ended)) => driver.run(Input::Wire(WireInput::Ended(ended)), Instant::now(), report),
                None => driver.run(Input::Wire(WireInput::Ended(Ended::default())), Instant::now(), report),
            },
        };
        if let Some(over) = ended {
            break over;
        }
    };
    driver.run(Input::Voice(VoiceInput::Hush), Instant::now(), report);
    let told = async {
        match gives_back(&over) {
            true => link.ask_within(VoiceLeave {}, LEAVE_WITHIN).await.map(|_| ()),
            false => Ok(()),
        }
    };
    let down = async {
        driver.end(report).await;
        // The connections close as the engine's halves go, which may wait on the engine: never on this thread.
        let Driver { voice, speaking, opening, devices, .. } = driver;
        let _ = tokio::task::spawn_blocking(move || drop((voice, speaking, opening, devices))).await;
    };
    // The leave is asked first, so the server has it before a `VoiceStop` of what is taken down.
    let ended = Box::pin(tokio::time::timeout(LEAVE_WITHIN, async { tokio::join!(told, down).0 })).await;
    let freed = ended.unwrap_or_else(|_| {
        report(Report::Said(format!("The call's end took over {LEAVE_WITHIN:?}; what is left ends behind it.")));
        Err(LinkError::Timeout)
    });
    Ok(Ending { over, freed })
}

/// What an ICE restart of the `lane` connection did, for an operator.
fn restart_said(lane: &str, restarted: Result<(), VoiceRefused>) -> String {
    match restarted {
        Ok(()) => format!("The {lane} route was lost; its ICE starts again."),
        Err(refused) => format!("The {lane} route was lost and its ICE did not start again: {refused:?}"),
    }
}

/// Whether what the chain told mutes the place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Muting {
    Leave,
    /// Muted, as the server is told, unless the microphone before still sounds.
    Silence,
}

/// What a call does about what the chain `told` of its devices while it is `sending` a microphone, whose own are
/// this call's to say only then. A microphone refused, or whose retry did not answer either, mutes the place, and
/// asking it again is unmuting; one not answering yet leaves it as it is. A device said not to answer that opens
/// clears its mark, said back by its name in `sounds` where it has one and its stall was the person's news.
fn heed(told: Told, sending: bool, sounds: &Sounds) -> (Option<Report>, Muting) {
    let notice = |subject: Subject, failed: &Failure| Some(Report::Notice(subject, words::failure::of(failed)));
    match told {
        Told::Refused(Direction::Speaker, failed) | Told::Unanswered(Direction::Speaker, failed) => {
            (notice(Subject::Speaker, &failed), Muting::Leave)
        }
        Told::Hung(Direction::Speaker, _) => (None, Muting::Leave),
        Told::Refused(Direction::Microphone, _)
        | Told::Unanswered(Direction::Microphone, _)
        | Told::Hung(Direction::Microphone, _)
            if !sending =>
        {
            (None, Muting::Leave)
        }
        Told::Refused(Direction::Microphone, failed) | Told::Hung(Direction::Microphone, failed) => {
            (notice(Subject::Microphone, &failed), Muting::Silence)
        }
        Told::Unanswered(Direction::Microphone, failed) => (notice(Subject::Microphone, &failed), Muting::Leave),
        Told::Answered(direction) => {
            let (subject, news) = match direction {
                Direction::Microphone => (Subject::Microphone, sending),
                Direction::Speaker => (Subject::Speaker, true),
            };
            let said = match sounds.answered(direction).filter(|_| news) {
                Some(back) => Report::Notice(subject, back),
                None => Report::Answered(subject),
            };
            (Some(said), Muting::Leave)
        }
        Told::Stopped => {
            let muting = if sending { Muting::Silence } else { Muting::Leave };
            (notice(Subject::Devices, &Failure::SoundStopped), muting)
        }
    }
}

/// A screen no longer shown: its decoder stops with it, joined on the blocking pool, before its slot is emptied, so no
/// picture of it is put in after.
async fn ended_shown(viewing: Viewing) {
    let slot = viewing.slot.clone();
    let _ = tokio::task::spawn_blocking(move || drop(viewing)).await;
    slot.empty();
}

/// The lane's step that lets go of a picture, given back to the server, so a watch of it again starts only once its
/// slot is emptied and the server holds none of it.
fn unwatch(mut hearing: Hearing, link: Link, viewed: Viewed, viewing: Viewing) -> Flight<(Hearing, Done)> {
    Box::pin(async move {
        // The server forgets a stopped stream's consumer itself, but not one it gave for the stream that replaced it
        // while this end still thought it the old: kept, the next ask for that stream is refused as taken.
        hearing.release(&link, viewed.0.as_str(), viewed.1.kind()).await;
        ended_shown(viewing).await;
        (hearing, Done::Unwatched(viewed))
    })
}

/// The ask of the server for the place's state, `state` being what the person wants.
fn ask_state(link: &Link, state: State) -> Flight<Result<(), LinkError>> {
    let link = link.clone();
    let set = VoiceSetState { mic_muted: state.mic_muted, deafened: state.deafened };
    Box::pin(async move { link.ask(set).await.map(|_| ()) })
}

/// The loudness tick of a call: every `every` while the window shows its levels, and none while it does not.
struct Levelling {
    every: Duration,
    ticking: Option<tokio::time::Interval>,
}

impl Levelling {
    /// Ticking from `now`, as a call starts.
    fn new(every: Duration, now: Instant) -> Self {
        let mut levelling = Self { every, ticking: None };
        levelling.show(true, now);
        levelling
    }

    /// Ticks from `now` on while `shown`, not at all while not; a tick already running is kept.
    fn show(&mut self, shown: bool, now: Instant) {
        match (shown, &self.ticking) {
            (true, Some(_)) => {}
            (true, None) => {
                let mut ticking = interval_at(now + self.every, self.every);
                ticking.set_missed_tick_behavior(MissedTickBehavior::Skip);
                self.ticking = Some(ticking);
            }
            (false, _) => self.ticking = None,
        }
    }

    /// The next tick; never, while the levels are not shown.
    async fn tick(&mut self) {
        match &mut self.ticking {
            Some(ticking) => {
                ticking.tick().await;
            }
            None => std::future::pending().await,
        }
    }
}

/// Asks of the server one at a time, in the order they were made: an ask that comes while one is out waits behind it,
/// the newest alone, and is made once that one is answered.
struct Asking<S, T> {
    flight: Option<Flight<T>>,
    behind: Option<S>,
}

impl<S, T> Default for Asking<S, T> {
    fn default() -> Self {
        Self { flight: None, behind: None }
    }
}

impl<S, T> Asking<S, T> {
    /// Asks `state` with `start` unless an ask is out, when it waits behind that one.
    fn ask(&mut self, state: S, start: impl FnOnce(S) -> Flight<T>) {
        if self.flight.is_some() {
            self.behind = Some(state);
        } else {
            self.flight = Some(start(state));
        }
    }

    /// Resolves with the answer to the ask that is out, then makes the one waiting behind it with `start`; never,
    /// while none is out. Dropping it leaves the ask in flight.
    async fn answered(&mut self, start: impl FnOnce(S) -> Flight<T>) -> T {
        match &mut self.flight {
            Some(flight) => {
                let told = flight.await;
                self.flight = self.behind.take().map(start);
                told
            }
            None => std::future::pending().await,
        }
    }
}

/// Decides `first` with `decide` and carries out each act it returns with `carry`, in order. What an act comes to
/// (`Carried::Then`) is decided after the acts of the decision that asked for it, and the answers to those in the
/// order they were asked, so no act is carried out before one decided earlier. `Some` ends the call.
fn in_order<C>(
    context: &mut C,
    first: Input,
    mut decide: impl FnMut(&mut C, Input) -> Vec<Act>,
    mut carry: impl FnMut(&mut C, Act) -> Carried,
) -> Option<Over> {
    let mut inputs = VecDeque::from([first]);
    while let Some(input) = inputs.pop_front() {
        for act in decide(context, input) {
            match carry(context, act) {
                Carried::Done => {}
                Carried::Over(over) => return Some(over),
                Carried::Then(next) => inputs.push_back(*next),
            }
        }
    }
    None
}

/// Resolves when the microphone being opened opens or would not; never otherwise. Dropping it leaves the opening
/// under way.
async fn mic_opened(
    opening: &mut Option<Task<Result<Speaking, VoiceRefused>>>,
) -> Result<Result<Speaking, VoiceRefused>, Panicked> {
    match opening {
        Some(task) => {
            let opened = task.answer().await;
            *opening = None;
            opened
        }
        None => std::future::pending().await,
    }
}

/// Resolves with the look at the devices, [`Panicked`] when it ended without an answer; never, while none is out.
/// Cancel-safe.
async fn glanced(glance: &mut Option<Glance>) -> Result<(sound::Plan, sound::Chosen), Panicked> {
    match glance {
        Some(looking) => {
            let looked = looking.answer().await;
            *glance = None;
            looked
        }
        None => std::future::pending().await,
    }
}

/// Joins the channel in `join` and is in it until `leaving` is told or the call ends, playing every microphone there,
/// sending this device's source, keeping to what `wants` holds (as it is when the place is asked for, and every
/// change after) and carrying out `commands`. The place is given back once on every ending this side
/// causes; the server frees it itself when it ended the call or the connection went.
pub async fn run(
    link: &Link,
    engine: &Engine,
    events: &mut Events,
    join: Join,
    (mut wants, mut commands): (watch::Receiver<Wants>, mpsc::Receiver<Command>),
    leaving: Leaving,
    mut report: impl FnMut(Report),
) -> Result<(), Stopped> {
    let first = wants.borrow_and_update().clone();
    let state = join_as(link, &join.channel_id, first.start()).await?;
    let inbox = Inbox { events, wants: &mut wants, commands: &mut commands, leaving: &leaving };
    let served = Box::pin(serve(link, engine, inbox, &join, (&state, first), &mut report)).await;
    let Ending { over, freed } = match served {
        Ok(ending) => ending,
        // No call was made, yet the server gave a place: it is given back.
        Err(over) => Ending { over, freed: link.ask_within(VoiceLeave {}, LEAVE_WITHIN).await.map(|_| ()) },
    };
    if let Err(LinkError::Closed(ended)) = &freed {
        return Err(over_by_link(&mut report, ended.clone()));
    }
    match over {
        Over::Left => {
            match freed {
                Ok(()) => report(Report::Left(Ok(()))),
                Err(why) => report(Report::Left(Err(words::failure::said(&why)))),
            }
            Ok(())
        }
        Over::Closed(end) => Err(over_by(&mut report, end)),
        Over::Link(ended) => Err(over_by_link(&mut report, ended)),
        Over::Failed(failed) => Err(over_by(&mut report, End::Failed(failed))),
        Over::Fault => {
            report(Report::Room(RoomEvent::Ended(End::Failed(Failure::CallFaulted))));
            Err(Stopped::Fault)
        }
    }
}

fn over_by(report: &mut impl FnMut(Report), end: End) -> Stopped {
    let stopped = Stopped::Ended(end.clone());
    report(Report::Room(RoomEvent::Ended(end)));
    stopped
}

fn over_by_link(report: &mut impl FnMut(Report), ended: Ended) -> Stopped {
    report(Report::Room(RoomEvent::Ended(End::Link(ended.clone()))));
    Stopped::Disconnected(ended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::chatkord_v1::{
        ChannelRenamed, MemberJoined, MemberState, ProducerClosed, ProducerJoined, RoomSnapshot, ShareWatched,
        VoiceClosed,
    };

    const BOOTH: &str = "booth";
    const OTHER: &str = "other";
    const ADA: &str = "ada";

    fn booth() -> ChannelId {
        ChannelId::parse(BOOTH).expect("a channel id")
    }

    fn member(id: &str) -> MemberId {
        MemberId::parse(id).expect("a member id")
    }

    fn event(kind: event::Kind) -> Event {
        Event { cursor: Vec::new(), kind: Some(kind) }
    }

    fn microphone() -> i32 {
        i32::from(TrackKind::Microphone)
    }

    fn named(label: &str) -> Sounds {
        let default = || vec![screens::Listed { asked: None, label: label.into() }];
        Sounds::settled_on(sound::Listing { microphones: default(), speakers: default() })
    }

    #[test]
    fn a_microphone_whose_retry_did_not_answer_either_mutes_the_place_as_a_refused_one_does() {
        let sounds = named("Desk");
        let stalled = || Some(Report::Notice(Subject::Microphone, words::failure::of(&Failure::EngineStalled)));
        let hung = Told::Hung(Direction::Microphone, Failure::EngineStalled);
        assert_eq!(heed(hung.clone(), true, &sounds), (stalled(), Muting::Silence), "said, and muted for everyone");
        let refused = Told::Refused(Direction::Microphone, Failure::DeviceRefused);
        assert_eq!(heed(refused, true, &sounds).1, Muting::Silence, "as a refusal");
        assert_eq!(heed(hung, false, &sounds), (None, Muting::Leave), "muted already: nothing to say or do");
        let waiting = Told::Unanswered(Direction::Microphone, Failure::EngineStalled);
        assert_eq!(heed(waiting, true, &sounds), (stalled(), Muting::Leave), "its retry may bring it back");
        let speaker = Told::Hung(Direction::Speaker, Failure::EngineStalled);
        assert_eq!(heed(speaker, true, &sounds), (None, Muting::Leave), "a speaker has nothing to mute");
    }

    #[test]
    fn a_microphone_that_answers_after_it_did_not_clears_its_mark_said_back_by_name_or_silently() {
        let answered = Told::Answered(Direction::Microphone);
        let back = Some(Report::Notice(Subject::Microphone, Sentence::SoundBack("Desk".into())));
        assert_eq!(heed(answered.clone(), true, &named("Desk")), (back, Muting::Leave));
        let cleared = (Some(Report::Answered(Subject::Microphone)), Muting::Leave);
        assert_eq!(heed(answered.clone(), true, &named("")), cleared, "no name: cleared with no toast");
        assert_eq!(heed(answered.clone(), true, &Sounds::default()), cleared, "nothing listed yet: the same");
        assert_eq!(heed(answered, false, &named("Desk")), cleared, "not sending: no news, but no mark left");
    }

    #[test]
    fn what_names_another_channel_is_not_this_calls() {
        let asked = |channel: &str| {
            [
                event::Kind::MemberJoined(MemberJoined {
                    channel_id: channel.into(),
                    user_id: ADA.into(),
                    ..Default::default()
                }),
                event::Kind::ProducerJoined(ProducerJoined {
                    channel_id: channel.into(),
                    user_id: ADA.into(),
                    kind: microphone(),
                }),
                event::Kind::ProducerClosed(ProducerClosed {
                    channel_id: channel.into(),
                    user_id: ADA.into(),
                    kind: microphone(),
                    ..Default::default()
                }),
                event::Kind::MemberState(MemberState {
                    channel_id: channel.into(),
                    user_id: ADA.into(),
                    ..Default::default()
                }),
                event::Kind::RoomSnapshot(RoomSnapshot { channel_id: channel.into(), ..Default::default() }),
                event::Kind::VoiceClosed(VoiceClosed { channel_id: channel.into(), ..Default::default() }),
            ]
            .into_iter()
            .map(|kind| steps(&booth(), None, event(kind)).len())
            .collect::<Vec<_>>()
        };
        assert_eq!(asked(OTHER), [0; 6], "nothing of another channel changes this room or ends this call");
        assert!(asked(BOOTH).iter().all(|steps| *steps > 0), "and the same events of this channel do");
    }

    #[test]
    fn a_microphone_that_starts_is_told_to_the_room_and_taken_and_one_that_ends_is_forgotten() {
        let started = ProducerJoined { channel_id: BOOTH.into(), user_id: ADA.into(), kind: microphone() };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerJoined(started))),
            [
                Step::Room(RoomEvent::Started { user_id: member(ADA), track: Track::Microphone }),
                Step::Began((member(ADA), TrackKind::Microphone))
            ]
        );
        let ended = ProducerClosed { channel_id: BOOTH.into(), user_id: ADA.into(), kind: microphone(), reason: 0 };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerClosed(ended))),
            [
                Step::Room(RoomEvent::Stopped { user_id: member(ADA), track: Track::Microphone }),
                Step::Ended((member(ADA), TrackKind::Microphone))
            ]
        );
        let screen =
            ProducerJoined { channel_id: BOOTH.into(), user_id: ADA.into(), kind: i32::from(TrackKind::ScreenVideo) };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerJoined(screen))),
            [
                Step::Room(RoomEvent::Started { user_id: member(ADA), track: Track::ScreenVideo }),
                Step::Began((member(ADA), TrackKind::ScreenVideo))
            ],
            "a screen is wanted from its start"
        );
    }

    #[test]
    fn a_shares_sound_is_taken_when_it_starts_and_forgotten_when_it_ends() {
        let sound = i32::from(TrackKind::ScreenAudio);
        let started = ProducerJoined { channel_id: BOOTH.into(), user_id: ADA.into(), kind: sound };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerJoined(started))),
            [
                Step::Room(RoomEvent::Started { user_id: member(ADA), track: Track::ScreenAudio }),
                Step::Began((member(ADA), TrackKind::ScreenAudio))
            ],
            "everyone in the call hears it"
        );
        let ended = ProducerClosed { channel_id: BOOTH.into(), user_id: ADA.into(), kind: sound, reason: 0 };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerClosed(ended))),
            [
                Step::Room(RoomEvent::Stopped { user_id: member(ADA), track: Track::ScreenAudio }),
                Step::Ended((member(ADA), TrackKind::ScreenAudio))
            ]
        );
    }

    #[test]
    fn one_s_own_screen_ended_for_one_s_roles_is_a_share_denied_and_nothing_else_of_one_s_own_is_heard() {
        let own = |kind: TrackKind, reason: CloseReason| {
            let ended = ProducerClosed {
                channel_id: BOOTH.into(),
                user_id: ADA.into(),
                kind: i32::from(kind),
                reason: i32::from(reason),
            };
            steps(&booth(), Some(&member(ADA)), event(event::Kind::ProducerClosed(ended)))
        };
        assert_eq!(own(TrackKind::ScreenVideo, CloseReason::NotAllowed), [Step::Denied(Video::Screen)]);
        assert_eq!(own(TrackKind::Camera, CloseReason::NotAllowed), [Step::Denied(Video::Camera)], "a camera alike");
        assert_eq!(own(TrackKind::ScreenAudio, CloseReason::NotAllowed), [], "the share's sound goes with its picture");
        assert_eq!(own(TrackKind::ScreenVideo, CloseReason::Normal), [], "the call tells its own stop itself");
        assert_eq!(own(TrackKind::Microphone, CloseReason::NotAllowed), []);
    }

    #[test]
    fn a_screen_that_ends_is_told_to_the_room_and_let_go() {
        let screen = i32::from(TrackKind::ScreenVideo);
        let ended = ProducerClosed { channel_id: BOOTH.into(), user_id: ADA.into(), kind: screen, reason: 0 };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::ProducerClosed(ended))),
            [
                Step::Room(RoomEvent::Stopped { user_id: member(ADA), track: Track::ScreenVideo }),
                Step::Ended((member(ADA), TrackKind::ScreenVideo))
            ]
        );
    }

    #[test]
    fn a_snapshot_names_every_stream_and_one_with_no_kind_is_left_out() {
        let streams = [
            (ADA, TrackKind::ScreenVideo),
            (ADA, TrackKind::ScreenAudio),
            (OTHER, TrackKind::Microphone),
            (OTHER, TrackKind::Unspecified),
        ]
        .map(|(user_id, kind)| StreamRef { user_id: user_id.into(), kind: i32::from(kind) });
        let whole = RoomSnapshot { channel_id: BOOTH.into(), streams: streams.to_vec(), ..Default::default() };
        let told = steps(&booth(), None, event(event::Kind::RoomSnapshot(whole)));
        let Some(listed) = told.into_iter().find_map(|step| match step {
            Step::Whole(_, listed) => Some(listed),
            _ => None,
        }) else {
            panic!("not a snapshot")
        };
        let named = [(ADA, TrackKind::ScreenVideo), (ADA, TrackKind::ScreenAudio), (OTHER, TrackKind::Microphone)];
        assert_eq!(listed, named.map(|(user_id, kind)| (member(user_id), kind)), "what is taken is the lane's call");
    }

    #[test]
    fn whether_ones_own_screen_is_watched_is_told_by_its_event_and_by_a_snapshot() {
        let screen = i32::from(TrackKind::ScreenVideo);
        let ada = member(ADA);
        let told = |kind| steps(&booth(), Some(&ada), event(kind));
        let watched_as = |video: Video, steps: &[Step]| {
            steps.iter().find_map(|step| match step {
                Step::Watched(of, watched) if *of == video => Some(*watched),
                _ => None,
            })
        };
        let watched = |steps: Vec<Step>| watched_as(Video::Screen, &steps);
        let camera = i32::from(TrackKind::Camera);
        let camera_told = told(event::Kind::ShareWatched(ShareWatched { kind: camera, watched: true }));
        assert_eq!(
            (watched_as(Video::Camera, &camera_told), watched_as(Video::Screen, &camera_told)),
            (Some(true), None)
        );
        assert_eq!(watched(told(event::Kind::ShareWatched(ShareWatched { kind: screen, watched: true }))), Some(true));
        assert_eq!(
            watched(told(event::Kind::ShareWatched(ShareWatched { kind: screen, watched: false }))),
            Some(false)
        );
        let sound = i32::from(TrackKind::ScreenAudio);
        assert_eq!(watched(told(event::Kind::ShareWatched(ShareWatched { kind: sound, watched: true }))), None);
        let whole = |watched: Vec<i32>| RoomSnapshot { channel_id: BOOTH.into(), watched, ..Default::default() };
        assert_eq!(watched(told(event::Kind::RoomSnapshot(whole(vec![screen])))), Some(true));
        assert_eq!(watched(told(event::Kind::RoomSnapshot(whole(vec![sound])))), Some(false), "only the screen's own");
        let both = told(event::Kind::RoomSnapshot(whole(vec![screen])));
        assert_eq!(
            (watched_as(Video::Screen, &both), watched_as(Video::Camera, &both)),
            (Some(true), Some(false)),
            "a snapshot tells each of the two, so an unwatched camera is told unwatched"
        );
    }

    #[test]
    fn a_members_state_reaches_the_room() {
        let state = MemberState {
            channel_id: BOOTH.into(),
            user_id: ADA.into(),
            mic_muted: true,
            deafened: false,
            admin_muted: false,
        };
        assert_eq!(
            steps(&booth(), None, event(event::Kind::MemberState(state))),
            [
                Step::Room(RoomEvent::State {
                    user_id: member(ADA),
                    state: State { mic_muted: true, deafened: false }
                }),
                Step::Room(RoomEvent::Held { user_id: member(ADA), held: false }),
            ]
        );
    }

    #[test]
    fn a_mute_an_administrator_put_on_someone_reaches_the_room_as_theirs_and_as_held() {
        let state = MemberState {
            channel_id: BOOTH.into(),
            user_id: ADA.into(),
            mic_muted: true,
            deafened: false,
            admin_muted: true,
        };
        let steps = steps(&booth(), None, event(event::Kind::MemberState(state)));
        assert!(steps.contains(&Step::Room(RoomEvent::Held { user_id: member(ADA), held: true })), "{steps:?}");
        let summary = MemberSummary { user_id: ADA.into(), mic_muted: true, admin_muted: true, ..Default::default() };
        let who = person(&summary).expect("a person");
        assert!(who.held && who.state.mic_muted, "a person who joins held is shown held");
    }

    #[test]
    fn a_late_echo_or_a_snapshot_shows_what_the_person_wants() {
        const MUTED: State = State { mic_muted: true, deafened: false };
        let heard = State::default();
        let echo = RoomEvent::State { user_id: member(ADA), state: heard };
        assert_eq!(as_wanted(echo, Some(&member(ADA)), MUTED), RoomEvent::State { user_id: member(ADA), state: MUTED });
        let other = RoomEvent::State { user_id: member(OTHER), state: heard };
        assert_eq!(as_wanted(other.clone(), Some(&member(ADA)), MUTED), other, "another member's is the server's");
        let snapshot = RoomEvent::Joined {
            members: vec![screens::Person::new(member(ADA), "Ada"), screens::Person::new(member(OTHER), "Other")],
            streams: Vec::new(),
        };
        let RoomEvent::Joined { members, .. } = as_wanted(snapshot, Some(&member(ADA)), MUTED) else {
            panic!("a snapshot")
        };
        assert_eq!([members[0].state, members[1].state], [MUTED, heard]);
    }

    #[test]
    fn a_place_the_server_ended_keeps_its_reason() {
        let ends = |reason: CloseReason| {
            let over = VoiceClosed { channel_id: BOOTH.into(), reason: reason.into() };
            let mut steps = steps(&booth(), None, event(event::Kind::VoiceClosed(over)));
            let Some(Step::Closed(end)) = steps.pop() else { panic!("not an end") };
            end
        };
        assert_eq!(
            ends(CloseReason::TakenOut),
            End::Closed(Some(CloseReason::TakenOut)),
            "the server's reason, as said"
        );
        assert_eq!(ends(CloseReason::Moved), End::Closed(Some(CloseReason::Moved)));
        assert_eq!(ends(CloseReason::OverRate), End::Closed(Some(CloseReason::OverRate)));
    }

    #[test]
    fn a_screens_sound_the_server_cannot_carry_is_said_on_its_card_and_other_refusals_are_not() {
        use proto::chatkord_v1::ErrorCode;
        use terms::LinkError;
        let busy = || VoiceRefused::Link(LinkError::Refused(ErrorCode::ServerUnavailable));
        assert_eq!(share_sound_unheard(TrackKind::ScreenAudio, busy()), Some(Failure::ShareSoundNotCarried));
        assert_eq!(share_sound_unheard(TrackKind::Microphone, busy()), None, "a voice is not a screen's sound");
        assert_eq!(share_sound_unheard(TrackKind::ScreenAudio, VoiceRefused::Engine), None, "the log's to say");
    }

    #[test]
    fn the_place_is_given_back_after_a_leave_or_a_failed_setup_and_never_after_the_server_or_the_link_ended_it() {
        assert!(gives_back(&Over::Left));
        assert!(gives_back(&Over::Failed(Failure::VoiceOffer)));
        assert!(gives_back(&Over::Fault));
        assert!(!gives_back(&Over::Closed(End::Closed(Some(CloseReason::Moved)))));
        assert!(!gives_back(&Over::Link(Ended::default())));
    }

    #[tokio::test]
    async fn a_step_that_panics_is_told_as_a_fault() {
        let mut lost = Task::<()>::spawn(async { panic!("a step that does not come back") });
        let lost = lost.answer().await;
        assert_eq!(lost, Err(Panicked));
        assert!(gives_back(&Over::Fault), "the place is still held");
        assert_eq!(Stopped::Fault.sentence(), words::failure::of(&Failure::CallFaulted));
    }

    #[test]
    fn what_is_not_the_rooms_is_passed_on_whatever_channel_the_call_is_in() {
        let changed = event(event::Kind::ChannelRenamed(ChannelRenamed::default()));
        assert_eq!(steps(&booth(), None, changed.clone()), [Step::Heard(changed)]);
    }

    #[test]
    fn a_lane_is_starting_until_first_up_mending_while_its_ice_restarts_and_up_otherwise() {
        use screens::Lane as Told;
        assert_eq!(lane_of(Reach::Starting, false), Told::Starting);
        assert_eq!(lane_of(Reach::Up, false), Told::Up);
        assert_eq!(lane_of(Reach::Down, false), Told::Up, "the engine is still trying the route by itself");
        assert_eq!(lane_of(Reach::Down, true), Told::Mending);
        assert_eq!(lane_of(Reach::Lost, true), Told::Mending);
        assert_eq!(lane_of(Reach::Up, true), Told::Mending, "until the next look ends the mending");
    }

    #[test]
    fn what_an_act_comes_to_is_decided_after_the_acts_of_the_decision_that_asked_for_it() {
        let mut log: Vec<&str> = Vec::new();
        let ended = in_order(
            &mut log,
            Input::Voice(VoiceInput::SettleFailed),
            |log, input| match input {
                Input::Voice(VoiceInput::SettleFailed) => {
                    log.push("decide first");
                    vec![VoiceAct::MuteMic.into(), VoiceAct::DropGlance.into()]
                }
                _ => {
                    log.push("decide second");
                    Vec::new()
                }
            },
            |log, act| match act {
                Act::Voice(VoiceAct::MuteMic) => {
                    log.push("mute");
                    Carried::Then(Box::new(Input::Voice(VoiceInput::Asked(Ok(())))))
                }
                _ => {
                    log.push("drop glance");
                    Carried::Done
                }
            },
        );
        assert!(ended.is_none());
        assert_eq!(log, ["decide first", "mute", "drop glance", "decide second"], "the rest of the acts come first");
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_told_the_window_is_hidden_ticks_for_no_levels_and_still_tells_its_lanes() {
        const EVERY: Duration = Duration::from_millis(100);
        let now = Instant::now();
        let mut state = decide::State::new(decide::Setup {
            me: Some(member(ADA)),
            channel_id: booth(),
            source: Source::Microphone,
            jitter: screens::Jitter::from_bits(1 << 63),
            shows: true,
            has_devices: true,
            start: State::default(),
            held: Hold::Free,
            microphone: None,
            speaker: None,
            unseen: Vec::new(),
            paused: Vec::new(),
            voice_at: ("voice.example.invalid".to_string(), 40_000),
        });
        let acts = decide::decide(&mut state, Input::Voice(VoiceInput::Commands(vec![Command::Levels(false)])), now);
        assert!(acts.iter().any(|act| matches!(act, Act::Voice(VoiceAct::Levels(false)))), "the tick is stopped");
        let mut levels = Levelling::new(EVERY, now);
        levels.show(false, now);
        assert!(tokio::time::timeout(EVERY * 10, levels.tick()).await.is_err(), "hidden: no loudness is read");

        let wire = Wire { send: Reach::Up, receive: Reach::Up, screen: false, camera: false };
        let beat = Input::Wire(WireInput::Beat(Beat { wire, chain: None, restarting: false, capturing: false }));
        let acts = decide::decide(&mut state, beat, Instant::now());
        let lanes = |act: &Act| matches!(act, Act::Report(Report::Room(RoomEvent::Lanes { .. })));
        assert!(acts.iter().any(lanes), "the look at the call tells the lanes, hidden or not");

        levels.show(true, Instant::now());
        assert!(tokio::time::timeout(EVERY * 2, levels.tick()).await.is_ok(), "shown again: read again");
    }

    #[tokio::test]
    async fn an_ask_that_comes_while_one_is_out_waits_behind_it_and_the_newest_alone_is_made_after() {
        let made = std::cell::RefCell::new(Vec::new());
        let start = |state: u8| -> Flight<u8> {
            made.borrow_mut().push(state);
            Box::pin(async move { state })
        };
        let mut asking = Asking::<u8, u8>::default();
        asking.ask(1, start);
        asking.ask(2, start);
        asking.ask(3, start);
        assert_eq!(*made.borrow(), [1], "the first is out; the others have not been made");
        assert_eq!(asking.answered(start).await, 1, "the first is answered, not dropped");
        assert_eq!(*made.borrow(), [1, 3], "the newest waiting is made once it is answered");
        assert_eq!(asking.answered(start).await, 3);
        assert_eq!(*made.borrow(), [1, 3], "nothing more is out");
    }
}
