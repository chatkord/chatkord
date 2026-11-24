//! The engine's report once a look, asked off the call's loop: what is sent (the share, the camera, a tone's
//! loudness) and both connections, joined into one answer the loop takes when it comes, as it takes every other ask's, so nothing the
//! engine is slow to answer holds up a mute or a leave. One look is out at a time, a task of its own; one the engine
//! has not answered for [`LATE`] is said once, by the parts still waiting, and one still out at its bound is dropped.

use crate::bounded::{Bounded, Panicked};
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::time::Instant;
use webrtc::{Connection, Half, SendStats, Step, VoiceRefused, Watching};

/// How long a look may go unanswered before it is said: three looks' time.
pub(super) const LATE: Duration = Duration::from_secs(3);

/// What a look is given past the engine's own bound on a report, for its answer to reach the loop.
const HANDING_OVER: Duration = Duration::from_secs(1);

/// The longest a look may be out: its parts are asked together, each a report the engine bounds itself.
pub(super) fn within() -> Duration {
    Step::Statistics(Half::Send).bound().max(Step::Statistics(Half::Receive).bound()) + HANDING_OVER
}

/// The engine's answer to one look.
pub(super) struct Looked {
    /// The share's report and which share it is of; none when nothing was shared when it was asked.
    pub(super) share: Option<(u64, SendStats)>,
    /// The camera's, likewise.
    pub(super) camera: Option<(u64, SendStats)>,
    /// A tone's loudness, none where the engine has no reading; none outside, when no tone was sent.
    pub(super) tone: Option<Option<u8>>,
    /// Both connections'.
    pub(super) connection: Result<Connection, VoiceRefused>,
    /// How each picture watched is taken; none when nothing is watched or the engine refused.
    pub(super) watching: Option<Watching>,
}

/// The names of a look's parts not answered yet.
type Waiting = Arc<Mutex<Vec<&'static str>>>;

/// A look asked and not yet answered.
pub(super) struct Look {
    answer: Bounded<Looked>,
    asked: Instant,
    waiting: Waiting,
    told: bool,
}

