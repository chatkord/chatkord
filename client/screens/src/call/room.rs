//! A voice channel's room as the window shows it: who is in it, what each person is sending, and who is being
//! heard, from what the server says and the engine measures, as the tiles to draw. Each event's time is handed in.

use crate::call::voice::{Connection, Tile};
use crate::chime::Chime;
use crate::closed::Then;
use crate::people::colour_of;
use choices::State;
use proto::chatkord_v1::{CloseReason, ErrorCode};
use proto::rules::{Colour, Grant, Grants};
use std::time::{Duration, Instant};
use terms::MemberId;
use terms::{Ended, LinkError};
use words::Phrase;
use words::Sentence;
use words::failure::{self, Failure};

/// A loudness (0 to 100, as the engine measures it) at which a person is being heard.
pub(crate) const SPEAKING_LEVEL: u8 = 10;

/// How long a person stays heard after their last loud moment, so a pause between words does not flicker a ring.
pub(crate) const HOLD: Duration = Duration::from_millis(400);

/// What a person can send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// Their microphone's sound.
    Microphone,
    /// The picture of the screen they share.
    ScreenVideo,
    /// The sound shared with their screen: part of the share, with no tile or word of its own.
    ScreenAudio,
    /// Their camera's picture, shown on their own tile.
    Camera,
}

/// How one of this account's two connections to the call is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lane {
    /// It has not connected for the first time yet.
    #[default]
    Starting,
    /// It carries the call.
    Up,
    /// It lost its route and is being made again.
    Mending,
}

/// What became of this call's take of a member's stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// The server was asked for it; asked again after a refusal, it is on its way anew.
    Asked,
    /// It arrives.
    Taken,
    /// It was refused, and stays so until it is asked for again.
    Refused,
}

/// This call's take of a member's microphone, as their tile says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Voice {
    /// On its way since then.
    Asked(Instant),
    Taken,
    Refused,
}

/// A person in the room, as the server names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    /// Their account's id.
    pub user_id: MemberId,
    /// The name others see: the one they chose, else their username.
    pub name: String,
    /// The colour they picked, if any.
    pub colour: Option<Colour>,
    /// What they did about hearing and being heard.
    pub state: State,
    /// An administrator muted them for everyone: they stay muted and cannot unmute themselves.
    pub held: bool,
}

impl Person {
    /// Someone with no colour picked, neither muted nor deafened nor held.
    pub fn new(user_id: MemberId, name: impl Into<String>) -> Person {
        Person { user_id, name: name.into(), colour: None, state: State::default(), held: false }
    }
}

/// Why a call is over, so a screen can offer what fits beside the sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// This account joined voice from another device: it can take the seat back.
    Moved,
    /// The channel was deleted.
    Deleted,
    /// The account's roles no longer allow the channel.
    RolesChanged,
    /// An administrator took the account out of the channel: it may join again.
    TakenOut,
    /// The server closed the place for another reason, or the connection ended.
    Closed,
    /// The call could not be set up or kept.
    Failed,
    /// The window could not keep up with the room.
    FellBehind,
}

/// How a call ended, as its owner found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// The server ended this account's place, with its reason when this build knows it.
    Closed(Option<CloseReason>),
    /// The connection ended.
    Link(Ended),
    /// The server could not be asked what the call needed.
    Unreached(LinkError),
    /// The call could not be set up or kept.
    Failed(Failure),
    /// The window could not keep up with the room.
    FellBehind,
}

impl End {
    /// Why it is over, for what a screen offers beside the sentence.
    pub fn why(&self) -> Why {
        match self {
            End::Closed(Some(reason)) => match Then::of(*reason) {
                Then::LeftVoice(why) => why,
                Then::TryAgain | Then::SignIn | Then::AccountGone | Then::OtherVersion => Why::Closed,
            },
            End::Closed(None) | End::Link(_) => Why::Closed,
            End::Unreached(_) | End::Failed(_) => Why::Failed,
            End::FellBehind => Why::FellBehind,
        }
    }

    /// What a person reads of it.
    pub fn sentence(&self) -> Sentence {
        match self {
            End::Closed(reason) => {
                Sentence::Phrase(reason.and_then(failure::closed).unwrap_or(Phrase::VoiceChannelClosed))
            }
            End::Link(gone) => failure::ended(gone),
            End::Unreached(failed) => failure::said(failed),
            End::Failed(failed) => failure::of(failed),
            End::FellBehind => Sentence::Phrase(Phrase::FellBehind),
        }
    }

