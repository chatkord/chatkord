//! The engine's accessors that push nothing (`Voice::levels`, `Hearing::mixed`, `Speaking::is_capturing`, the stats)
//! read as the call reads them: once a beat. A wait is for what a look finds, bounded by a deadline; a measuring
//! window is a count of looks.

use std::future::Future;
use std::time::Duration;
use tokio::time::{Interval, MissedTickBehavior, interval, timeout};

/// The beat the tests' calls look at the levels on (`Join::level_every`), and the tests read them on.
pub(crate) const LEVELS_EVERY: Duration = Duration::from_millis(100);

/// A pace for asks a server counts, `every` apart at the least: a step that ran long delays the next, never bunches
/// them.
pub(crate) fn paced(every: Duration) -> Interval {
    let mut pace = interval(every);
    pace.set_missed_tick_behavior(MissedTickBehavior::Delay);
    pace
}

/// A look every beat.
pub(crate) struct Looks {
    beat: Interval,
}

impl Looks {
    /// A look every `beat`, the first at once; a look missed is skipped, never made up.
    pub(crate) fn every(beat: Duration) -> Self {
        let mut beat = interval(beat);
        beat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self { beat }
    }

    /// A look on the levels' beat.
    pub(crate) fn levels() -> Self {
        Self::every(LEVELS_EVERY)
    }

    /// The next look.
    pub(crate) async fn next(&mut self) {
        self.beat.tick().await;
    }

    /// What `read` finds at a look once it does, for no longer than `within`.
    pub(crate) async fn until<R>(&mut self, within: Duration, mut read: impl FnMut() -> Option<R>) -> Option<R> {
        timeout(within, async {
            loop {
                self.beat.tick().await;
                if let Some(found) = read() {
                    return found;
                }
            }
        })
        .await
        .ok()
    }

    /// The same for a read the engine answers later.
    pub(crate) async fn until_answered<R, F: Future<Output = Option<R>>>(
        &mut self,
        within: Duration,
        mut read: impl FnMut() -> F,
    ) -> Option<R> {
        timeout(within, async {
            loop {
                self.beat.tick().await;
                if let Some(found) = read().await {
                    return found;
                }
            }
        })
        .await
        .ok()
    }

    /// A measuring window of `length`, whole looks from now: what it measures is read before and after.
    pub(crate) async fn over(&mut self, length: Duration) {
        let period = self.beat.period();
        self.beat.reset();
        for _ in 0..length.as_nanos().div_ceil(period.as_nanos().max(1)) {
            self.beat.tick().await;
        }
    }
}
