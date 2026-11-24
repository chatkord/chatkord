//! Encoders whose device was lost, kept for the rest of the process. An encoder on a device a driver reset removed can
//! neither be drained nor freed: on the RX 9060 XT after a forced reset (four times), the drain never returned from
//! `amfrtdrv64`, and with it skipped, freeing the codec context read a null pointer in AMD's D3D11 driver
//! (`amdxx64`, by `AmdDxExtCreate11` into `D3DKMTOpenResource`), ending the process. What a lost device held is a few
//! frames and a context, once for each loss, so it is held rather than freed.

use super::Parts;
use std::sync::{Mutex, PoisonError};

/// What an encoder on a lost device held, whole.
struct Retired(#[expect(dead_code, reason = "held only so it is never dropped")] Parts);

// SAFETY: a retired encoder is only moved into the list below and never used or dropped again: no thread touches what
// it holds, so the thread it was last used on does not matter.
unsafe impl Send for Retired {}

/// What is never dropped: values put in stay, unreachable, until the process ends.
struct Graveyard<T>(Mutex<Vec<T>>);

impl<T> Graveyard<T> {
    const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    /// Keeps `value`; how many are kept now.
    fn bury(&self, value: T) -> usize {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.push(value);
        kept.len()
    }
}

/// Every encoder retired in this process, which is one for each device it lost.
static KEPT: Graveyard<Retired> = Graveyard::new();

/// Keeps `retired` unreachable until the process ends, and says so once.
pub(super) fn keep(parts: Parts) {
    let kept = KEPT.bury(Retired(parts));
    report::note!(warn, "an encoder on a lost device was kept, not freed", kept = kept);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Says when it is dropped.
    struct Flag(Arc<AtomicBool>);

    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn what_is_buried_is_counted_and_never_dropped() {
        let yard = Graveyard::new();
        let dropped = Arc::new(AtomicBool::new(false));
        assert_eq!(yard.bury(Flag(Arc::clone(&dropped))), 1);
        assert_eq!(yard.bury(Flag(Arc::clone(&dropped))), 2, "one for each loss");
        assert!(!dropped.load(Ordering::SeqCst), "held, not freed");
    }
}
