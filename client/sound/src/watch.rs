//! The sound devices of this system coming and going, as one wake-up: whoever asked lists them again. One listening
//! serves the process, since a second on the same daemon would hear the same things twice; the owner thread
//! ([`crate::Sound`]) keeps it while anyone holds a [`Changes`].

#[cfg(windows)]
mod endpoints;
#[cfg(windows)]
pub(crate) use endpoints::Endpoints;
#[cfg(target_os = "linux")]
mod linux;

use crate::host::Host;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

/// How long the devices must be quiet before a change is told: a headset plugged in is several nodes arriving at
/// once, and the list is only right when they have.
pub const DEVICE_SETTLE: Duration = Duration::from_millis(500);

/// This system has no sound devices to listen to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this system has no sound devices to listen to")
    }
}

impl std::error::Error for Unavailable {}

/// What the owner thread is woken with when the sound server's registry changes or is lost.
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

/// The one listening, on the owner thread.
pub(crate) struct Listening {
    changed: watch::Sender<u64>,
    #[cfg(target_os = "linux")]
    registry: linux::Kept,
    #[cfg(windows)]
    _endpoints: Endpoints,
}

impl Listening {
    /// Starts listening to the system, which talks to it. Each change wakes the owner through `wake`.
    #[cfg(target_os = "linux")]
    pub(crate) fn start(host: &Host, wake: Wake) -> Result<Self, Unavailable> {
        host.watching();
        let (changed, _) = watch::channel(0u64);
        let registry = linux::Kept::open(changed.clone(), wake)?;
        Ok(Self { changed, registry })
    }

    /// Starts listening to the system, which talks to it. The endpoints' notifications wake the owner through `wake`.
    #[cfg(windows)]
    pub(crate) fn start(host: &Host, wake: Wake) -> Result<Self, Unavailable> {
        let (changed, _) = watch::channel(0u64);
        let endpoints = host.listen(changed.clone(), wake)?;
        Ok(Self { changed, _endpoints: endpoints })
    }

    /// Whether anyone holds a [`Changes`] of it.
    pub(crate) fn held(&self) -> bool {
        self.changed.receiver_count() > 0
    }

    /// A new holder's wake-ups. The holder that started the listening is told once of the devices already there,
    /// whether or not the system reports them itself (PipeWire's registry does; Windows tells only what changes).
    pub(crate) fn changes(&self, started: bool) -> Changes {
        let mut seen = self.changed.subscribe();
        if started {
            seen.mark_changed();
        }
        Changes { seen }
    }

    /// Opens again a registry the sound server ended, once its wait is over; called at each turn of the owner.
    pub(crate) fn keep(&mut self, now: Instant) {
        #[cfg(target_os = "linux")]
        self.registry.keep(now);
        // Windows' notifications need no looking after.
        #[cfg(windows)]
        let _ = (&self.changed, now);
    }

    /// When the registry lost is next tried, if one is.
    pub(crate) fn due(&self) -> Option<Instant> {
        #[cfg(target_os = "linux")]
        return self.registry.due();
        #[cfg(windows)]
        return None;
    }
}

/// Wake-ups for one holder: the sound devices changed. Dropping the last holder ends the listening.
pub struct Changes {
    seen: watch::Receiver<u64>,
}

impl Changes {
    /// Waits until the devices change and then stay quiet for [`DEVICE_SETTLE`]. Changes before the first call, and
    /// the devices already there when listening began, are told once.
    pub async fn changed(&mut self) {
        if quiet_after_change(&mut self.seen, DEVICE_SETTLE).await.is_err() {
            // The listening lives as long as this does, so its channel cannot close.
            std::future::pending::<()>().await;
        }
    }

    /// Sends what `wake` makes to `to` each time the devices change, until `to` is closed.
    pub fn forward<T: Send + 'static>(mut self, to: mpsc::Sender<T>, wake: impl Fn() -> T + Send + 'static) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = self.changed() => {
                        if to.send(wake()).await.is_err() {
                            break;
                        }
                    }
                    () = to.closed() => break,
                }
            }
        });
    }
}

/// Waits for a change, then for `settle` with none.
async fn quiet_after_change(seen: &mut watch::Receiver<u64>, settle: Duration) -> Result<(), watch::error::RecvError> {
    seen.changed().await?;
    while tokio::time::timeout(settle, seen.changed()).await.is_ok_and(|more| more.is_ok()) {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTLE: Duration = Duration::from_millis(60);

    #[tokio::test]
    async fn a_burst_of_changes_is_one_wake_up_told_when_it_has_settled() {
        let (tell, mut seen) = watch::channel(0u64);
        let started = tokio::time::Instant::now();
        tokio::spawn(async move {
            for _ in 0..4 {
                tell.send_modify(|count| *count += 1);
                tokio::time::sleep(SETTLE / 3).await;
            }
            // Kept open, so the wait below ends by settling, never by the channel closing.
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        quiet_after_change(&mut seen, SETTLE).await.expect("a wake-up");
        assert!(started.elapsed() >= SETTLE, "told only after a quiet spell: {:?}", started.elapsed());
        let again = tokio::time::timeout(SETTLE * 3, quiet_after_change(&mut seen, SETTLE)).await;
        assert!(again.is_err(), "the burst was one wake-up, not four");
    }

    #[tokio::test]
    async fn a_closed_channel_is_an_error_not_a_wake_up() {
        let (tell, mut seen) = watch::channel(0u64);
        drop(tell);
        assert!(quiet_after_change(&mut seen, SETTLE).await.is_err());
    }
}
