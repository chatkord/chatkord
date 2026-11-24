//! When a share answers an ask for a key frame: every key frame, whatever made it (the start, a reopen, an ask),
//! answers the asks before it, and none is made for an ask within [`KEY_FRAME_EVERY`] of the last, so the asks of
//! every viewer and of the engine cost at most one a second beside the share's own.

use std::time::Duration;

/// The least time between a key frame and one made for an ask, as the server spaces a screen's asks from its viewers.
pub(crate) const KEY_FRAME_EVERY: Duration = Duration::from_secs(1);

/// What became of an ask, as the share's run takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// The encoder is to make a key frame now.
    Now,
    /// Made at [`KEY_FRAME_EVERY`] after the last key frame, unless one comes before then.
    Deferred,
    /// Answered by the key frame an earlier ask already waits for.
    Merged,
}

/// The last key frame and the ask that waits for its time, on the share's own clock (its time since it began).
#[derive(Debug, Default)]
pub(crate) struct KeyGate {
    /// When the last key frame went out, or was asked of the encoder, whichever came later.
    last: Option<Duration>,
    /// When the first ask that waits for its time arrived.
    waiting: Option<Duration>,
}

impl KeyGate {
    /// An ask arrived at `now`.
    pub(crate) fn ask(&mut self, now: Duration) -> Asked {
        if self.last.is_some_and(|last| now < last + KEY_FRAME_EVERY) {
            if self.waiting.is_some() {
                return Asked::Merged;
            }
            self.waiting = Some(now);
            return Asked::Deferred;
        }
        self.last = Some(now);
        Asked::Now
    }

    /// Whether the ask that waited is to be answered at `now`, its time after the last key frame come: the encoder is
    /// to make one.
    pub(crate) fn due(&mut self, now: Duration) -> bool {
        let due = self.waiting.is_some() && self.last.is_none_or(|last| now >= last + KEY_FRAME_EVERY);
        if due {
            self.waiting = None;
            self.last = Some(now);
        }
        due
    }

    /// A key frame went out at `at`, of whatever cause. Whether it answered an ask that waited, which then makes none.
    pub(crate) fn sent(&mut self, at: Duration) -> bool {
        self.last = Some(self.last.map_or(at, |last| last.max(at)));
        let answered = self.waiting.is_some_and(|arrived| arrived <= at);
        if answered {
            self.waiting = None;
        }
        answered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_first_ask_and_one_a_second_after_the_last_key_frame_are_answered_at_once() {
        let mut gate = KeyGate::default();
        assert_eq!(gate.ask(Duration::ZERO), Asked::Now, "no key frame yet");
        assert!(!gate.sent(16 * MS), "nothing waited");
        assert_eq!(gate.ask(16 * MS + KEY_FRAME_EVERY), Asked::Now);
    }

    #[test]
    fn asks_within_a_second_of_any_key_frame_make_none_more() {
        let mut gate = KeyGate::default();
        // The share's first, or a reopen's: no ask made it.
        assert!(!gate.sent(Duration::ZERO));
        assert_eq!(gate.ask(100 * MS), Asked::Deferred);
        for at in [300, 600, 900] {
            assert_eq!(gate.ask(at * MS), Asked::Merged, "at {at} ms");
            assert!(!gate.due(at * MS), "not before its time: {at} ms");
        }
        assert!(gate.due(KEY_FRAME_EVERY), "the asks in the second cost one key frame, at its end");
        assert!(!gate.due(KEY_FRAME_EVERY * 2), "and only one");
        assert_eq!(gate.ask(KEY_FRAME_EVERY + 10 * MS), Asked::Deferred, "the one asked of the encoder counts");
    }

    #[test]
    fn a_deferred_ask_fires_a_second_after_the_last_key_frame() {
        let mut gate = KeyGate::default();
        assert!(!gate.sent(500 * MS));
        assert_eq!(gate.ask(700 * MS), Asked::Deferred);
        assert!(!gate.due(500 * MS + KEY_FRAME_EVERY - MS));
        assert!(gate.due(500 * MS + KEY_FRAME_EVERY));
    }

    #[test]
    fn an_ask_after_which_a_key_frame_went_out_is_dropped() {
        let mut gate = KeyGate::default();
        assert_eq!(gate.ask(Duration::ZERO), Asked::Now);
        assert_eq!(gate.ask(5 * MS), Asked::Deferred, "asked before the key frame asked for is out");
        assert!(gate.sent(16 * MS), "the key frame came after it, so it answers it");
        assert!(!gate.due(KEY_FRAME_EVERY * 2), "nothing more to make");
    }
}
