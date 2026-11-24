//! PipeWire's registry, kept: a sound server that restarts ends the connection, so the owner thread opens it again
//! after a growing wait, and each opening tells the holders to list the devices anew.

use super::{Unavailable, Wake};
use crate::voice::RETRY;
use std::time::Instant;
use tokio::sync::watch;

/// The registry, and the wait before the next try while it is lost.
pub(super) struct Kept {
    registry: Option<pw::Registry>,
    changed: watch::Sender<u64>,
    wake: Wake,
    tries: u32,
    retry_at: Option<Instant>,
}

impl Kept {
    /// Listens now, so a device without PipeWire is told at once.
    pub(super) fn open(changed: watch::Sender<u64>, wake: Wake) -> Result<Self, Unavailable> {
        let registry = registry(&changed, &wake)?;
        Ok(Self { registry: Some(registry), changed, wake, tries: 0, retry_at: None })
    }

    /// Looks at the registry: a lost one is waited for, then opened again, each wait longer than the last.
    pub(super) fn keep(&mut self, now: Instant) {
        if self.registry.as_ref().is_some_and(|registry| !registry.lost()) {
            self.tries = 0;
            self.retry_at = None;
            return;
        }
        self.registry = None;
        match self.retry_at {
            None => {
                self.retry_at = Some(now + RETRY.wait(self.tries.saturating_add(1)));
                self.tries += 1;
            }
            Some(at) if now >= at => {
                self.registry = registry(&self.changed, &self.wake).ok();
                // A server still away is waited for again; one back is looked at when it next calls.
                self.retry_at = self.registry.is_none().then(|| now + RETRY.wait(self.tries.saturating_add(1)));
                self.tries += 1;
            }
            Some(_) => {}
        }
    }

    /// When the lost registry is next tried.
    pub(super) fn due(&self) -> Option<Instant> {
        self.retry_at
    }
}

/// A registry whose every change counts on `changed` and wakes the owner.
fn registry(changed: &watch::Sender<u64>, wake: &Wake) -> Result<pw::Registry, Unavailable> {
    let (tell, wake) = (changed.clone(), wake.clone());
    pw::Registry::sound(move || {
        tell.send_modify(|count| *count = count.wrapping_add(1));
        wake();
    })
    .map_err(|_| Unavailable)
}
