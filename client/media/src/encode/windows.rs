//! Windows' parts of a session.

use super::*;

impl Session {
    /// The device this session encodes on, for a capture to draw into: it stays the same for the session's life, as
    /// another encoder on the same device keeps it.
    pub fn gpu(&self) -> Option<capture::Gpu> {
        self.parts.feed.device().and_then(HwDevice::gpu)
    }
}
