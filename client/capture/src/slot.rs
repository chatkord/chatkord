//! The hand-off between a capture's own thread and the caller.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::{CaptureError, Frame};

/// What the capture has made, whether or not anyone took it: how many pictures, and how far apart the system drew
/// them at the widest.
#[derive(Default)]
pub(crate) struct Arrivals {
    delivered: u64,
    /// When the system drew the newest picture; it outlives a reading of the gap, so the next gap starts from it.
    stamped: Option<Instant>,
    widest_gap: Duration,
}

impl Arrivals {
    /// A picture the system drew at `stamp` arrived, with `skipped` more that went straight back unread.
    fn arrive(&mut self, stamp: Instant, skipped: u64) {
        self.delivered += 1 + skipped;
        if let Some(last) = self.stamped.replace(stamp) {
            self.widest_gap = self.widest_gap.max(stamp.saturating_duration_since(last));
        }
    }

    /// The widest gap between two pictures since it was last taken, which starts the next from none.
    fn take_widest_gap(&mut self) -> Duration {
        std::mem::take(&mut self.widest_gap)
    }
}

/// What the capture thread leaves for the caller: the newest frame, or the reason it ended.
#[derive(Default)]
pub(crate) struct Slot {
    pub(crate) frame: Option<Frame>,
    pub(crate) ended: Option<CaptureError>,
    /// Frames replaced before anyone took them.
    pub(crate) dropped: u64,
    pub(crate) arrivals: Arrivals,
}

/// Shared between the caller and the capture's own thread. A [`Frame`] is never dropped while `slot` is held:
/// dropping one can take a lock the capture's thread holds while it takes `slot` (PipeWire's loop does).
pub(crate) struct Shared {
    pub(crate) slot: Mutex<Slot>,
    pub(crate) arrived: Condvar,
}

impl Shared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { slot: Mutex::new(Slot::default()), arrived: Condvar::new() })
    }

    /// A frame arrived: it replaces one nobody took. `skipped` more went straight back unread.
    pub(crate) fn put(&self, frame: Frame, skipped: u64) {
        let stamp = frame.captured();
        let replaced = match self.slot.lock() {
            Ok(mut slot) => {
                slot.dropped += skipped + u64::from(slot.frame.is_some());
                slot.arrivals.arrive(stamp, skipped);
                slot.frame.replace(frame)
            }
            // A caller panicked holding the slot: the frame is the only thing left to drop.
            Err(_) => return,
        };
        self.arrived.notify_one();
        // Outside the lock: dropping a frame sends its buffer home.
        drop(replaced);
    }

    /// Frames that arrived and were replaced, or went back unread, before anyone took them.
    pub(crate) fn dropped(&self) -> u64 {
        self.slot.lock().map_or(0, |slot| slot.dropped)
    }

    /// Frames the system made, those nobody took included.
    pub(crate) fn delivered(&self) -> u64 {
        self.slot.lock().map_or(0, |slot| slot.arrivals.delivered)
    }

    /// The widest gap between two frames' own times since it was last taken.
    pub(crate) fn take_widest_gap(&self) -> Duration {
        self.slot.lock().map_or(Duration::ZERO, |mut slot| slot.arrivals.take_widest_gap())
    }

    /// The capture is over; a frame already in the slot is still delivered first.
    pub(crate) fn end(&self, why: CaptureError) {
        if let Ok(mut slot) = self.slot.lock() {
            slot.ended.get_or_insert(why);
        }
        self.arrived.notify_all();
    }

    /// The newest frame, waiting up to `wait` for one; `Ok(None)` is a quiet screen, not a failure. A frame that
    /// arrived before the end is still delivered first.
    pub(crate) fn take(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        let guard = self.slot.lock().map_err(|_| CaptureError::Stream)?;
        let (mut slot, _) = self
            .arrived
            .wait_timeout_while(guard, wait, |s| s.frame.is_none() && s.ended.is_none())
            .map_err(|_| CaptureError::Stream)?;
        match (slot.frame.take(), slot.ended) {
            (Some(frame), _) => Ok(Some(frame)),
            (None, Some(why)) => Err(why),
            (None, None) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn every_picture_the_system_made_is_counted_delivered_those_sent_back_unread_too() {
        let mut arrivals = Arrivals::default();
        let start = Instant::now();
        arrivals.arrive(start, 0);
        arrivals.arrive(start + 16 * MS, 2);
        assert_eq!(arrivals.delivered, 4);
    }

    #[test]
    fn the_widest_gap_is_the_largest_between_two_pictures_and_the_first_has_none() {
        let mut arrivals = Arrivals::default();
        let start = Instant::now();
        arrivals.arrive(start, 0);
        assert_eq!(arrivals.take_widest_gap(), Duration::ZERO, "one picture has no gap");
        arrivals.arrive(start + 16 * MS, 0);
        arrivals.arrive(start + 100 * MS, 0);
        arrivals.arrive(start + 120 * MS, 0);
        assert_eq!(arrivals.take_widest_gap(), 84 * MS);
    }

    #[test]
    fn taking_the_widest_gap_starts_it_over_and_the_next_gap_runs_from_the_last_picture() {
        let mut arrivals = Arrivals::default();
        let start = Instant::now();
        arrivals.arrive(start, 0);
        arrivals.arrive(start + 50 * MS, 0);
        assert_eq!(arrivals.take_widest_gap(), 50 * MS);
        assert_eq!(arrivals.take_widest_gap(), Duration::ZERO, "read once");
        arrivals.arrive(start + 70 * MS, 0);
        assert_eq!(arrivals.take_widest_gap(), 20 * MS, "from the last picture, not from the reading");
    }

    #[test]
    fn a_stamp_that_runs_backwards_is_no_gap() {
        let mut arrivals = Arrivals::default();
        let start = Instant::now();
        arrivals.arrive(start + 10 * MS, 0);
        arrivals.arrive(start, 0);
        assert_eq!(arrivals.take_widest_gap(), Duration::ZERO);
    }
}
