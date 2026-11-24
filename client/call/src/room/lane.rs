//! The call's one media lane: what this account takes of the others, brought one step at a time to the room the events
//! describe. What is wanted is the room itself, so a stream that came and went while the lane was busy is never taken.
//! Voices go before pictures, and a stream refused is tried again, ever less often, for as long as the room lists it.

use super::mend;
use crate::bounded::{Panicked, Task};
use crate::watch::viewing::{Viewed, Viewing};
use link::ASK_TIMEOUT;
use net::Jitter;
use picture::Painting;
use proto::chatkord_v1::{ErrorCode, TrackKind};
use proto::video::Video;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use terms::{LinkError, MemberId};
use tokio::time::Instant;
use webrtc::{Half, Hearing, Step, VoiceRefused};
use words::Failure;

/// A member's stream, as the server names it.
pub(super) type Stream = (MemberId, TrackKind);

/// When a stream began, in the order this call was told: a stream produced again is a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Began(u64);

/// Whether this call shows the pictures sent in it: the screens shared and the cameras.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shown {
    Pictures,
    Nothing,
}

/// The streams of the room this call takes: every other member's microphone and share sound, and their screens and
/// cameras where the call shows them.
#[derive(Debug)]
pub(super) struct Wanted {
    me: Option<MemberId>,
    shown: Shown,
    next: u64,
    streams: BTreeMap<Stream, Began>,
}

impl Wanted {
    pub(super) fn new(me: Option<MemberId>, shown: Shown) -> Self {
        Self { me, shown, next: 0, streams: BTreeMap::new() }
    }

    fn takes(&self, (user_id, kind): &Stream) -> bool {
        let kind_taken = match kind {
            TrackKind::Microphone | TrackKind::ScreenAudio => true,
            TrackKind::ScreenVideo | TrackKind::Camera => self.shown == Shown::Pictures,
            TrackKind::Unspecified => false,
        };
        kind_taken && self.me.as_ref() != Some(user_id)
    }

    fn stamp(&mut self) -> Began {
        self.next += 1;
        Began(self.next)
    }

    /// A stream began: it is wanted from now, as a new one even if one of its name was.
    pub(super) fn began(&mut self, stream: Stream) {
        if self.takes(&stream) {
            let began = self.stamp();
            self.streams.insert(stream, began);
        }
    }

    pub(super) fn ended(&mut self, stream: &Stream) {
        self.streams.remove(stream);
    }

    /// The room whole: what it lists is wanted, keeping when each began, and what was refused is tried again at
    /// once, whatever its refusal was.
    pub(super) fn whole(&mut self, streams: Vec<Stream>, tried: &mut Tried) {
        let mut now = BTreeMap::new();
        let taken: Vec<Stream> = streams.into_iter().filter(|stream| self.takes(stream)).collect();
        for stream in taken {
            let began = match self.streams.get(&stream) {
                Some(began) => *began,
                None => self.stamp(),
            };
            if matches!(tried.get(&stream), Some(Attempt::Refused(refused, _)) if *refused == began) {
                tried.remove(&stream);
            }
            now.insert(stream, began);
        }
        self.streams = now;
    }
}

/// Whether a refusal may pass by itself while the room still lists the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The server, the route or the engine would not just now.
    Passing,
    /// Trying again changes nothing: the stream is gone, or this device cannot show it.
    Final,
}

impl Refusal {
    /// A sound that was not taken: the server saying the stream is not there is final, the room drops it.
    pub(super) fn of_take(refused: &VoiceRefused) -> Self {
        match refused {
            VoiceRefused::Link(LinkError::Refused(ErrorCode::NotFound)) => Self::Final,
            VoiceRefused::Offer
            | VoiceRefused::Engine
            | VoiceRefused::AlreadySending
            | VoiceRefused::Sound(_)
            | VoiceRefused::Link(_)
            | VoiceRefused::Stalled(_)
            | VoiceRefused::Unreached { .. } => Self::Passing,
        }
    }