    /// What a person holding `grants` reads of it: a join refused for want of the server's voice address tells one who
    /// may set it where to.
    pub fn said_to(&self, grants: Grants) -> Sentence {
        match self {
            End::Unreached(LinkError::Refused(ErrorCode::NoVoiceAddress)) if grants.holds(Grant::ManageServer) => {
                Sentence::Phrase(Phrase::RefusedNoVoiceAddressManage)
            }
            End::Closed(_) | End::Link(_) | End::Unreached(_) | End::Failed(_) | End::FellBehind => self.sentence(),
        }
    }
}

/// What the host tells the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The room whole: on joining, or after falling behind its events. The people there and the streams live
    /// replace what was known; anyone or anything left out is gone.
    Joined {
        /// Everyone in the room, this account among them.
        members: Vec<Person>,
        /// Each stream live, by its sender's account id.
        streams: Vec<(MemberId, Track)>,
    },
    /// Someone muted or deafened themselves, or undid it.
    State {
        /// Their account's id.
        user_id: MemberId,
        /// What they did, whole.
        state: State,
    },
    /// An administrator muted someone for everyone, or lifted it.
    Held {
        /// Their account's id.
        user_id: MemberId,
        /// Whether they are held muted now.
        held: bool,
    },
    /// Someone came into the room.
    MemberJoined {
        /// Their account's id.
        user_id: MemberId,
        /// The name others see: the one they chose, else their username.
        name: String,
        /// The colour they picked, if any.
        colour: Option<Colour>,
    },
    /// Someone left the room, with whatever they sent.
    MemberLeft {
        /// Their account's id.
        user_id: MemberId,
    },
    /// Someone chose the name others see, or went back to their username.
    Renamed {
        /// Their account's id.
        user_id: MemberId,
        /// The name others see now.
        name: String,
    },
    /// Someone picked the colour others see them in, or went back to none.
    Recoloured {
        /// Their account's id.
        user_id: MemberId,
        /// The colour they picked; none when they went back to none.
        colour: Option<Colour>,
    },
    /// Someone began sending a stream.
    Started {
        /// The sender's account id.
        user_id: MemberId,
        /// What they began sending.
        track: Track,
    },
    /// A stream ended, from the server's word that it did, never from a timer.
    Stopped {
        /// The sender's account id.
        user_id: MemberId,
        /// What they stopped sending.
        track: Track,
    },
    /// The loudness of each person being heard, now.
    Levels(Vec<(MemberId, u8)>),
    /// How each of this account's two connections to the call is.
    Lanes {
        /// The one this account sends on.
        send: Lane,
        /// The one everyone else is heard and seen on.
        receive: Lane,
    },
    /// What became of this call's take of a member's stream.
    Taking {
        /// The sender's account id.
        user_id: MemberId,
        /// Which of their streams.
        track: Track,
        /// What became of it.
        take: Take,
    },
    /// The room is over for this connection.
    Ended(End),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    user_id: MemberId,
    /// Absent until the server says it: a stream may be announced before its sender's arrival is.
    name: Option<String>,
    colour: Option<Colour>,
    microphone: bool,
    /// This call's take of that microphone; none before the call says, and for this account's own.
    voice: Option<Voice>,
    screen: bool,
    /// Their share carries sound, which plays at its own volume.
    screen_sound: bool,
    camera: bool,
    state: State,
    held: bool,
    last_loud: Option<Instant>,
}

impl Member {
    /// The name the server gave, or its id until it gives one, as a tile shows it.
    fn shown_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.user_id.as_str().to_owned())
    }

    /// Muted, deafened, held by an administrator or sending no microphone: no sound comes from them.
    fn silent(&self) -> bool {
        self.state.mic_muted || self.state.deafened || self.held || !self.microphone
    }
}

/// Who is in a voice channel's room and what each sends, as the host's events told it, until the room ends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Room {
    /// The account that looks, so its tile is first and its own.
    me: Option<MemberId>,
    joined: bool,
    members: Vec<Member>,
    send: Lane,
    receive: Lane,
    ended: Option<End>,
}

impl Room {
    /// A room nobody has joined yet, seen by the account `me`.
    pub fn new(me: Option<MemberId>) -> Room {
        Room { me, ..Room::default() }
    }

    /// What `user_id` has done about hearing and being heard: the server's word, but this account's own is the call's,
    /// what the person pressed.
    pub fn state_of(&self, user_id: &MemberId) -> State {
        self.members.iter().find(|member| &member.user_id == user_id).map(|member| member.state).unwrap_or_default()
    }

    /// Whether an administrator has `user_id` muted for everyone, as the server last said.
    pub fn held(&self, user_id: &MemberId) -> bool {
        self.members.iter().any(|member| member.user_id == *user_id && member.held)
    }

    /// Whether `user_id`'s share carries sound, as the server last said.
    pub fn shares_sound(&self, user_id: &MemberId) -> bool {
        self.members.iter().any(|member| member.user_id == *user_id && member.screen_sound)
    }