impl Look {
    /// Asks the engine at `now` for the share's report and the camera's, each with which video it is of, a tone's
    /// loudness where one is sent, both connections', and how each picture is taken while one is watched.
    pub(super) fn ask(
        now: Instant,
        share: Option<(u64, impl Future<Output = SendStats> + Send + 'static)>,
        camera: Option<(u64, impl Future<Output = SendStats> + Send + 'static)>,
        tone: Option<impl Future<Output = Option<u8>> + Send + 'static>,
        connection: impl Future<Output = Result<Connection, VoiceRefused>> + Send + 'static,
        watching: Option<impl Future<Output = Result<Watching, VoiceRefused>> + Send + 'static>,
    ) -> Self {
        let waiting = Waiting::default();
        let share = part("the share's report", share, &waiting);
        let camera = part("the camera's report", camera, &waiting);
        let tone = asked("the tone's level", tone, &waiting);
        let connection = named("the connections' report", connection, &waiting);
        let watching = asked("the watched pictures' report", watching, &waiting);
        let answer = Bounded::spawn(now, within(), async move {
            let (share, camera, tone, connection, watching) = tokio::join!(share, camera, tone, connection, watching);
            Looked { share, camera, tone, connection, watching: watching.and_then(Result::ok) }
        });
        Self { answer, asked: now, waiting, told: false }
    }

    /// The parts still unanswered when the look is still out at its bound at `now`: it is to be dropped, and the
    /// next asked.
    pub(super) fn stalled(&self, now: Instant) -> Option<Vec<&'static str>> {
        self.answer.late(now).then(|| self.waiting.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }

    /// The parts still unanswered, once, when the look has waited [`LATE`] at `now`.
    pub(super) fn late(&mut self, now: Instant) -> Option<Vec<&'static str>> {
        if self.told || now.saturating_duration_since(self.asked) < LATE {
            return None;
        }
        self.told = true;
        Some(self.waiting.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
}

/// Resolves with the answer to the look under way, [`Panicked`] when its task ended without one; never, while no
/// look is out. Dropping it leaves the look under way; once it resolves the look is over.
pub(super) async fn answered(look: &mut Option<Look>) -> Result<Looked, Panicked> {
    match look {
        Some(look) => look.answer.answer().await,
        None => std::future::pending().await,
    }
}

/// `ask`, its `name` waiting until it answers.
fn named<F: Future>(name: &'static str, ask: F, waiting: &Waiting) -> impl Future<Output = F::Output> + use<F> {
    waiting.lock().unwrap_or_else(PoisonError::into_inner).push(name);
    let waiting = Arc::clone(waiting);
    async move {
        let answer = ask.await;
        waiting.lock().unwrap_or_else(PoisonError::into_inner).retain(|each| *each != name);
        answer
    }
}

/// The answer to `ask`, its `name` waiting until then; none when nothing was asked.
fn asked<F: Future>(
    name: &'static str,
    ask: Option<F>,
    waiting: &Waiting,
) -> impl Future<Output = Option<F::Output>> + use<F> {
    let ask = ask.map(|ask| named(name, ask, waiting));
    async move {
        match ask {
            Some(ask) => Some(ask.await),
            None => None,
        }
    }
}

/// The answer to a `report` asked of a video, with which video it is of; none when nothing was asked.
fn part<F: Future>(
    name: &'static str,
    report: Option<(u64, F)>,
    waiting: &Waiting,
) -> impl Future<Output = Option<(u64, F::Output)>> + use<F> {
    asked(name, report.map(|(of, ask)| async move { (of, ask.await) }), waiting)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No tone sent: nothing is asked of its loudness.
    const NO_TONE: Option<std::future::Ready<Option<u8>>> = None;

    /// Nothing watched: nothing is asked of how a picture is taken.
    const NO_WATCH: Option<std::future::Ready<Result<Watching, VoiceRefused>>> = None;

    fn stats() -> SendStats {
        SendStats { bytes_sent: 7, ..SendStats::default() }
    }

    #[tokio::test]
    async fn a_look_is_answered_whole_with_which_video_each_report_is_of() {
        let ready = async { stats() };
        let mut look = Some(Look::ask(
            Instant::now(),
            Some((4, ready)),
            None::<(u64, std::future::Ready<SendStats>)>,
            NO_TONE,
            async { Err(VoiceRefused::Engine) },
            NO_WATCH,
        ));
        let looked = answered(&mut look).await.expect("an answer");
        assert_eq!(looked.share, Some((4, stats())));
        assert_eq!(looked.camera, None, "nothing asked of a camera not sent");
        assert_eq!(looked.tone, None, "nor of a tone not sent");
        assert!(looked.connection.is_err());
        assert_eq!(looked.watching, None, "nor of pictures not watched");
    }

    #[tokio::test]
    async fn a_tone_s_loudness_is_a_part_of_the_look_named_while_it_waits() {
        let asked = Instant::now();
        let no_video = || None::<(u64, std::future::Ready<SendStats>)>;
        let refused = || async { Err::<Connection, _>(VoiceRefused::Engine) };
        let mut answered_look =
            Some(Look::ask(asked, no_video(), no_video(), Some(async { Some(40_u8) }), refused(), NO_WATCH));
        assert_eq!(answered(&mut answered_look).await.expect("an answer").tone, Some(Some(40)));
        let mut unread =
            Some(Look::ask(asked, no_video(), no_video(), Some(async { None::<u8> }), refused(), NO_WATCH));
        assert_eq!(answered(&mut unread).await.expect("an answer").tone, Some(None), "asked, and no reading yet");
        let mut out =
            Look::ask(asked, no_video(), no_video(), Some(std::future::pending::<Option<u8>>()), refused(), NO_WATCH);
        tokio::task::yield_now().await;
        assert_eq!(out.late(asked + LATE), Some(vec!["the tone's level"]), "the loop waits for none of it");
    }

    #[tokio::test]
    async fn a_look_unanswered_is_said_once_by_the_parts_still_waiting() {
        let asked = Instant::now();
        let mut look = Look::ask(
            asked,
            Some((1, async { stats() })),
            Some((2, std::future::pending::<SendStats>())),
            NO_TONE,
            std::future::pending::<Result<Connection, VoiceRefused>>(),
            NO_WATCH,
        );
        let mut pending = Some(look);
        // The answered part is set aside as it answers; the others wait.
        assert!(tokio::time::timeout(Duration::from_millis(10), answered(&mut pending)).await.is_err());
        look = pending.take().expect("still out");
        assert_eq!(look.late(asked + LATE / 2), None, "not late yet");
        assert_eq!(look.late(asked + LATE), Some(vec!["the camera's report", "the connections' report"]));
        assert_eq!(look.late(asked + LATE * 2), None, "said once");
    }

    #[tokio::test(start_paused = true)]
    async fn a_look_that_never_answers_is_dropped_at_its_bound_and_not_before() {
        let asked = Instant::now();
        let (held, mut let_go) = tokio::sync::mpsc::channel::<()>(1);
        let never = async move {
            let _held = held;
            std::future::pending::<Result<Connection, VoiceRefused>>().await
        };
        let none = None::<(u64, std::future::Ready<SendStats>)>;
        let look = Look::ask(asked, Some((1, async { stats() })), none, NO_TONE, never, NO_WATCH);
        // The task runs: its answered part is set aside, the other waits.
        tokio::task::yield_now().await;
        assert_eq!(look.stalled(asked + LATE), None, "late is said, not dropped");
        assert_eq!(look.stalled(asked + within() - Duration::from_millis(1)), None);
        assert_eq!(look.stalled(asked + within()), Some(vec!["the connections' report"]), "by what never answered");
        assert!(within() > webrtc::ENGINE_WITHIN, "the engine's own bound on a report comes first");
        drop(look);
        assert_eq!(let_go.recv().await, None, "a look dropped ends its asks");
    }

    #[tokio::test]
    async fn no_look_under_way_never_answers() {
        let mut none = None;
        assert!(tokio::time::timeout(Duration::from_millis(20), answered(&mut none)).await.is_err());
    }
}