    /// A picture that was not shown: one this device has no decoder for is final, as one the server says is gone.
    pub(super) fn of_watch(failed: &Failure) -> Self {
        let gone = matches!(failed, Failure::Link(LinkError::Refused(ErrorCode::NotFound)));
        if gone || *failed == Failure::CannotDecode { Self::Final } else { Self::Passing }
    }
}

/// When a refused stream is tried again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Retry {
    /// Refused `tries` times running; offered again from `due`.
    At { tries: u32, due: Instant },
    /// Not until the room is told whole.
    Never,
}

impl Retry {
    /// The retry after a refusal at `now` of a stream refused `earlier` times running before it, on the schedule a
    /// lost route is mended on, spread by `jitter`.
    pub(super) fn after(earlier: u32, refusal: Refusal, now: Instant, jitter: Jitter) -> Self {
        match refusal {
            Refusal::Final => Self::Never,
            Refusal::Passing => {
                let tries = earlier.saturating_add(1);
                Self::At { tries, due: now + mend::BACKOFF.delay(tries, jitter) }
            }
        }
    }
}

/// What became of the last try at a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Attempt {
    Taken(Began),
    Refused(Began, Retry),
}

/// How many times running the stream that `began` then was refused, as `tried` has it.
pub(super) fn refusals(tried: &Tried, stream: &Stream, began: Began) -> u32 {
    match tried.get(stream) {
        Some(Attempt::Refused(refused, Retry::At { tries, .. })) if *refused == began => *tries,
        Some(Attempt::Taken(_) | Attempt::Refused(..)) | None => 0,
    }
}

/// Voices first, then a share's sound, then pictures: a picture's decoder may take a while to open, and nobody
/// should wait for a voice behind it.
fn rank(kind: TrackKind) -> u8 {
    match kind {
        TrackKind::Microphone => 0,
        TrackKind::ScreenAudio => 1,
        TrackKind::ScreenVideo | TrackKind::Camera => 2,
        TrackKind::Unspecified => 3,
    }
}

/// Whether `stream` is to be tried at `now`: never tried, or refused and due again.
fn open(tried: &Tried, stream: &Stream, now: Instant) -> bool {
    match tried.get(stream) {
        None => true,
        Some(Attempt::Refused(_, Retry::At { due, .. })) => *due <= now,
        Some(Attempt::Taken(_) | Attempt::Refused(_, Retry::Never)) => false,
    }
}

/// When the earliest refused stream still wanted is due again; none while nothing waits to be tried again.
pub(super) fn due(wanted: &Wanted, tried: &Tried) -> Option<Instant> {
    tried
        .iter()
        .filter_map(|(stream, attempt)| match attempt {
            Attempt::Refused(began, Retry::At { due, .. }) if wanted.streams.get(stream) == Some(began) => Some(*due),
            Attempt::Taken(_) | Attempt::Refused(..) => None,
        })
        .min()
}

/// What the lane did about each stream, which is what [`Hearing`] holds of it.
pub(super) type Tried = BTreeMap<Stream, Attempt>;

/// The lane's next step toward what is wanted.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    /// Stop taking a stream no longer wanted, or wanted anew.
    Forget(Stream),
    /// Stop showing a picture no longer wanted, or wanted anew.
    Unwatch(Viewed),
    /// Let go of a refusal nobody wants any more.
    Drop(Stream),
    Take(Stream, Began),
    Watch(Viewed, Began),
}

