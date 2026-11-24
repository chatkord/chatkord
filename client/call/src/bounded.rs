//! A step run as a task of its own under one bound: whoever asked takes its answer when it comes, and tells by the
//! time it is given whether the step is still out past its bound. A step that waits inside the engine or a device
//! therefore never holds up its asker, and letting go of it ends it.

use std::future::Future;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// A step whose task panicked: it did not end by answering, so what it held is in an unknown state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Panicked;

impl std::fmt::Display for Panicked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("A step of the call panicked.")
    }
}

impl std::error::Error for Panicked {}

/// A step under way, and when it is late.
#[derive(Debug)]
pub(crate) struct Bounded<T> {
    task: JoinHandle<T>,
    within: Duration,
    due: Instant,
}

impl<T: Send + 'static> Bounded<T> {
    /// Starts `step` at `now`, late once `within` has passed.
    pub(crate) fn spawn(now: Instant, within: Duration, step: impl Future<Output = T> + Send + 'static) -> Self {
        Self { task: tokio::spawn(step), within, due: now + within }
    }
}

impl<T> Bounded<T> {
    /// When the step is late.
    pub(crate) fn due(&self) -> Instant {
        self.due
    }

    /// How long the step was given.
    pub(crate) fn within(&self) -> Duration {
        self.within
    }

    /// Whether the step is still out at its bound, at `now`.
    pub(crate) fn late(&self, now: Instant) -> bool {
        now >= self.due
    }

    /// Resolves with what the step came to, or [`Panicked`] when its task ended without saying; cancel-safe.
    /// Once it has resolved the step is over, and is not asked again.
    pub(crate) async fn answer(&mut self) -> Result<T, Panicked> {
        (&mut self.task).await.map_err(|_| Panicked)
    }
}

impl<T> Drop for Bounded<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A step running as a task of its own, its bound kept by whoever decides when it is late. Letting go of it ends it.
#[derive(Debug)]
pub(crate) struct Task<T>(JoinHandle<T>);

impl<T: Send + 'static> Task<T> {
    /// Starts `step`.
    pub(crate) fn spawn(step: impl Future<Output = T> + Send + 'static) -> Self {
        Self(tokio::spawn(step))
    }
}

impl<T> Task<T> {
    /// Resolves with what the step came to, or [`Panicked`] when its task ended without saying; cancel-safe.
    pub(crate) async fn answer(&mut self) -> Result<T, Panicked> {
        (&mut self.0).await.map_err(|_| Panicked)
    }

    /// Ends the step where it next waits.
    pub(crate) fn end(&self) {
        self.0.abort();
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The earliest of `times`.
pub(crate) fn earliest(times: impl IntoIterator<Item = Option<Instant>>) -> Option<Instant> {
    times.into_iter().flatten().min()
}

/// Resolves at `due`; never, while nothing is.
pub(crate) async fn at(due: Option<Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

/// What an operator reads of a step still out at its bound.
pub(crate) fn stalled_said(what: impl std::fmt::Display, within: Duration) -> String {
    format!("The engine did not answer within {within:?}: {what}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHIN: Duration = Duration::from_secs(7);

    #[tokio::test(start_paused = true)]
    async fn a_step_that_answers_is_handed_back_and_one_still_out_is_late_only_from_its_bound() {
        let asked = Instant::now();
        let mut quick = Bounded::spawn(asked, WITHIN, async { 7 });
        assert_eq!(quick.answer().await, Ok(7));
        let never = Bounded::spawn(asked, WITHIN, std::future::pending::<()>());
        assert!(!never.late(asked + WITHIN - Duration::from_millis(1)), "not before its bound");
        assert!(never.late(asked + WITHIN), "late at its bound");
        assert_eq!((never.due(), never.within()), (asked + WITHIN, WITHIN));
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_let_go_is_ended_and_one_that_panics_is_told_as_a_fault() {
        let (held, mut let_go) = tokio::sync::mpsc::channel::<()>(1);
        let never = Bounded::spawn(Instant::now(), WITHIN, async move {
            let _held = held;
            std::future::pending::<()>().await;
        });
        drop(never);
        assert_eq!(let_go.recv().await, None, "what the step held went with it");
        let mut lost = Bounded::spawn(Instant::now(), WITHIN, async {
            panic!("a step that does not come back");
        });
        assert_eq!(lost.answer().await, Err(Panicked));
    }

    #[tokio::test(start_paused = true)]
    async fn the_earliest_bound_is_the_one_waited_for_and_none_waits_for_ever() {
        let now = Instant::now();
        let (near, far) = (now + Duration::from_secs(1), now + Duration::from_secs(9));
        assert_eq!(earliest([None, Some(far), Some(near)]), Some(near));
        assert_eq!(earliest([None, None]), None);
        at(Some(near)).await;
        assert_eq!(Instant::now(), near);
        assert!(tokio::time::timeout(Duration::from_secs(60), at(None)).await.is_err());
    }
}
