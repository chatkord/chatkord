//! What this account sends of its own beside the lane, a screen or a camera: each wait (the open, the send, the
//! stop) is a step of its own, so the call goes on while it runs, and a stop or a replacement asked meanwhile is
//! carried out when the step ends. A send or a stop is a task under one bound: still out at it, the step is let go
//! and what was sent is over.

use crate::bounded::Bounded;
use crate::room::lane::Flight;
use link::ASK_TIMEOUT;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use webrtc::{Half, Step};
use words::Failure;

/// The longest one track's start may take over the send connection: this end's DTLS the first time and the producer
/// asked of the server, the engine's offer, its own description and the answer set, and the same three again where
/// a refused track's section is closed.
pub(crate) fn start_within() -> Duration {
    ASK_TIMEOUT * 2 + negotiation() * 2
}

/// The longest one track's end may take: the stop asked of the server, its section closed by an offer and an
/// answer, and the thread that encoded it joined.
pub(super) fn stop_within() -> Duration {
    ASK_TIMEOUT + negotiation() + crate::send::worker::STOP_BOUND
}

/// The engine's three steps of one renegotiation of the send connection.
fn negotiation() -> Duration {
    Step::CreateOffer.bound() + Step::SetLocal(Half::Send).bound() + Step::SetRemote(Half::Send).bound()
}

/// Why what is sent ends, as the person is told it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The person stopped it.
    Stopped,
    /// The server ended it: this account's roles no longer allow one.
    Denied,
    /// It could not be opened, sent or kept up.
    Failed(Failure),
}

/// One kind of thing sent, a screen or a camera: how it is begun, sent, stopped and said.
pub(crate) trait Kind: Sized + 'static {
    /// What the person asked for.
    type Ask: Send + 'static;
    /// What is opened and waits to be sent.
    type Begun: Send + 'static;
    /// What is being sent.
    type Live: Send + 'static;
    /// How its end is said to the person.
    type Said: Clone + std::fmt::Debug + Send;

    /// What it is, in an operator's words: "the screen".
    const WHAT: &'static str;

    /// The longest [`Sender::send`]'s step may take, every wait in it at its own bound.
    fn send_within() -> Duration;
    /// The longest [`Kind::stop`] or [`Kind::close`] may take, likewise.
    fn stop_within() -> Duration;

    /// Opens what `ask` names, read by `watched` for whether anyone takes it once sent.
    fn begin(&self, ask: Self::Ask, watched: Arc<AtomicBool>) -> JoinHandle<Result<Self::Begun, Failure>>;
    /// Lets go of an ask never begun.
    fn let_go(ask: Self::Ask);
    /// Lets go of what was opened and never sent.
    fn abandon(begun: Self::Begun) -> Flight<()>;
    /// Ends what is sent, at the server too.
    fn stop(&self, live: Self::Live) -> Flight<()>;
    /// Resolves with why it ended by itself; never while it runs, and cancel-safe.
    fn ended(live: &mut Self::Live) -> impl Future<Output = Failure> + Send;
    /// Lets go of what is left at the server of what [`Kind::ended`] by itself.
    fn close(&self, live: Self::Live) -> Flight<()>;
    /// How `end` is said.
    fn said(end: End) -> Self::Said;
    /// What it showed of itself is let go, once it is over.
    fn emptied(&self);
}

/// Whether the room was told it started, so it is told it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Was {
    Live,
    Unsent,
}

/// What is done with what is opened or sent once that step ends.
#[derive(Clone, Debug)]
enum Then<S> {
    Send,
    /// Let go of it, and say this.
    Abandon(S),
}

enum State<K: Kind> {
    Off,
    Beginning(JoinHandle<Result<K::Begun, Failure>>, Then<K::Said>),
    Sending(Bounded<Result<K::Live, Failure>>, Then<K::Said>),
    Live(Box<K::Live>),
    /// What was sent is let go: what the person is told after, and whether the room was told it started.
    Stopping(Bounded<()>, K::Said, Was),
}