/// What the lane does next at `now`: what was taken and is no longer wanted goes first, then what is wanted and
/// not tried, or refused and due again: voices before pictures, each kind in the order it began.
pub(super) fn next(wanted: &Wanted, tried: &Tried, now: Instant) -> Option<Action> {
    let stale = tried.iter().find(|(stream, attempt)| {
        let began = match attempt {
            Attempt::Taken(began) | Attempt::Refused(began, _) => began,
        };
        wanted.streams.get(*stream) != Some(began)
    });
    if let Some(((user_id, kind), attempt)) = stale {
        return Some(match (attempt, Video::of(*kind)) {
            (Attempt::Refused(..), _) => Action::Drop((user_id.clone(), *kind)),
            (Attempt::Taken(_), Some(video)) => Action::Unwatch((user_id.clone(), video)),
            (Attempt::Taken(_), None) => Action::Forget((user_id.clone(), *kind)),
        });
    }
    let ((user_id, kind), began) = wanted
        .streams
        .iter()
        .filter(|(stream, _)| open(tried, stream, now))
        .min_by_key(|((_, kind), began)| (rank(*kind), **began))?;
    Some(match Video::of(*kind) {
        Some(video) => Action::Watch((user_id.clone(), video), *began),
        None => Action::Take((user_id.clone(), *kind), *began),
    })
}

/// A step under way: it owns [`Hearing`] and gives it back with what it did.
pub(crate) type Flight<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What a step did.
pub(super) enum Done {
    Took(Stream, Began, Result<(), VoiceRefused>),
    /// A picture shown, or not, on the GPU it was looked up on.
    Watched(Viewed, Began, Result<Box<Viewing>, Failure>, Painting),
    Unwatched(Viewed),
    /// The receive connection's ICE asked to start again, or why not.
    Mended(Result<(), VoiceRefused>),
}

/// What a step of the lane is doing, in an operator's words. It names no member: it is logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub(super) enum Doing {
    Taking(TrackKind),
    Showing(Video),
    LettingGo(Video),
    Mending,
}

/// How long opening a picture's decoder may take, the GPU looked up first: past a GPU wait's bound (one second) ten
/// times over. A bound, not a measurement.
const DECODER_WITHIN: Duration = Duration::from_secs(10);

/// The engine's three steps of one renegotiation of the receive connection.
fn renegotiation() -> Duration {
    Step::SetRemote(Half::Receive).bound() + Step::CreateAnswer.bound() + Step::SetLocal(Half::Receive).bound()
}

impl Doing {
    /// The longest the step may take, every wait in it at its own bound: past this it is not waiting on any of
    /// them, and nothing brings it back.
    pub(super) fn within(self) -> Duration {
        // The consumer asked for, this end's DTLS the first time, the resume; then the track's arrival.
        let taking = ASK_TIMEOUT * 3 + renegotiation() + Step::Arrival.bound();
        match self {
            Self::Taking(_) => taking,
            // A decoder that would not open gives the stream back.
            Self::Showing(_) => taking + DECODER_WITHIN + ASK_TIMEOUT,
            Self::LettingGo(_) => ASK_TIMEOUT + crate::send::worker::STOP_BOUND,
            Self::Mending => ASK_TIMEOUT + renegotiation(),
        }
    }
}

impl fmt::Display for Doing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Taking(TrackKind::Microphone) => f.write_str("taking a voice"),
            Self::Taking(_) => f.write_str("taking a share's sound"),
            Self::Showing(Video::Screen) => f.write_str("taking a screen"),
            Self::Showing(Video::Camera) => f.write_str("taking a camera"),
            Self::LettingGo(Video::Screen) => f.write_str("letting go of a screen"),
            Self::LettingGo(Video::Camera) => f.write_str("letting go of a camera"),
            Self::Mending => f.write_str("starting the receive route's ICE again"),
        }
    }
}

/// Where the lane is, as the call decides it: free, carrying out one step with the time it is late, or gone with a
/// step that never came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    Idle,
    Busy {
        /// What the step does.
        doing: Doing,
        /// When it is late.
        due: Instant,
    },
    /// What it takes is gone, with the call or with a step that never came back: nothing more can be taken.
    Ended,
}

