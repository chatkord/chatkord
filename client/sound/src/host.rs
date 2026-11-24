//! What the owner thread ([`crate::Sound`]) holds of the system for its whole life: on Windows one COM apartment and
//! one device enumerator that the listing, the applications, the endpoint notifications and the chimes all use; on
//! Linux nothing of its own, each of them being PipeWire's.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::Host;
#[cfg(windows)]
pub(crate) use windows::Host;

#[cfg(any(test, feature = "testing"))]
use std::sync::{Arc, Mutex};
#[cfg(any(test, feature = "testing"))]
use std::thread::{self, ThreadId};

/// An ask the owner thread serves, named for [`Served`].
#[cfg_attr(
    not(any(test, feature = "testing")),
    expect(unreachable_pub, reason = "public only to the tests that ask which thread served")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// The thread started, and holds the system.
    Owner,
    /// The devices listed.
    Listing,
    /// The endpoint notifications registered.
    Watch,
    /// A chime started.
    Chime,
    /// The applications listed.
    Apps,
}

/// Which thread each kind of ask ran on, kept only for the tests that check the owner serves them all itself.
#[cfg_attr(
    not(any(test, feature = "testing")),
    expect(unreachable_pub, reason = "public only to the tests that ask which thread served")
)]
#[derive(Clone, Debug, Default)]
pub struct Served {
    #[cfg(any(test, feature = "testing"))]
    seen: Arc<Mutex<Vec<(Call, ThreadId)>>>,
}

impl Served {
    pub(crate) fn record(&self, call: Call) {
        #[cfg(any(test, feature = "testing"))]
        if let Ok(mut seen) = self.seen.lock() {
            seen.push((call, thread::current().id()));
        }
        #[cfg(not(any(test, feature = "testing")))]
        let _ = call;
    }

    /// The threads `call` ran on, in order.
    #[cfg(any(test, feature = "testing"))]
    pub fn threads(&self, call: Call) -> Vec<ThreadId> {
        self.seen.lock().map_or_else(
            |_| Vec::new(),
            |seen| seen.iter().filter(|(made, _)| *made == call).map(|(_, thread)| *thread).collect(),
        )
    }
}