    /// Whether the snapshot of the room has arrived.
    pub fn joined(&self) -> bool {
        self.joined
    }

    /// How the room ended, once it has.
    pub fn ended(&self) -> Option<&End> {
        self.ended.as_ref()
    }

    /// Why the room is over, once it is.
    pub fn ended_why(&self) -> Option<Why> {
        self.ended.as_ref().map(End::why)
    }

    fn member(&mut self, user_id: &MemberId) -> &mut Member {
        let at = match self.members.iter().position(|member| &member.user_id == user_id) {
            Some(at) => at,
            None => {
                self.members.push(Member {
                    user_id: user_id.clone(),
                    name: None,
                    colour: None,
                    microphone: false,
                    voice: None,
                    screen: false,
                    screen_sound: false,
                    camera: false,
                    state: State::default(),
                    held: false,
                    last_loud: None,
                });
                self.members.len() - 1
            }
        };
        &mut self.members[at]
    }

    /// Takes in what the host tells it, `now` being when it arrived (a loud level holds a ring from then), and says
    /// the moment it marks with a sound: this account's own first snapshot, someone else coming or going, a screen
    /// share starting or stopping. A later snapshot sounds nothing, nor do the people and shares it lists.
    pub fn on(&mut self, event: Event, now: Instant) -> Option<Chime> {
        match event {
            Event::Joined { members, streams } => {
                let first = !self.joined;
                self.joined = true;
                self.members.retain(|member| members.iter().any(|person| person.user_id == member.user_id));
                for member in &mut self.members {
                    (member.microphone, member.screen, member.screen_sound, member.camera) =
                        (false, false, false, false);
                }
                for person in members {
                    let member = self.member(&person.user_id);
                    (member.name, member.colour, member.state, member.held) =
                        (Some(person.name), person.colour, person.state, person.held);
                }
                for (user_id, track) in streams {
                    self.set(&user_id, track, true);
                }
                for member in &mut self.members {
                    member.voice = member.voice.filter(|_| member.microphone);
                }
                first.then_some(Chime::Joined)
            }
            Event::State { user_id, state } => {
                self.member(&user_id).state = state;
                None
            }
            Event::Held { user_id, held } => {
                self.member(&user_id).held = held;
                None
            }
            Event::MemberJoined { user_id, name, colour } => {
                let arrives = self.joined && !self.is_here(&user_id) && !self.is_me(&user_id);
                let member = self.member(&user_id);
                (member.name, member.colour) = (Some(name), colour);
                arrives.then_some(Chime::MemberJoined)
            }
            Event::MemberLeft { user_id } => {
                let leaves = self.is_here(&user_id) && !self.is_me(&user_id);
                self.members.retain(|member| member.user_id != user_id);
                leaves.then_some(Chime::MemberLeft)
            }
            // Said to every signed-in session: only someone in the room is renamed here.
            Event::Renamed { user_id, name } => {
                if let Some(member) = self.members.iter_mut().find(|member| member.user_id == user_id) {
                    member.name = Some(name);
                }
                None
            }
            Event::Recoloured { user_id, colour } => {
                if let Some(member) = self.members.iter_mut().find(|member| member.user_id == user_id) {
                    member.colour = colour;
                }
                None
            }
            Event::Started { user_id, track } => {
                let starts = track == Track::ScreenVideo && !self.sends(&user_id, track);
                self.set(&user_id, track, true);
                starts.then(|| if self.is_me(&user_id) { Chime::ShareStarted } else { Chime::MemberShareStarted })
            }
            Event::Stopped { user_id, track } => {
                let stops = track == Track::ScreenVideo && self.sends(&user_id, track);
                self.set(&user_id, track, false);
                stops.then(|| if self.is_me(&user_id) { Chime::ShareStopped } else { Chime::MemberShareStopped })
            }
            Event::Levels(levels) => {
                for (user_id, level) in levels {
                    if level >= SPEAKING_LEVEL
                        && let Some(member) = self.members.iter_mut().find(|member| member.user_id == user_id)
                    {
                        member.last_loud = Some(now);
                    }
                }
                None
            }
            Event::Lanes { send, receive } => {
                (self.send, self.receive) = (send, receive);
                None
            }
            // A tile says only of a voice, and of one the room still lists.
            Event::Taking { user_id, track: Track::Microphone, take } => {
                if let Some(member) =
                    self.members.iter_mut().find(|member| member.user_id == user_id && member.microphone)
                {
                    member.voice = Some(match (take, member.voice) {
                        (Take::Asked, Some(asked @ Voice::Asked(_))) => asked,
                        (Take::Asked, _) => Voice::Asked(now),
                        (Take::Taken, _) => Voice::Taken,
                        (Take::Refused, _) => Voice::Refused,
                    });
                }
                None
            }
            Event::Taking { track: Track::ScreenVideo | Track::ScreenAudio | Track::Camera, .. } => None,
            Event::Ended(end) => {
                self.ended = Some(end);
                None
            }
        }
    }

