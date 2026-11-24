//! The clock of a made-up source fed from a thread of its own: it blocks to the next frame's time, its own beat, not
//! a wait for anything to happen.

use std::time::Duration;

/// Blocks the feeding thread for `length`, to the next frame's time.
#[expect(clippy::disallowed_methods, reason = "a made-up source's frame clock on a thread of its own, not a wait")]
pub(crate) fn frame_clock(length: Duration) {
    std::thread::sleep(length);
}