/// The step of a sender still out at its bound, in an operator's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stalled {
    Sending,
    Stopping,
}

impl fmt::Display for Stalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sending => "starting to send",
            Self::Stopping => "stopping",
        })
    }
}

/// A stop of `K`, a task from now.
fn stopping<K: Kind>(stop: Flight<()>) -> Bounded<()> {
    Bounded::spawn(Instant::now(), K::stop_within(), stop)
}

/// What a step came to, for the call.
pub(crate) enum Turn<K: Kind> {
    /// It opened: the call sends it over its voice.
    Begun(Box<K::Begun>),
    /// It is being sent: the room is told.
    Sent,
    /// It is over, as the person is told.
    Over(K::Said, Was),
    /// It was let go for another ask, which is opening now: the person is told nothing until it is sent.
    Replaced(Was),
}

/// An ask waiting for the one before it to be let go.
enum Queued<A> {
    /// It replaces the one before: the person is told nothing of that one's end.
    Replacing(A),
    /// It follows one the person or the server had ended already, whose end is told.
    After(A),
}

impl<A> Queued<A> {
    fn ask(self) -> A {
        match self {
            Self::Replacing(ask) | Self::After(ask) => ask,
        }
    }
}

/// This account's own screen or camera.
pub(crate) struct Sender<K: Kind> {
    kind: K,
    state: State<K>,
    /// The ask to begin once the one before it is let go: the latest wins.
    queued: Option<Queued<K::Ask>>,
    /// Whether anyone takes it now, as the server last told; what is sent reads it.
    watched: Arc<AtomicBool>,
}

impl<K: Kind> Sender<K> {
    pub(crate) fn new(kind: K) -> Self {
        Self { kind, state: State::Off, queued: None, watched: Arc::default() }
    }

    /// The server told whether anyone takes it now, whatever step it is at.
    pub(crate) fn watched(&self, watched: bool) {
        self.watched.store(watched, Ordering::Relaxed);
    }

    /// Begins what the person asked for; one already begun, sent or stopping is let go first and this one begun after.
    pub(crate) fn ask(&mut self, ask: K::Ask) {
        let (queued, earlier) = match (&self.state, self.queued.take()) {
            (State::Off, _) => return self.begin(ask),
            // Asked again while one waits: the latest wins, and replaces as the first asked would have.
            (_, Some(Queued::Replacing(earlier))) => (Queued::Replacing(ask), Some(earlier)),
            (_, Some(Queued::After(earlier))) => (Queued::After(ask), Some(earlier)),
            (State::Stopping(..), None) => (Queued::After(ask), None),
            (State::Beginning(..) | State::Sending(..) | State::Live(_), None) => (Queued::Replacing(ask), None),
        };
        if let Some(earlier) = earlier {
            K::let_go(earlier);
        }
        self.queued = Some(queued);
        self.let_go(K::said(End::Stopped));
    }

    fn begin(&mut self, ask: K::Ask) {
        // Nobody takes it until the server says someone does.
        self.watched.store(false, Ordering::Relaxed);
        self.state = State::Beginning(self.kind.begin(ask, Arc::clone(&self.watched)), Then::Send);
    }

    /// Whether anyone takes it now.
    pub(crate) fn is_watched(&self) -> bool {
        self.watched.load(Ordering::Relaxed)
    }