    fn is_here(&self, user_id: &MemberId) -> bool {
        self.members.iter().any(|member| &member.user_id == user_id)
    }

    /// Whether `user_id` is here and sending `track`.
    fn sends(&self, user_id: &MemberId, track: Track) -> bool {
        self.members.iter().find(|member| &member.user_id == user_id).is_some_and(|member| match track {
            Track::Microphone => member.microphone,
            Track::ScreenVideo => member.screen,
            Track::ScreenAudio => member.screen_sound,
            Track::Camera => member.camera,
        })
    }

    fn is_me(&self, user_id: &MemberId) -> bool {
        self.me.as_ref() == Some(user_id)
    }

    fn set(&mut self, user_id: &MemberId, track: Track, on: bool) {
        let member = self.member(user_id);
        match track {
            Track::Microphone => {
                member.microphone = on;
                member.voice = member.voice.filter(|_| on);
            }
            Track::ScreenVideo => member.screen = on,
            // A share's sound is part of the share: it adds no tile and no word of its own, only a volume.
            Track::ScreenAudio => member.screen_sound = on,
            Track::Camera => member.camera = on,
        }
    }

    /// The name `user_id` is shown by, as its tile shows it; none for someone not in the room.
    pub(crate) fn shown_name(&self, user_id: &MemberId) -> Option<String> {
        self.members.iter().find(|member| &member.user_id == user_id).map(Member::shown_name)
    }

    /// How this account's own tile says its connection is.
    fn own_connection(&self) -> Connection {
        match (self.send, self.receive) {
            (Lane::Mending, _) | (_, Lane::Mending) => Connection::Reconnecting,
            (Lane::Starting, _) => Connection::Connecting,
            (Lane::Up, _) => Connection::Connected,
        }
    }

    /// How another member's tile says this call's hold on their voice is at `now`; someone sending none has none
    /// to wait for.
    fn connection_to(&self, member: &Member, now: Instant) -> Connection {
        match member.voice {
            Some(Voice::Refused) => Connection::Failed,
            Some(Voice::Asked(_) | Voice::Taken) if self.receive == Lane::Mending => Connection::Reconnecting,
            Some(Voice::Asked(since)) if now.saturating_duration_since(since) > design::CONNECTING_AFTER => {
                Connection::Connecting
            }
            Some(Voice::Asked(_) | Voice::Taken) | None => Connection::Connected,
        }
    }

