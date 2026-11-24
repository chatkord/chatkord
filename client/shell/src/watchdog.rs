//! The two deadlines of a run: the start's, which ends a run that never showed its first picture, and the exit's, which
//! ends a run whose window closed and whose process did not follow. Each is a thread that waits on a channel; what it
//! does when its time passes is the caller's, so a test sees it without a process ending.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long a run has to show its first picture (or settle into the tray) before it is taken for stuck.
pub const START_WITHIN: Duration = Duration::from_secs(30);

/// How long a run has, from its window closing, to end: its connections part together within two seconds, the
/// blocking work still running is let go within two more, and nothing after that waits on anything of ours.
pub const EXIT_WITHIN: Duration = Duration::from_secs(5);

/// A deadline running, until it is [cancelled](Watch::cancel).
#[derive(Debug)]
pub struct Watch {
    cancel: Sender<()>,
    thread: JoinHandle<()>,
}

impl Watch {
    /// Ends the deadline unspent; returns once its thread is gone, so what it would have done never is.
    pub fn cancel(self) {
        let _ = self.cancel.send(());
        let _ = self.thread.join();
    }
}

/// Runs `fire` after `within` unless the returned [`Watch`] is cancelled (or dropped) first; none when the thread
/// cannot be made.
fn after(within: Duration, fire: impl FnOnce() + Send + 'static) -> Option<Watch> {
    let (cancel, cancelled) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("chatkord-watchdog".into())
        .spawn(move || {
            if cancelled.recv_timeout(within) == Err(RecvTimeoutError::Timeout) {
                fire();
            }
        })
        .ok()?;
    Some(Watch { cancel, thread })
}

/// Notes that the first picture was not shown within `within`, then calls `say` (the person's one sentence) and
/// `exit` (the process's end, with failure), unless the returned watch is cancelled first by the first picture.
pub fn start(
    within: Duration,
    say: impl FnOnce() + Send + 'static,
    exit: impl FnOnce() + Send + 'static,
) -> Option<Watch> {
    after(within, move || {
        report::note!(error, "the first picture was not shown in time", seconds = within.as_secs());
        say();
        exit();
    })
}

/// The exit's deadline: armed as the window closes, and disarmed (or dropped) only where the run still has a person to
/// tell something after its window went (a fatal sentence), which must not be cut off.
#[derive(Debug, Default)]
pub struct Exit(Mutex<Option<Watch>>);

impl Exit {
    /// Notes that the run did not end within `within` from now, then calls `exit`, unless disarmed first.
    pub fn arm(&self, within: Duration, exit: impl FnOnce() + Send + 'static) {
        let watch = after(within, move || {
            report::note!(error, "the run did not end in time");
            exit();
        });
        if watch.is_none() {
            report::note!(warn, "the exit's deadline could not be kept");
        }
        let earlier = std::mem::replace(&mut *self.armed(), watch);
        if let Some(earlier) = earlier {
            earlier.cancel();
        }
    }

    /// Ends the deadline unspent, if it is armed.
    pub fn disarm(&self) {
        let armed = self.armed().take();
        if let Some(watch) = armed {
            watch.cancel();
        }
    }

    fn armed(&self) -> MutexGuard<'_, Option<Watch>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    #[test]
    fn a_start_that_shows_no_picture_says_so_and_exits_and_one_that_does_not_is_left_alone() {
        let steps = Arc::new(AtomicU8::new(0));
        let (said, exited) = (Arc::clone(&steps), Arc::clone(&steps));
        let (done, finished) = mpsc::channel();
        let watch = start(
            Duration::ZERO,
            move || said.store(1, Ordering::SeqCst),
            move || {
                assert_eq!(exited.swap(2, Ordering::SeqCst), 1, "the person is told before the run ends");
                let _ = done.send(());
            },
        )
        .expect("a thread");
        assert!(finished.recv_timeout(Duration::from_secs(5)).is_ok(), "the deadline ended the run");
        drop(watch);

        let spent = Arc::new(AtomicU8::new(0));
        let (said, exited) = (Arc::clone(&spent), Arc::clone(&spent));
        let watch = start(
            Duration::from_secs(3600),
            move || said.store(1, Ordering::SeqCst),
            move || exited.store(2, Ordering::SeqCst),
        )
        .expect("a thread");
        watch.cancel();
        assert_eq!(spent.load(Ordering::SeqCst), 0, "a picture shown in time leaves the run alone");
    }

    #[test]
    fn a_run_that_does_not_end_after_its_window_closed_is_ended_unless_it_still_has_something_to_say() {
        let (done, finished) = mpsc::channel();
        let stuck = Exit::default();
        stuck.arm(Duration::ZERO, move || {
            let _ = done.send(());
        });
        assert!(finished.recv_timeout(Duration::from_secs(5)).is_ok(), "the deadline ended the run");

        let spent = Arc::new(AtomicU8::new(0));
        let exited = Arc::clone(&spent);
        let exit = Exit::default();
        exit.arm(Duration::from_secs(3600), move || exited.store(1, Ordering::SeqCst));
        exit.disarm();
        assert_eq!(spent.load(Ordering::SeqCst), 0, "a disarmed deadline ends nothing");
    }
}
