//! The thread a share's or a camera's pictures are encoded on: it runs on the blocking pool until it is ended, its
//! owner dropping it ends it, and a stop waits for it no longer than [`STOP_BOUND`].

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// The longest a stop waits for its worker: past a GPU wait's bound (one second) and a picture's wait (a tenth), a
/// worker still running is stuck in a driver and is left behind, the share or camera said over all the same.
pub(crate) const STOP_BOUND: Duration = Duration::from_secs(2);

/// How a worker is told to end, from any thread, whatever it is waiting on.
pub(crate) trait Halt: Send + Sync + 'static {
    /// Asks the worker to end; it may already have.
    fn end(&self);
}

/// The thread, the way it is ended, and how it ended.
pub(crate) struct Worker<T, H: Halt> {
    halt: Arc<H>,
    outcome: Outcome<T>,
}

/// How a worker ended: awaited from the thread, then kept once told, or taken by [`Worker::end`].
enum Outcome<T> {
    Awaited(oneshot::Receiver<T>),
    Told(Option<T>),
}

impl<T: Send + 'static, H: Halt> Worker<T, H> {
    /// Runs `body` on the blocking pool with `halt` and a slot for what it holds, which is let go only after it has
    /// told how it ended: a device whose release hangs never holds up what the person is told.
    pub(crate) fn spawn<L: Default>(halt: Arc<H>, body: impl FnOnce(&H, &mut L) -> T + Send + 'static) -> Self {
        let (told, outcome) = oneshot::channel();
        let ended = Arc::clone(&halt);
        drop(tokio::task::spawn_blocking(move || {
            let mut held = L::default();
            let _ = told.send(body(&ended, &mut held));
            drop(held);
        }));
        Self { halt, outcome: Outcome::Awaited(outcome) }
    }

    /// The way it is ended.
    pub(crate) fn halt(&self) -> &H {
        &self.halt
    }

    /// How it ended, once it has; never while it runs, and cancel-safe. None when the thread ended without telling
    /// (it panicked), or [`Worker::end`] took it.
    pub(crate) async fn ended(&mut self) -> Option<&T> {
        if let Outcome::Awaited(told) = &mut self.outcome {
            self.outcome = Outcome::Told(told.await.ok());
        }
        match &self.outcome {
            Outcome::Told(ended) => ended.as_ref(),
            Outcome::Awaited(_) => None,
        }
    }

    /// Ends it and waits until it has stopped, at most [`STOP_BOUND`]: how it ended, or none past the bound, the
    /// thread left behind.
    pub(crate) async fn end(mut self) -> Option<T> {
        self.halt.end();
        if tokio::time::timeout(STOP_BOUND, self.ended()).await.is_err() {
            report::note!(
                warn,
                "a worker still ran after it was ended; it is left behind",
                bound_ms = report::millis(STOP_BOUND),
            );
            return None;
        }
        match std::mem::replace(&mut self.outcome, Outcome::Told(None)) {
            Outcome::Told(ended) => ended,
            Outcome::Awaited(_) => None,
        }
    }
}

/// A worker its owner lets go of is ended too: a runtime shutting down waits for its blocking tasks, so one left
/// running would hold the process open.
impl<T, H: Halt> Drop for Worker<T, H> {
    fn drop(&mut self) {
        self.halt.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    #[derive(Default)]
    struct Flag(AtomicBool);

    impl Halt for Flag {
        fn end(&self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    /// Runs until it is ended, a few milliseconds a turn.
    fn until_ended(flag: &Flag, (): &mut ()) -> u32 {
        let mut turns = 0;
        while !flag.0.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
            turns += 1;
        }
        turns
    }

    #[test]
    fn a_worker_whose_owner_drops_it_ends_so_its_runtime_can_end() {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let worker = runtime.block_on(async { Worker::spawn(Arc::new(Flag::default()), until_ended) });
        let (ended, told) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(worker);
            drop(runtime);
            let _ = ended.send(());
        });
        assert!(told.recv_timeout(Duration::from_secs(2)).is_ok(), "the runtime waited on a worker nobody holds");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_ended_tells_how_it_ended() {
        let worker = Worker::spawn(Arc::new(Flag::default()), |flag: &Flag, (): &mut ()| {
            until_ended(flag, &mut ());
            "ended by its halt"
        });
        assert_eq!(worker.end().await, Some("ended by its halt"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_stuck_past_the_bound_is_left_behind_within_it() {
        let worker = Worker::spawn(Arc::new(Flag::default()), |_: &Flag, (): &mut ()| {
            std::thread::sleep(STOP_BOUND * 2);
        });
        let asked = Instant::now();
        assert!(worker.end().await.is_none(), "a stuck worker is said left behind");
        assert!(asked.elapsed() < STOP_BOUND + Duration::from_millis(500), "the stop waited {:?}", asked.elapsed());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_a_worker_holds_is_let_go_after_it_told_how_it_ended() {
        struct Hangs;
        impl Drop for Hangs {
            fn drop(&mut self) {
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        let mut worker = Worker::spawn(Arc::new(Flag::default()), |_: &Flag, held: &mut Option<Hangs>| {
            *held = Some(Hangs);
            7
        });
        let told = tokio::time::timeout(Duration::from_secs(1), worker.ended()).await;
        assert_eq!(told.ok().flatten(), Some(&7), "told before what it held is let go");
    }
}