    /// The tiles to draw at `now`: this account's first, then the others in the order they arrived.
    pub fn tiles(&self, now: Instant) -> Vec<Tile> {
        let own = |member: &&Member| self.me.as_ref() == Some(&member.user_id);
        self.members
            .iter()
            .filter(own)
            .chain(self.members.iter().filter(|member| !own(member)))
            .map(|member| {
                let heard = member.last_loud.is_some_and(|at| now.saturating_duration_since(at) <= HOLD);
                Tile {
                    user_id: Some(member.user_id.clone()),
                    held: member.held,
                    name: member.shown_name(),
                    colour: colour_of(&member.user_id, member.colour),
                    own: own(&member),
                    connection: if own(&member) { self.own_connection() } else { self.connection_to(member, now) },
                    speaking: heard && member.microphone,
                    muted: member.silent(),
                    deafened: member.state.deafened,
                    sharing: member.screen,
                    camera: if member.camera { crate::CameraSeen::Shown } else { crate::CameraSeen::Off },
                    elsewhere: false,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::member;

    #[test]
    fn a_place_the_server_ended_says_why_it_ended() {
        let ends = |reason| End::Closed(Some(reason));
        assert_eq!(ends(CloseReason::TakenOut).why(), Why::TakenOut);
        assert_eq!(ends(CloseReason::TakenOut).sentence().in_english(), "An admin took you out of the voice channel.");
        assert_eq!(ends(CloseReason::OverRate).why(), Why::Closed);
        assert_eq!(
            ends(CloseReason::OverRate).sentence().in_english(),
            "This device sent more than the server allows, so it left the voice channel."
        );
        assert_eq!(ends(CloseReason::Moved).why(), Why::Moved);
        assert_eq!(ends(CloseReason::Moved).sentence().in_english(), "This account joined voice from another device.");
        assert_eq!(ends(CloseReason::ChannelDeleted).why(), Why::Deleted);
        assert_eq!(ends(CloseReason::NotAllowed).why(), Why::RolesChanged);
        for other in [ends(CloseReason::Normal), End::Closed(None)] {
            assert_eq!(
                (other.why(), other.sentence().in_english()),
                (Why::Closed, "The voice channel was closed.".into())
            );
        }
        let gone = End::Link(Ended { reason: None, refusal: None, silent: true, cursor: None });
        assert_eq!((gone.why(), gone.sentence().in_english()), (Why::Closed, "The server stopped answering.".into()));
        assert_eq!(End::Unreached(LinkError::Timeout).why(), Why::Failed);
    }

    #[test]
    fn a_call_refused_for_want_of_a_voice_address_tells_who_may_set_it_where() {
        let refused = End::Unreached(LinkError::Refused(ErrorCode::NoVoiceAddress));
        let manager = Grants::of(Grant::ManageServer);
        assert_eq!(refused.said_to(Grants::NONE), Sentence::Phrase(Phrase::RefusedNoVoiceAddress));
        assert_eq!(refused.said_to(manager), Sentence::Phrase(Phrase::RefusedNoVoiceAddressManage));
        let other = End::Unreached(LinkError::Refused(ErrorCode::NotAuthorized));
        assert_eq!(other.said_to(manager), other.sentence(), "only the missing address is the manager's to fix");
    }

    const ADA: &str = "ada";
    const GRACE: &str = "grace";
    const LIN: &str = "lin";

    pub(crate) fn named(user: &str, name: &str) -> Person {
        Person::new(member(user), name)
    }

    fn joined(now: Instant) -> Room {
        let mut room = Room::new(None);
        room.on(
            Event::Joined {
                members: vec![named(ADA, "Ada"), named(GRACE, "Grace")],
                streams: vec![(member(ADA), Track::Microphone)],
            },
            now,
        );
        room
    }

    #[test]
    fn a_later_snapshot_replaces_who_is_there_and_what_they_send() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(
            Event::Joined { members: vec![named(GRACE, "Grace")], streams: vec![(member(GRACE), Track::ScreenVideo)] },
            now,
        );
        let tiles = room.tiles(now);
        assert_eq!(tiles.len(), 1, "ada was left out, so she is gone");
        assert_eq!((tiles[0].name.as_str(), tiles[0].muted, tiles[0].sharing), ("Grace", true, true));
    }

    const UP: Event = Event::Lanes { send: Lane::Up, receive: Lane::Up };

    fn connections(room: &Room, now: Instant) -> Vec<Connection> {
        room.tiles(now).iter().map(|tile| tile.connection).collect()
    }

    /// Ada looks; Grace sends a microphone, and both lanes are up.
    fn ada_hears_grace(now: Instant) -> Room {
        let mut room = Room::new(Some(member(ADA)));
        room.on(
            Event::Joined {
                members: vec![named(ADA, "Ada"), named(GRACE, "Grace")],
                streams: vec![(member(GRACE), Track::Microphone)],
            },
            now,
        );
        room.on(UP, now);
        room
    }

    fn taking(take: Take) -> Event {
        Event::Taking { user_id: member(GRACE), track: Track::Microphone, take }
    }

    #[test]
    fn ones_own_tile_says_connecting_until_its_send_lane_is_up_and_nobody_elses_does() {
        let now = Instant::now();
        let mut room = Room::new(Some(member(ADA)));
        room.on(Event::Joined { members: vec![named(ADA, "Ada"), named(GRACE, "Grace")], streams: vec![] }, now);
        assert_eq!(connections(&room, now), [Connection::Connecting, Connection::Connected]);
        room.on(Event::Lanes { send: Lane::Starting, receive: Lane::Up }, now);
        assert_eq!(connections(&room, now), [Connection::Connecting, Connection::Connected]);
        room.on(Event::Lanes { send: Lane::Up, receive: Lane::Starting }, now);
        assert_eq!(connections(&room, now), [Connection::Connected, Connection::Connected]);
    }

    #[test]
    fn ones_own_tile_says_its_connection_is_made_again_while_either_lane_mends() {
        let now = Instant::now();
        let mut room = Room::new(Some(member(ADA)));
        room.on(Event::Joined { members: vec![named(ADA, "Ada"), named(GRACE, "Grace")], streams: vec![] }, now);
        room.on(UP, now);
        assert_eq!(connections(&room, now), [Connection::Connected, Connection::Connected]);
        for (send, receive) in [(Lane::Mending, Lane::Up), (Lane::Up, Lane::Mending), (Lane::Starting, Lane::Mending)] {
            room.on(Event::Lanes { send, receive }, now);
            assert_eq!(
                connections(&room, now),
                [Connection::Reconnecting, Connection::Connected],
                "{send:?} {receive:?}: Grace sends no voice to lose"
            );
        }
        room.on(UP, now);
        assert_eq!(connections(&room, now), [Connection::Connected, Connection::Connected]);
    }

    #[test]
    fn anothers_tile_says_connecting_only_once_their_voice_has_been_asked_for_longer_than_a_moment() {
        let start = Instant::now();
        let mut room = ada_hears_grace(start);
        assert_eq!(connections(&room, start)[1], Connection::Connected, "before the call says anything of it");
        room.on(taking(Take::Asked), start);
        assert_eq!(connections(&room, start + design::CONNECTING_AFTER)[1], Connection::Connected);
        let late = start + design::CONNECTING_AFTER + Duration::from_millis(1);
        assert_eq!(connections(&room, late), [Connection::Connected, Connection::Connecting]);
        room.on(taking(Take::Asked), late);
        assert_eq!(connections(&room, late)[1], Connection::Connecting, "asked twice, it waits since the first");
        room.on(taking(Take::Taken), late);
        assert_eq!(connections(&room, late)[1], Connection::Connected);
    }

    #[test]
    fn anothers_tile_says_failed_while_their_voice_stands_refused_and_waits_anew_when_asked_again() {
        let start = Instant::now();
        let mut room = ada_hears_grace(start);
        room.on(taking(Take::Asked), start);
        room.on(taking(Take::Refused), start);
        assert_eq!(connections(&room, start), [Connection::Connected, Connection::Failed]);
        room.on(Event::Lanes { send: Lane::Up, receive: Lane::Mending }, start);
        assert_eq!(connections(&room, start)[1], Connection::Failed, "a refusal is said before a lane that mends");
        room.on(UP, start);
        let again = start + Duration::from_secs(8);
        room.on(taking(Take::Asked), again);
        assert_eq!(connections(&room, again)[1], Connection::Connected, "the moment starts over");
        assert_eq!(connections(&room, again + Duration::from_secs(2))[1], Connection::Connecting);
        room.on(taking(Take::Taken), again);
        assert_eq!(connections(&room, again + Duration::from_secs(2))[1], Connection::Connected);
    }

    #[test]
    fn anothers_tile_says_reconnecting_while_the_receive_lane_mends_and_only_if_their_voice_is_taken() {
        let now = Instant::now();
        let mut room = ada_hears_grace(now);
        room.on(Event::MemberJoined { user_id: member(LIN), name: "Lin".into(), colour: None }, now);
        room.on(taking(Take::Taken), now);
        room.on(Event::Lanes { send: Lane::Mending, receive: Lane::Up }, now);
        assert_eq!(connections(&room, now)[1], Connection::Connected, "the send lane carries nobody else");
        room.on(Event::Lanes { send: Lane::Up, receive: Lane::Mending }, now);
        assert_eq!(
            connections(&room, now),
            [Connection::Reconnecting, Connection::Reconnecting, Connection::Connected],
            "Lin sends no microphone"
        );
        room.on(taking(Take::Asked), now);
        assert_eq!(connections(&room, now)[1], Connection::Reconnecting, "asked again while it mends");
        room.on(UP, now);
        room.on(taking(Take::Taken), now);
        assert_eq!(connections(&room, now)[1], Connection::Connected);
    }

    #[test]
    fn a_take_is_forgotten_when_the_microphone_stops_its_sender_leaves_or_a_snapshot_leaves_it_out() {
        let now = Instant::now();
        let failed = |room: &Room| connections(room, now).contains(&Connection::Failed);
        let refused = || {
            let mut room = ada_hears_grace(now);
            room.on(taking(Take::Refused), now);
            assert!(failed(&room));
            room
        };
        let mut room = refused();
        room.on(Event::Stopped { user_id: member(GRACE), track: Track::Microphone }, now);
        assert!(!failed(&room));
        room.on(Event::Started { user_id: member(GRACE), track: Track::Microphone }, now);
        assert!(!failed(&room), "a microphone started again has not been asked for yet");

        let mut room = refused();
        room.on(Event::MemberLeft { user_id: member(GRACE) }, now);
        room.on(Event::MemberJoined { user_id: member(GRACE), name: "Grace".into(), colour: None }, now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::Microphone }, now);
        assert!(!failed(&room));

        let members = || vec![named(ADA, "Ada"), named(GRACE, "Grace")];
        let mut room = refused();
        room.on(Event::Joined { members: members(), streams: vec![(member(GRACE), Track::Microphone)] }, now);
        assert!(failed(&room), "a snapshot that still lists the microphone keeps what became of it");
        room.on(Event::Joined { members: members(), streams: vec![] }, now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::Microphone }, now);
        assert!(!failed(&room));
    }

    #[test]
    fn a_take_of_a_picture_or_of_a_voice_the_room_does_not_list_changes_no_tile() {
        let now = Instant::now();
        let mut room = ada_hears_grace(now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::Camera }, now);
        room.on(Event::Taking { user_id: member(GRACE), track: Track::Camera, take: Take::Refused }, now);
        room.on(Event::Taking { user_id: member(LIN), track: Track::Microphone, take: Take::Refused }, now);
        assert_eq!(connections(&room, now), [Connection::Connected, Connection::Connected]);
    }

