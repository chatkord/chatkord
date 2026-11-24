//! The idle time asked of the system every [`POLL_EVERY`], and the transitions it makes.

use super::{POLL_EVERY, State, moved};
use crate::Unavailable;
use std::time::Duration;
use tokio::time::{Instant, MissedTickBehavior, interval_at};

/// Where the idle time is read from.
enum Source {
    #[cfg(target_os = "linux")]
    Mutter(super::mutter::Monitor),
    #[cfg(target_os = "linux")]
    X11,
    #[cfg(windows)]
    Windows,
}

/// The idle time, read on a tick.
pub(super) struct Poll {
    source: Source,
    state: State,
    /// A reading taken when the source was opened, which the first call tells before it waits.
    first: Option<Duration>,
    ticks: tokio::time::Interval,
}

impl Poll {
    async fn open(source: Source) -> Result<Self, Unavailable> {
        let first = Self::read(&source).await.ok_or(Unavailable)?;
        let mut ticks = interval_at(Instant::now() + POLL_EVERY, POLL_EVERY);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Ok(Self { source, state: State::Active, first: Some(first), ticks })
    }

    #[cfg(target_os = "linux")]
    pub(super) async fn mutter() -> Result<Self, Unavailable> {
        Self::open(Source::Mutter(super::mutter::Monitor::open().await?)).await
    }

    #[cfg(target_os = "linux")]
    pub(super) async fn x11() -> Result<Self, Unavailable> {
        Self::open(Source::X11).await
    }

    #[cfg(windows)]
    pub(super) async fn windows() -> Result<Self, Unavailable> {
        Self::open(Source::Windows).await
    }

    async fn read(source: &Source) -> Option<Duration> {
        match source {
            #[cfg(target_os = "linux")]
            Source::Mutter(monitor) => monitor.idle_for().await,
            #[cfg(target_os = "linux")]
            Source::X11 => crate::blocking(|| super::x11::idle_for().ok_or(Unavailable)).await.ok(),
            #[cfg(windows)]
            Source::Windows => crate::blocking(|| super::windows::idle_for().ok_or(Unavailable)).await.ok(),
        }
    }

    /// Waits for the next transition. A reading that fails is skipped: the next tick asks again.
    pub(super) async fn next(&mut self) -> Option<State> {
        let mut reading = self.first.take();
        loop {
            if let Some(idle_for) = reading
                && let Some(next) = moved(self.state, idle_for)
            {
                self.state = next;
                return Some(next);
            }
            self.ticks.tick().await;
            reading = Self::read(&self.source).await;
        }
    }
}