impl Lane {
    /// Whether no step runs and one may start.
    pub(super) fn is_idle(&self) -> bool {
        matches!(self, Lane::Idle)
    }

    /// A step starts at `now`, said as `doing`; nothing changes while one runs or the lane is gone.
    pub(super) fn start(&mut self, now: Instant, doing: Doing) {
        if self.is_idle() {
            *self = Lane::Busy { doing, due: now + doing.within() };
        }
    }

    /// The running step landed: the lane is free again.
    pub(super) fn landed(&mut self) {
        if matches!(self, Lane::Busy { .. }) {
            *self = Lane::Idle;
        }
    }

    /// The running step ended without an answer, and what the lane takes went with it. What it was doing.
    pub(super) fn lost(&mut self) -> Option<Doing> {
        let Lane::Busy { doing, .. } = *self else { return None };
        *self = Lane::Ended;
        Some(doing)
    }

    /// When the running step is late.
    pub(super) fn due(&self) -> Option<Instant> {
        match self {
            Lane::Busy { due, .. } => Some(*due),
            Lane::Idle | Lane::Ended => None,
        }
    }

    /// The step still out at its bound at `now`, and the bound: it is let go, and the lane ends with it, for what it
    /// takes is the step's and never comes back.
    pub(super) fn stalled(&mut self, now: Instant) -> Option<(Doing, Duration)> {
        let Lane::Busy { doing, due } = *self else { return None };
        if now < due {
            return None;
        }
        *self = Lane::Ended;
        Some((doing, doing.within()))
    }
}

/// What carries out the lane's steps: what it takes while no step runs, else the step, a task of its own.
/// `H` is what it takes, [`Hearing`] in a call.
pub(super) struct Steps<H = Hearing> {
    held: Option<H>,
    step: Option<Task<(H, Done)>>,
}

