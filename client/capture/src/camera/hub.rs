//! Cameras coming and going, as each system's own notifications tell them, to everyone who lists the cameras; and each
//! open camera known by its device's id, so one that is removed is stopped and one camera has one opener in this
//! process. One hub serves the process, whichever system tells it.

use super::Stopper;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A listener to wake, by its watch's id.
type Listener = (u64, Arc<dyn Fn() + Send + Sync>);

/// The listeners to wake, and the open cameras by their device's id.
pub(super) struct Hub {
    next: AtomicU64,
    listeners: Mutex<Vec<Listener>>,
    open: Mutex<Vec<(u64, String, Stopper)>>,
    /// Told when an open camera is let go.
    freed: Condvar,
}

/// How long an open waits for this process's last holder of the same camera to let it go: a holder that was told to
/// stop lets go within a picture's time, and a system refuses a second opener of one camera, or a second format.
const HANDOVER: Duration = Duration::from_secs(2);

static HUB: Hub = Hub {
    next: AtomicU64::new(0),
    listeners: Mutex::new(Vec::new()),
    open: Mutex::new(Vec::new()),
    freed: Condvar::new(),
};

/// The process's hub.
pub(super) fn hub() -> &'static Hub {
    &HUB
}

impl Hub {
    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// Wakes `changed` whenever a camera comes or goes, until [`Hub::unlisten`] with the id returned.
    pub(super) fn listen(&self, changed: impl Fn() + Send + Sync + 'static) -> u64 {
        let id = self.id();
        self.listeners.lock().unwrap_or_else(PoisonError::into_inner).push((id, Arc::new(changed)));
        id
    }

    /// Stops waking the listener of `id`.
    pub(super) fn unlisten(&self, id: u64) {
        self.listeners.lock().unwrap_or_else(PoisonError::into_inner).retain(|(each, _)| *each != id);
    }

    /// Wakes every listener, outside the lock, so one that lists the cameras again never waits on it.
    pub(super) fn changed(&self) {
        let listeners: Vec<_> = self
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            listener();
        }
    }

    /// Stops every open camera of the device `device`, which went away.
    pub(super) fn removed(&self, device: &str) {
        let open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stopped = 0_u64;
        for (_, _, stopper) in open.iter().filter(|(_, each, _)| each == device) {
            stopper.stop();
            stopped += 1;
        }
        report::note!(info, "a camera was removed", stopped = stopped);
    }

    /// Holds the camera of `device`, to be stopped by `stopper` if it is removed, once this process's last holder of
    /// it has let it go (within [`HANDOVER`]): a test's camera let go as a call opens it is never refused as busy.
    /// [`Hub::release`] with the id returned lets it go.
    pub(super) fn hold(&self, device: &str, stopper: Stopper) -> u64 {
        let id = self.id();
        let until = Instant::now() + HANDOVER;
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        while open.iter().any(|(_, each, _)| each == device) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            open = self.freed.wait_timeout(open, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        open.push((id, device.to_string(), stopper));
        id
    }

    /// Lets go of the camera held as `id`.
    pub(super) fn release(&self, id: u64) {
        self.open.lock().unwrap_or_else(PoisonError::into_inner).retain(|(each, _, _)| *each != id);
        self.freed.notify_all();
    }
}