    /// Whether anyone takes it, for what is about to be sent.
    pub(crate) fn watched_now(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.watched)
    }

    /// Sends what was opened, as the call's voice sends it.
    pub(crate) fn send(&mut self, flight: Flight<Result<K::Live, Failure>>) {
        self.state = State::Sending(Bounded::spawn(Instant::now(), K::send_within(), flight), Then::Send);
    }

    /// Whether it is on the send connection now: being started on it, sent, or ended on it.
    pub(crate) fn on_the_wire(&self) -> bool {
        match &self.state {
            State::Sending(..) | State::Live(_) | State::Stopping(..) => true,
            State::Off | State::Beginning(..) => false,
        }
    }

    /// When the step under way is late.
    pub(crate) fn due(&self) -> Option<Instant> {
        match &self.state {
            State::Sending(step, _) => Some(step.due()),
            State::Stopping(step, ..) => Some(step.due()),
            State::Off | State::Beginning(..) | State::Live(_) => None,
        }
    }

    /// The step still out at its bound at `now`, with its bound and the turn it comes to: it is let go, and what was
    /// being sent or stopped is over. A send that never came back is told as the engine's stall, unless the person
    /// had let it go; a stop, as what it was stopped for.
    pub(crate) fn stalled(&mut self, now: Instant) -> Option<(Stalled, Duration, Turn<K>)> {
        let late = match &self.state {
            State::Sending(step, _) => step.late(now).then(|| step.within()),
            State::Stopping(step, ..) => step.late(now).then(|| step.within()),
            State::Off | State::Beginning(..) | State::Live(_) => None,
        }?;
        let (stalled, turn) = match std::mem::replace(&mut self.state, State::Off) {
            State::Sending(_, then) => {
                (Stalled::Sending, Turn::Over(told::<K>(then, Failure::EngineStalled), Was::Unsent))
            }
            State::Stopping(_, told, was) => {
                self.kind.emptied();
                (Stalled::Stopping, Turn::Over(told, was))
            }
            state @ (State::Off | State::Beginning(..) | State::Live(_)) => {
                self.state = state;
                return None;
            }
        };
        Some((stalled, late, self.follow(turn)))
    }

    /// The person stopped it, or the one still being begun or sent.
    pub(crate) fn stop(&mut self) {
        self.end_as(End::Stopped);
    }

    /// The server ended it: this account's roles no longer allow one.
    pub(crate) fn denied(&mut self) {
        self.end_as(End::Denied);
    }

    fn end_as(&mut self, end: End) {
        if let Some(queued) = self.queued.take() {
            K::let_go(queued.ask());
        }
        self.let_go(K::said(end));
    }

    fn let_go(&mut self, told: K::Said) {
        self.state = match std::mem::replace(&mut self.state, State::Off) {
            State::Live(live) => State::Stopping(stopping::<K>(self.kind.stop(*live)), told, Was::Live),
            State::Beginning(flight, _) => State::Beginning(flight, Then::Abandon(told)),
            State::Sending(flight, _) => State::Sending(flight, Then::Abandon(told)),
            state @ (State::Stopping(..) | State::Off) => state,
        };
    }

    /// What is sent now, if it is.
    pub(crate) fn live(&mut self) -> Option<&mut K::Live> {
        match &mut self.state {
            State::Live(live) => Some(live),
            State::Off | State::Beginning(..) | State::Sending(..) | State::Stopping(..) => None,
        }
    }

    /// Resolves with the next turn it takes; never, while nothing is sent. Dropping it leaves the step under way.
    pub(crate) async fn next(&mut self) -> Turn<K> {
        let turn = self.step().await;
        self.follow(turn)
    }

    /// `turn`, and the ask that waited for it begun.
    fn follow(&mut self, turn: Turn<K>) -> Turn<K> {
        match (turn, self.queued.take()) {
            (Turn::Over(_, was), Some(Queued::Replacing(ask))) => {
                self.begin(ask);
                Turn::Replaced(was)
            }
            (turn @ Turn::Over(..), Some(Queued::After(ask))) => {
                self.begin(ask);
                turn
            }
            (turn, queued) => {
                self.queued = queued;
                turn
            }
        }
    }

    async fn step(&mut self) -> Turn<K> {
        loop {
            match &mut self.state {
                State::Off => return std::future::pending().await,
                State::Beginning(flight, then) => {
                    let begun = flight.await.unwrap_or(Err(Failure::EncodeStopped));
                    match (begun, then.clone()) {
                        (Ok(begun), Then::Send) => {
                            self.state = State::Off;
                            return Turn::Begun(Box::new(begun));
                        }
                        (Ok(begun), Then::Abandon(told)) => {
                            self.state = State::Stopping(stopping::<K>(K::abandon(begun)), told, Was::Unsent);
                        }
                        (Err(failed), then) => {
                            self.state = State::Off;
                            return Turn::Over(told::<K>(then, failed), Was::Unsent);
                        }
                    }
                }
                State::Sending(flight, then) => {
                    let sent = flight.answer().await.unwrap_or(Err(Failure::EncodeStopped));
                    match (sent, then.clone()) {
                        (Ok(live), Then::Send) => {
                            self.state = State::Live(Box::new(live));
                            return Turn::Sent;
                        }
                        (Ok(live), Then::Abandon(told)) => {
                            self.state = State::Stopping(stopping::<K>(self.kind.stop(live)), told, Was::Unsent);
                        }
                        (Err(failed), then) => {
                            self.state = State::Off;
                            return Turn::Over(told::<K>(then, failed), Was::Unsent);
                        }
                    }
                }
                State::Live(live) => {
                    let failed = K::ended(live).await;
                    if let State::Live(live) = std::mem::replace(&mut self.state, State::Off) {
                        let close = self.kind.close(*live);
                        self.state = State::Stopping(stopping::<K>(close), K::said(End::Failed(failed)), Was::Live);
                    }
                }
                State::Stopping(flight, told, was) => {
                    // A stop that panicked has nothing left to stop.
                    let _ = flight.answer().await;
                    let (told, was) = (told.clone(), *was);
                    self.state = State::Off;
                    self.kind.emptied();
                    return Turn::Over(told, was);
                }
            }
        }
    }

    /// The call is over: what is sent is stopped, a step under way finished first; one still being opened is let go
    /// on its own once it opens, so the call's end never waits on a picker or a device.
    pub(crate) async fn end(&mut self) {
        if let Some(queued) = self.queued.take() {
            K::let_go(queued.ask());
        }
        match std::mem::replace(&mut self.state, State::Off) {
            State::Off => {}
            State::Beginning(flight, _) => {
                drop(tokio::spawn(async move {
                    if let Ok(Ok(begun)) = flight.await {
                        K::abandon(begun).await;
                    }
                }));
            }
            State::Sending(mut flight, _) => {
                if let Ok(Ok(live)) = flight.answer().await {
                    self.kind.stop(live).await;
                }
            }
            State::Live(live) => self.kind.stop(*live).await,
            State::Stopping(mut flight, ..) => {
                let _ = flight.answer().await;
            }
        }
        self.kind.emptied();
    }
}