impl<H: Send + 'static> Steps<H> {
    /// A lane free with `held` in hand.
    pub(super) fn new(held: H) -> Self {
        Self { held: Some(held), step: None }
    }

    /// What it takes, while no step runs.
    pub(super) fn idle(&mut self) -> Option<&mut H> {
        self.held.as_mut()
    }

    /// Starts a step on what it takes; does nothing while one runs.
    pub(super) fn start(&mut self, step: impl FnOnce(H) -> Flight<(H, Done)>) {
        if let Some(held) = self.held.take() {
            self.step = Some(Task::spawn(step(held)));
        }
    }

    /// Resolves with what the running step did, and the lane is free again; [`Panicked`] when its task ended without
    /// an answer, and what the lane takes went with it. Never, while none runs. Dropping it leaves the step running.
    pub(super) async fn done(&mut self) -> Result<Done, Panicked> {
        let Some(step) = &mut self.step else { return std::future::pending().await };
        let answer = step.answer().await;
        self.step = None;
        let (held, done) = answer?;
        self.held = Some(held);
        Ok(done)
    }

    /// The running step is let go.
    pub(super) fn drop_step(&mut self) {
        self.step = None;
    }

    /// The lane's end with the call's: a step under way is ended where it waits, and what the lane takes is let go
    /// on the blocking pool, which may wait on the engine, before this returns.
    pub(super) async fn end(&mut self) {
        let held = match self.step.take() {
            Some(mut step) => {
                step.end();
                step.answer().await.ok().map(|(held, done)| (held, Some(done)))
            }
            None => self.held.take().map(|held| (held, None)),
        };
        if let Some(held) = held {
            let _ = tokio::task::spawn_blocking(move || drop(held)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";
    const BEA: &str = "bea";
    const ME: &str = "me";
    const MIC: TrackKind = TrackKind::Microphone;
    const SOUND: TrackKind = TrackKind::ScreenAudio;
    const SCREEN: TrackKind = TrackKind::ScreenVideo;

    fn member(id: &str) -> MemberId {
        MemberId::parse(id).expect("a member id")
    }

    fn of(user_id: &str, kind: TrackKind) -> Stream {
        (member(user_id), kind)
    }

    fn wanted() -> Wanted {
        Wanted::new(Some(member(ME)), Shown::Pictures)
    }

    /// No spread: `from_bits` maps the middle of its range to 1.0.
    fn even() -> Jitter {
        Jitter::from_bits(1 << 63)
    }

    /// Carries out every step as if each worked, and returns them in order.
    fn settle(wanted: &Wanted, tried: &mut Tried) -> Vec<Action> {
        let now = Instant::now();
        let mut done = Vec::new();
        while let Some(action) = next(wanted, tried, now) {
            match &action {
                Action::Forget(stream) | Action::Drop(stream) => {
                    tried.remove(stream);
                }
                Action::Unwatch((user_id, video)) => {
                    tried.remove(&(user_id.clone(), video.kind()));
                }
                Action::Take(stream, began) => {
                    tried.insert(stream.clone(), Attempt::Taken(*began));
                }
                Action::Watch((user_id, video), began) => {
                    tried.insert((user_id.clone(), video.kind()), Attempt::Taken(*began));
                }
            }
            done.push(action);
            assert!(done.len() < 100, "the lane never settles: {done:?}");
        }
        done
    }

    fn named(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .map(|action| match action {
                Action::Forget((user_id, kind)) => format!("forget {} {kind:?}", user_id.as_str()),
                Action::Unwatch((user_id, video)) => format!("unwatch {} {video:?}", user_id.as_str()),
                Action::Drop((user_id, kind)) => format!("drop {} {kind:?}", user_id.as_str()),
                Action::Take((user_id, kind), _) => format!("take {} {kind:?}", user_id.as_str()),
                Action::Watch((user_id, video), _) => format!("watch {} {video:?}", user_id.as_str()),
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn voices_are_taken_before_a_share_s_sound_and_pictures_whatever_began_first_each_in_the_order_it_began() {
        let mut wanted = wanted();
        wanted.began(of(ADA, TrackKind::Camera));
        wanted.began(of(ADA, SCREEN));
        wanted.began(of(ADA, SOUND));
        wanted.began(of(BEA, MIC));
        wanted.began(of(ADA, MIC));
        let mut tried = Tried::new();
        assert_eq!(
            named(&settle(&wanted, &mut tried)),
            [
                "take bea Microphone",
                "take ada Microphone",
                "take ada ScreenAudio",
                "watch ada Camera",
                "watch ada Screen"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusal_is_offered_again_only_once_due_after_one_two_four_then_every_eight_seconds() {
        let start = Instant::now();
        let mut wanted = wanted();
        wanted.began(of(ADA, MIC));
        wanted.began(of(BEA, MIC));
        let mut tried = Tried::new();
        let (mut now, mut waits) = (start, Vec::new());
        for round in 0..6 {
            let Some(Action::Take(stream, began)) = next(&wanted, &tried, now) else { panic!("round {round}: due") };
            assert_eq!(stream, of(ADA, MIC), "round {round}: the stream refused, in its turn");
            let retry = Retry::after(refusals(&tried, &stream, began), Refusal::Passing, now, even());
            tried.insert(stream, Attempt::Refused(began, retry));
            // Bea's voice is not held up behind the refusal.
            if round == 0 {
                let Some(Action::Take(other, began)) = next(&wanted, &tried, now) else { panic!("bea is untried") };
                tried.insert(other, Attempt::Taken(began));
            }
            let Retry::At { tries, due: again } = retry else { panic!("a refusal that may pass is tried again") };
            assert_eq!(tries, round + 1, "each refusal is counted");
            assert_eq!(due(&wanted, &tried), Some(again), "the loop is woken when it is due");
            assert_eq!(next(&wanted, &tried, again - Duration::from_millis(1)), None, "round {round}: not before");
            waits.push(again.duration_since(now).as_secs_f64().round());
            now = again;
        }
        assert_eq!(waits, [1.0, 2.0, 4.0, 8.0, 8.0, 8.0]);
        wanted.ended(&of(ADA, MIC));
        assert_eq!(due(&wanted, &tried), None, "a retry lasts only while the room lists the stream");
        assert_eq!(named(&settle(&wanted, &mut tried)), ["drop ada Microphone"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusal_that_cannot_pass_is_never_offered_again_until_the_room_is_told_whole() {
        let start = Instant::now();
        let mut wanted = wanted();
        wanted.began(of(ADA, TrackKind::Camera));
        let mut tried = Tried::new();
        let Some(Action::Watch(_, began)) = next(&wanted, &tried, start) else { panic!("untried") };
        let retry = Retry::after(0, Refusal::of_watch(&Failure::CannotDecode), start, even());
        assert_eq!(retry, Retry::Never);
        tried.insert(of(ADA, TrackKind::Camera), Attempt::Refused(began, retry));
        assert_eq!(due(&wanted, &tried), None);
        assert_eq!(next(&wanted, &tried, start + Duration::from_secs(3600)), None, "however long it waits");
        wanted.whole(vec![of(ADA, TrackKind::Camera)], &mut tried);
        assert_eq!(named(&settle(&wanted, &mut tried)), ["watch ada Camera"], "the room said it anew");
    }

    #[test]
    fn a_stream_the_server_says_is_gone_or_this_device_cannot_decode_is_final_and_any_other_refusal_may_pass() {
        let refused = |code| LinkError::Refused(code);
        assert_eq!(Refusal::of_take(&VoiceRefused::Link(refused(ErrorCode::NotFound))), Refusal::Final);
        assert_eq!(Refusal::of_take(&VoiceRefused::Link(refused(ErrorCode::ServerUnavailable))), Refusal::Passing);
        assert_eq!(Refusal::of_take(&VoiceRefused::Link(LinkError::Timeout)), Refusal::Passing);
        assert_eq!(Refusal::of_take(&VoiceRefused::Stalled(Step::Arrival)), Refusal::Passing);
        assert_eq!(Refusal::of_take(&VoiceRefused::Engine), Refusal::Passing);
        assert_eq!(Refusal::of_watch(&Failure::CannotDecode), Refusal::Final);
        assert_eq!(Refusal::of_watch(&Failure::Link(refused(ErrorCode::NotFound))), Refusal::Final);
        assert_eq!(Refusal::of_watch(&Failure::ScreenNotCarried), Refusal::Passing);
        assert_eq!(Refusal::of_watch(&Failure::EngineStalled), Refusal::Passing);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_that_ended_before_its_turn_is_never_taken() {
        let mut wanted = wanted();
        wanted.began(of(ADA, MIC));
        wanted.began(of(BEA, MIC));
        wanted.ended(&of(ADA, MIC));
        assert_eq!(named(&settle(&wanted, &mut Tried::new())), ["take bea Microphone"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_begun_again_while_taken_is_forgotten_then_taken_anew() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        wanted.began(of(ADA, MIC));
        settle(&wanted, &mut tried);
        wanted.ended(&of(ADA, MIC));
        wanted.began(of(ADA, MIC));
        assert_eq!(named(&settle(&wanted, &mut tried)), ["forget ada Microphone", "take ada Microphone"]);
    }

    #[tokio::test(start_paused = true)]
    async fn what_is_no_longer_wanted_goes_before_anything_is_taken() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        wanted.began(of(ADA, MIC));
        wanted.began(of(ADA, SCREEN));
        settle(&wanted, &mut tried);
        wanted.began(of(BEA, MIC));
        wanted.ended(&of(ADA, MIC));
        wanted.ended(&of(ADA, SCREEN));
        assert_eq!(
            named(&settle(&wanted, &mut tried)),
            ["forget ada Microphone", "unwatch ada Screen", "take bea Microphone"],
            "a screen is let go before the next is shown"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn one_s_own_streams_are_never_taken_even_when_the_room_whole_lists_them() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        wanted.began(of(ME, MIC));
        wanted.whole(vec![of(ME, MIC), of(ME, SOUND), of(ME, SCREEN), of(ADA, MIC)], &mut tried);
        assert_eq!(named(&settle(&wanted, &mut tried)), ["take ada Microphone"]);
    }

    #[tokio::test(start_paused = true)]
    async fn the_room_whole_keeps_what_is_taken_forgets_what_it_leaves_out_and_tries_a_refusal_again() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        for stream in [of(ADA, MIC), of(BEA, MIC), of(BEA, SCREEN)] {
            wanted.began(stream);
        }
        settle(&wanted, &mut tried);
        // Bea's microphone was refused: it is not tried again before it is due, unless the room is told whole.
        let Some(Attempt::Taken(began)) = tried.get(&of(BEA, MIC)).copied() else { panic!("taken") };
        let now = Instant::now();
        tried.insert(of(BEA, MIC), Attempt::Refused(began, Retry::after(3, Refusal::Passing, now, even())));
        assert_eq!(next(&wanted, &tried, now), None, "a refusal stands until it is due");
        wanted.whole(vec![of(ADA, MIC), of(BEA, MIC), of(ADA, SOUND)], &mut tried);
        assert_eq!(
            named(&settle(&wanted, &mut tried)),
            ["unwatch bea Screen", "take bea Microphone", "take ada ScreenAudio"],
            "ada's microphone is kept as it is"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusal_nobody_wants_any_more_is_let_go_and_what_is_tried_never_outgrows_the_room() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        let now = Instant::now();
        for round in 0..50 {
            wanted.began(of(ADA, MIC));
            if let Some(Action::Take(stream, began)) = next(&wanted, &tried, now) {
                let refusal = if round % 2 == 0 { Refusal::Passing } else { Refusal::Final };
                tried.insert(stream, Attempt::Refused(began, Retry::after(0, refusal, now, even())));
            }
            wanted.ended(&of(ADA, MIC));
            settle(&wanted, &mut tried);
            assert!(tried.is_empty(), "round {round}: {tried:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_camera_is_watched_beside_a_screen_and_let_go_alone() {
        let mut wanted = wanted();
        let mut tried = Tried::new();
        wanted.began(of(ADA, SCREEN));
        wanted.began(of(ADA, TrackKind::Camera));
        wanted.began(of(ME, TrackKind::Camera));
        assert_eq!(named(&settle(&wanted, &mut tried)), ["watch ada Screen", "watch ada Camera"], "never one's own");
        wanted.ended(&of(ADA, TrackKind::Camera));
        assert_eq!(named(&settle(&wanted, &mut tried)), ["unwatch ada Camera"], "the screen stays");
    }

    #[tokio::test(start_paused = true)]
    async fn no_screen_is_wanted_where_nothing_shows_one() {
        let mut wanted = Wanted::new(Some(member(ME)), Shown::Nothing);
        wanted.began(of(ADA, TrackKind::Camera));
        wanted.began(of(ADA, SCREEN));
        wanted.began(of(ADA, SOUND));
        wanted.whole(vec![of(BEA, SCREEN), of(ADA, SOUND)], &mut Tried::new());
        assert_eq!(named(&settle(&wanted, &mut Tried::new())), ["take ada ScreenAudio"], "its sound is still heard");
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_with_no_kind_is_never_wanted() {
        let mut wanted = wanted();
        wanted.began(of(ADA, TrackKind::Unspecified));
        assert_eq!(next(&wanted, &Tried::new(), Instant::now()), None);
    }

    /// A step that mends, or one whose task is lost saying `lost`.
    fn step(lost: Option<&'static str>) -> Flight<((), Done)> {
        Box::pin(async move {
            match lost {
                Some(lost) => panic!("{lost}"),
                None => ((), Done::Mended(Ok(()))),
            }
        })
    }

    fn mended() -> Flight<((), Done)> {
        step(None)
    }

    /// A step that never returns, holding `held` while it is out.
    fn never(held: tokio::sync::mpsc::Sender<()>) -> Flight<((), Done)> {
        Box::pin(async move {
            let _held = held;
            std::future::pending::<((), Done)>().await
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_is_a_task_of_its_own_and_the_lane_is_free_again_when_it_lands() {
        let (mut lane, mut steps) = (Lane::Idle, Steps::new(()));
        lane.start(Instant::now(), Doing::Mending);
        steps.start(|()| mended());
        assert!(!lane.is_idle() && steps.idle().is_none(), "one step at a time");
        lane.start(Instant::now(), Doing::Taking(MIC));
        steps.start(|()| step(Some("no second step is started while one runs")));
        assert!(matches!(steps.done().await, Ok(Done::Mended(Ok(())))));
        lane.landed();
        assert!(lane.is_idle() && steps.idle().is_some());
        assert_eq!(lane.due(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_that_never_returns_is_named_at_its_bound_and_the_lane_ends_with_it() {
        let asked = Instant::now();
        let doing = Doing::Taking(MIC);
        let (held, mut let_go) = tokio::sync::mpsc::channel::<()>(1);
        let (mut lane, mut steps) = (Lane::Idle, Steps::new(()));
        lane.start(asked, doing);
        steps.start(|()| never(held));
        assert_eq!(lane.due(), Some(asked + doing.within()));
        assert_eq!(lane.stalled(asked + doing.within() - Duration::from_millis(1)), None, "not before its bound");
        assert_eq!(lane.stalled(asked + doing.within()), Some((doing, doing.within())));
        steps.drop_step();
        assert_eq!(doing.to_string(), "taking a voice", "said by what it was doing");
        assert_eq!(lane, Lane::Ended, "what it took went with the step: nothing more can be taken");
        assert_eq!(let_go.recv().await, None, "and the step was let go");
        lane.start(Instant::now(), Doing::Mending);
        assert_eq!(lane, Lane::Ended, "an ended lane starts nothing");
        assert_eq!(lane.stalled(asked + doing.within() * 2), None, "said once");
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_whose_task_is_lost_ends_the_lane() {
        let (mut lane, mut steps) = (Lane::Idle, Steps::new(()));
        lane.start(Instant::now(), Doing::Showing(Video::Screen));
        steps.start(|()| step(Some("lost")));
        assert_eq!(steps.done().await.err(), Some(Panicked));
        assert_eq!(lane.lost(), Some(Doing::Showing(Video::Screen)));
        assert_eq!(lane, Lane::Ended);
        assert!(steps.idle().is_none(), "what it took went with the step");
    }

    #[tokio::test(start_paused = true)]
    async fn the_call_s_end_lets_go_of_a_step_under_way_at_once() {
        let (held, mut let_go) = tokio::sync::mpsc::channel::<()>(1);
        let mut steps = Steps::new(());
        steps.start(|()| never(held));
        let asked = Instant::now();
        steps.end().await;
        assert_eq!(Instant::now(), asked, "nothing waited for");
        assert_eq!(let_go.recv().await, None);
        assert!(steps.idle().is_none());
    }

    #[test]
    fn every_step_s_bound_is_past_every_wait_inside_it() {
        let asks = ASK_TIMEOUT * 3;
        assert!(Doing::Taking(MIC).within() >= asks + webrtc::ENGINE_WITHIN * 3 + Step::Arrival.bound());
        assert!(Doing::Showing(Video::Camera).within() > Doing::Taking(MIC).within());
        assert!(Doing::Mending.within() >= ASK_TIMEOUT + webrtc::ENGINE_WITHIN * 3);
    }
}