    #[test]
    fn a_rename_reaches_the_tile_of_someone_in_the_room_and_adds_nobody() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(Event::Renamed { user_id: member(GRACE), name: "Grace Hopper".into() }, now);
        room.on(Event::Renamed { user_id: member("linus"), name: "Linus".into() }, now);
        let names: Vec<String> = room.tiles(now).into_iter().map(|tile| tile.name).collect();
        assert_eq!(names, ["Ada", "Grace Hopper"]);
    }

    #[test]
    fn the_snapshot_of_the_room_is_who_is_there_and_what_they_already_send() {
        let now = Instant::now();
        let room = joined(now);
        assert!(room.joined());
        let tiles = room.tiles(now);
        assert_eq!(tiles.iter().map(|tile| tile.name.as_str()).collect::<Vec<_>>(), ["Ada", "Grace"]);
        assert!(!tiles[0].muted && tiles[1].muted, "Ada sends a microphone, Grace does not");
    }

    #[test]
    fn people_come_and_go_in_the_order_they_arrived_and_never_twice() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(Event::MemberJoined { user_id: member("lin"), name: "Lin".into(), colour: None }, now);
        room.on(Event::MemberJoined { user_id: member("lin"), name: "Lin".into(), colour: None }, now);
        assert_eq!(room.tiles(now).len(), 3);
        room.on(Event::MemberLeft { user_id: member(ADA) }, now);
        assert_eq!(room.tiles(now).iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Grace", "Lin"]);
        room.on(Event::MemberLeft { user_id: member("nobody") }, now);
        assert_eq!(room.tiles(now).len(), 2);
    }

    #[test]
    fn a_stream_starting_and_ending_is_the_servers_word_for_muted_and_sharing() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::Microphone }, now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::ScreenVideo }, now);
        room.on(Event::Started { user_id: member(GRACE), track: Track::ScreenAudio }, now);
        let grace = room.tiles(now).remove(1);
        assert!(!grace.muted && grace.sharing);
        room.on(Event::Stopped { user_id: member(GRACE), track: Track::ScreenVideo }, now);
        room.on(Event::Stopped { user_id: member(ADA), track: Track::Microphone }, now);
        let tiles = room.tiles(now);
        assert!(!tiles[1].sharing, "the share ended");
        assert!(tiles[0].muted, "Ada stopped sending");
    }

    #[test]
    fn a_shares_sound_is_known_from_its_start_to_its_stop_and_from_a_snapshot() {
        let now = Instant::now();
        let mut room = joined(now);
        assert!(!room.shares_sound(&member(GRACE)));
        room.on(Event::Started { user_id: member(GRACE), track: Track::ScreenAudio }, now);
        assert!(room.shares_sound(&member(GRACE)));
        assert!(!room.shares_sound(&member(ADA)), "a share's sound is its sharer's alone");
        room.on(Event::Stopped { user_id: member(GRACE), track: Track::ScreenAudio }, now);
        assert!(!room.shares_sound(&member(GRACE)), "its sound stopped");
        room.on(Event::Started { user_id: member(GRACE), track: Track::ScreenAudio }, now);
        let snapshot = Event::Joined {
            members: vec![named(ADA, "Ada"), named(GRACE, "Grace")],
            streams: vec![(member(ADA), Track::ScreenAudio)],
        };
        room.on(snapshot, now);
        assert!(
            room.shares_sound(&member(ADA)) && !room.shares_sound(&member(GRACE)),
            "a snapshot says anew what each sends"
        );
    }

    #[test]
    fn a_person_is_heard_while_loud_and_for_a_moment_after_and_not_when_quiet() {
        let start = Instant::now();
        let mut room = joined(start);
        room.on(Event::Levels(vec![(member(ADA), 5)]), start);
        assert!(!room.tiles(start)[0].speaking, "below the level is not speech");
        room.on(Event::Levels(vec![(member(ADA), SPEAKING_LEVEL)]), start);
        assert!(room.tiles(start)[0].speaking);
        assert!(room.tiles(start + HOLD)[0].speaking, "held through a pause");
        assert!(!room.tiles(start + HOLD + Duration::from_millis(1))[0].speaking, "and not for ever");
    }

    #[test]
    fn only_someone_sending_a_microphone_can_be_heard_and_a_stranger_is_ignored() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(Event::Levels(vec![(member(GRACE), 90), (member("stranger"), 90)]), now);
        let tiles = room.tiles(now);
        assert!(!tiles[1].speaking, "Grace sends no microphone");
        assert_eq!(tiles.len(), 2, "a level names no one new");
    }

    #[test]
    fn a_stream_announced_before_its_sender_is_kept_and_named_when_they_arrive() {
        let now = Instant::now();
        let mut room = Room::new(None);
        room.on(Event::Started { user_id: member("lin"), track: Track::Microphone }, now);
        assert_eq!(room.tiles(now)[0].name, "lin", "until the server says a name, the person is their id");
        room.on(Event::MemberJoined { user_id: member("lin"), name: "Lin".into(), colour: None }, now);
        let tiles = room.tiles(now);
        assert_eq!((tiles.len(), tiles[0].name.as_str(), tiles[0].muted), (1, "Lin", false));
    }

    #[test]
    fn the_end_of_the_room_is_said_in_a_sentence() {
        let now = Instant::now();
        let mut room = joined(now);
        assert_eq!(room.ended(), None);
        room.on(Event::Ended(End::Closed(Some(CloseReason::Moved))), now);
        assert_eq!(room.ended(), Some(&End::Closed(Some(CloseReason::Moved))));
        assert_eq!(room.ended_why(), Some(Why::Moved));
    }

    #[test]
    fn a_person_who_muted_or_deafened_is_shown_so_even_while_they_send_a_stream() {
        let now = Instant::now();
        let mut room = joined(now);
        assert!(!room.tiles(now)[0].muted);
        room.on(Event::State { user_id: member(ADA), state: State { mic_muted: true, deafened: false } }, now);
        let ada = room.tiles(now).remove(0);
        assert!(ada.muted && !ada.deafened, "Ada muted and still sends a stream");
        room.on(Event::State { user_id: member(ADA), state: State { mic_muted: true, deafened: true } }, now);
        assert!(room.tiles(now)[0].deafened);
        room.on(Event::State { user_id: member(ADA), state: State::default() }, now);
        assert!(!room.tiles(now)[0].muted, "and undoing it shows her again");
    }

    #[test]
    fn a_snapshot_carries_each_persons_state_and_a_state_before_the_person_is_kept() {
        let now = Instant::now();
        let mut room = Room::new(None);
        room.on(Event::State { user_id: member(GRACE), state: State { mic_muted: false, deafened: true } }, now);
        let mut ada = named(ADA, "Ada");
        ada.state.mic_muted = true;
        room.on(Event::Joined { members: vec![ada, named(GRACE, "Grace")], streams: vec![] }, now);
        let tiles = room.tiles(now);
        assert!(tiles[0].muted && !tiles[0].deafened);
        assert!(!tiles[1].deafened, "the snapshot is the whole truth, and Grace had not deafened in it");
    }

    #[test]
    fn your_own_tile_is_first_and_marked_whenever_you_arrived() {
        let now = Instant::now();
        let mut room = Room::new(Some(member(GRACE)));
        room.on(Event::Joined { members: vec![named(ADA, "Ada"), named(GRACE, "Grace")], streams: vec![] }, now);
        let tiles = room.tiles(now);
        assert_eq!(tiles.iter().map(|tile| tile.name.as_str()).collect::<Vec<_>>(), ["Grace", "Ada"]);
        assert!(tiles[0].own() && !tiles[1].own());
        assert_eq!(tiles[1].colour, design::PersonColour::Swatch(crate::people::swatch_of(&member(ADA))));
        room.on(Event::MemberJoined { user_id: member("lin"), name: "Lin".into(), colour: None }, now);
        assert!(room.tiles(now)[0].own(), "a later arrival does not take the first place");
    }

    #[test]
    fn your_tile_follows_the_microphone_you_send_and_is_heard_by_your_level() {
        let now = Instant::now();
        let mut room = Room::new(Some(member(ADA)));
        room.on(Event::Joined { members: vec![named(ADA, "Ada")], streams: vec![] }, now);
        assert!(
            room.tiles(now)[0].muted,
            "the server never announces your own stream, so until the call says it, none"
        );
        room.on(Event::Started { user_id: member(ADA), track: Track::Microphone }, now);
        room.on(Event::Levels(vec![(member(ADA), 40)]), now);
        let own = room.tiles(now).remove(0);
        assert!(!own.muted && own.speaking, "you ring when you speak");
    }

    #[test]
    fn deafening_shows_muted_too_and_undoing_it_brings_the_microphone_back() {
        let now = Instant::now();
        let mut room = joined(now);
        room.on(Event::State { user_id: member(ADA), state: State { mic_muted: false, deafened: true } }, now);
        let ada = room.tiles(now).remove(0);
        assert!(ada.muted && ada.deafened, "deaf people send nothing");
        room.on(Event::State { user_id: member(ADA), state: State::default() }, now);
        assert!(!room.tiles(now)[0].muted);
        assert_eq!(room.state_of(&member(ADA)), State::default());
        assert_eq!(room.state_of(&member("nobody")), State::default());
    }
}