/// What the person is told of a step that failed: why, unless they had let it go already.
fn told<K: Kind>(then: Then<K::Said>, failed: Failure) -> K::Said {
    match then {
        Then::Send => K::said(End::Failed(failed)),
        Then::Abandon(told) => told,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const WITHIN: Duration = Duration::from_secs(40);

    /// A kind that opens and sends at once, writing down what is done with it.
    #[derive(Default)]
    struct Told(Arc<Mutex<Vec<String>>>);

    impl Told {
        fn note(log: &Mutex<Vec<String>>, what: String) {
            log.lock().expect("the log").push(what);
        }

        fn taken(&self) -> Vec<String> {
            std::mem::take(&mut *self.0.lock().expect("the log"))
        }
    }

    struct Sent(&'static str);

    impl Kind for Told {
        type Ask = &'static str;
        type Begun = &'static str;
        type Live = Sent;
        type Said = End;

        const WHAT: &'static str = "the thing";

        fn send_within() -> Duration {
            WITHIN
        }

        fn stop_within() -> Duration {
            WITHIN
        }

        fn begin(&self, ask: &'static str, _: Arc<AtomicBool>) -> JoinHandle<Result<&'static str, Failure>> {
            Self::note(&self.0, format!("begin {ask}"));
            tokio::spawn(async move { Ok(ask) })
        }

        fn let_go(_: &'static str) {}

        fn abandon(_: &'static str) -> Flight<()> {
            Box::pin(async {})
        }

        fn stop(&self, live: Sent) -> Flight<()> {
            Self::note(&self.0, format!("stop {}", live.0));
            Box::pin(async {})
        }

        fn ended(_: &mut Sent) -> impl Future<Output = Failure> + Send {
            std::future::pending()
        }

        fn close(&self, live: Sent) -> Flight<()> {
            Self::note(&self.0, format!("close {}", live.0));
            Box::pin(async {})
        }

        fn said(end: End) -> End {
            end
        }

        fn emptied(&self) {
            Self::note(&self.0, "emptied".into());
        }
    }

    /// A sender of [`Told`], and its log.
    fn sender() -> (Sender<Told>, Told) {
        let log = Arc::default();
        (Sender::new(Told(Arc::clone(&log))), Told(log))
    }

    /// Takes `sender`'s next turn, opening and sending what it begins as the call does.
    async fn turn(sender: &mut Sender<Told>) -> Turn<Told> {
        let turn = tokio::time::timeout(std::time::Duration::from_secs(1), sender.next()).await.expect("a turn");
        if let Turn::Begun(begun) = &turn {
            let name = **begun;
            sender.send(Box::pin(async move { Ok(Sent(name)) }));
        }
        turn
    }

    /// `sender` sending `name`, from off.
    async fn live(sender: &mut Sender<Told>, log: &Told, name: &'static str) {
        sender.ask(name);
        assert!(matches!(turn(sender).await, Turn::Begun(_)));
        assert!(matches!(turn(sender).await, Turn::Sent));
        log.taken();
    }

    #[tokio::test]
    async fn an_ask_while_live_stops_the_one_sent_and_begins_itself_after() {
        let (mut sender, log) = sender();
        live(&mut sender, &log, "first").await;
        sender.ask("second");
        assert!(matches!(turn(&mut sender).await, Turn::Replaced(Was::Live)));
        assert!(matches!(turn(&mut sender).await, Turn::Begun(begun) if *begun == "second"));
        assert_eq!(log.taken(), ["stop first", "emptied", "begin second"]);
    }

    #[tokio::test]
    async fn an_ask_while_one_begins_lets_that_one_go_unsent_and_the_latest_ask_wins() {
        let (mut sender, log) = sender();
        sender.ask("first");
        sender.ask("second");
        sender.ask("third");
        assert!(matches!(turn(&mut sender).await, Turn::Replaced(Was::Unsent)));
        assert!(matches!(turn(&mut sender).await, Turn::Begun(begun) if *begun == "third"));
        assert_eq!(log.taken(), ["begin first", "emptied", "begin third"]);
    }

    #[tokio::test]
    async fn an_ask_while_one_is_being_sent_lets_it_go_once_it_is_and_begins_after() {
        let (mut sender, log) = sender();
        sender.ask("first");
        assert!(matches!(turn(&mut sender).await, Turn::Begun(_)));
        sender.ask("second");
        assert!(matches!(turn(&mut sender).await, Turn::Replaced(Was::Unsent)));
        assert_eq!(log.taken(), ["begin first", "stop first", "emptied", "begin second"]);
    }

    #[tokio::test]
    async fn a_stop_asked_twice_ends_it_once_and_lets_a_queued_ask_go() {
        let (mut sender, log) = sender();
        live(&mut sender, &log, "first").await;
        sender.ask("second");
        sender.stop();
        sender.stop();
        assert!(matches!(turn(&mut sender).await, Turn::Over(End::Stopped, Was::Live)));
        assert_eq!(log.taken(), ["stop first", "emptied"]);
        let next = tokio::time::timeout(std::time::Duration::from_millis(50), sender.next()).await;
        assert!(next.is_err(), "nothing more is sent");
    }

    #[tokio::test]
    async fn an_ask_after_a_stop_tells_that_stop_and_begins_after_it() {
        let (mut sender, log) = sender();
        live(&mut sender, &log, "first").await;
        sender.stop();
        sender.ask("second");
        assert!(matches!(turn(&mut sender).await, Turn::Over(End::Stopped, Was::Live)));
        assert!(matches!(turn(&mut sender).await, Turn::Begun(begun) if *begun == "second"));
        assert_eq!(log.taken(), ["stop first", "emptied", "begin second"]);
    }

    #[tokio::test]
    async fn a_denial_while_live_is_said_as_a_denial() {
        let (mut sender, log) = sender();
        live(&mut sender, &log, "first").await;
        sender.denied();
        assert!(matches!(turn(&mut sender).await, Turn::Over(End::Denied, Was::Live)));
    }

    #[tokio::test]
    async fn the_call_s_end_while_one_stops_finishes_the_stop_and_lets_go_of_what_it_showed() {
        let (mut sender, log) = sender();
        live(&mut sender, &log, "first").await;
        sender.stop();
        sender.end().await;
        assert_eq!(log.taken(), ["stop first", "emptied"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_send_that_never_comes_back_is_over_as_the_engine_s_stall_at_its_bound() {
        let (mut sender, log) = sender();
        sender.ask("first");
        assert!(matches!(sender.next().await, Turn::Begun(_)));
        let asked = Instant::now();
        sender.send(Box::pin(std::future::pending()));
        assert_eq!(sender.due(), Some(asked + WITHIN));
        assert!(sender.stalled(asked + WITHIN - Duration::from_millis(1)).is_none(), "not before its bound");
        let Some((stalled, within, turn)) = sender.stalled(asked + WITHIN) else { panic!("late at its bound") };
        assert_eq!((stalled.to_string().as_str(), within), ("starting to send", WITHIN));
        assert!(matches!(turn, Turn::Over(End::Failed(Failure::EngineStalled), Was::Unsent)));
        assert!(sender.stalled(asked + WITHIN * 2).is_none(), "said once");
        assert_eq!(sender.due(), None);
        sender.ask("second");
        assert!(matches!(sender.next().await, Turn::Begun(begun) if *begun == "second"), "and it may be asked again");
        assert_eq!(log.taken(), ["begin first", "begin second"]);
    }

    /// A kind whose stop never ends.
    struct Stuck;

    impl Kind for Stuck {
        type Ask = ();
        type Begun = ();
        type Live = ();
        type Said = End;

        const WHAT: &'static str = "the stuck thing";

        fn send_within() -> Duration {
            WITHIN
        }

        fn stop_within() -> Duration {
            WITHIN
        }

        fn begin(&self, (): (), _: Arc<AtomicBool>) -> JoinHandle<Result<(), Failure>> {
            tokio::spawn(async { Ok(()) })
        }

        fn let_go((): ()) {}

        fn abandon((): ()) -> Flight<()> {
            Box::pin(async {})
        }

        fn stop(&self, (): ()) -> Flight<()> {
            Box::pin(std::future::pending())
        }

        fn ended((): &mut ()) -> impl Future<Output = Failure> + Send {
            std::future::pending()
        }

        fn close(&self, (): ()) -> Flight<()> {
            Box::pin(std::future::pending())
        }

        fn said(end: End) -> End {
            end
        }

        fn emptied(&self) {}
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_that_never_comes_back_is_over_as_what_the_person_asked_at_its_bound_and_the_next_ask_begins() {
        let mut sender = Sender::new(Stuck);
        sender.ask(());
        assert!(matches!(sender.next().await, Turn::Begun(_)));
        sender.send(Box::pin(async { Ok(()) }));
        assert!(matches!(sender.next().await, Turn::Sent));
        let asked = Instant::now();
        sender.stop();
        sender.ask(());
        let Some((stalled, _, turn)) = sender.stalled(asked + WITHIN) else { panic!("late at its bound") };
        assert_eq!(stalled, Stalled::Stopping);
        assert!(matches!(turn, Turn::Over(End::Stopped, Was::Live)), "told as the stop it was");
        assert!(matches!(sender.next().await, Turn::Begun(_)), "the ask that waited for it begins");
    }
}
