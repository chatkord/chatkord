//! The one watch on an engine's voice chain, shared by whoever looks at it (a call, the probe): its health line in
//! the log every ten seconds, and a warning once when its thread stands still, however many look.

use crate::voice_log::VoiceLog;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::time::Instant;
use webrtc::{ChainHealth, Devices};

/// The watch on one engine's chain. A clone is the same watch.
#[derive(Clone, Debug, Default)]
pub struct ChainWatch(Arc<Mutex<VoiceLog>>);

impl ChainWatch {
    /// Looks at the chain behind `devices` at `now`, writing its line and its warning when one is due; how the chain
    /// is, for the looker to tell a chain that stopped.
    pub(crate) fn look(&self, devices: &Devices, now: Instant) -> ChainHealth {
        let health = devices.health();
        let sounding = *devices.sounding().borrow();
        let (line, held_in, microphone, speaker) = {
            let mut log = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            (
                log.look(health, now),
                log.stalled(health, sounding, now),
                log.microphone_stalled(health, sounding, now),
                log.speaker_stalled(health, sounding, now),
            )
        };
        if let Some(line) = line {
            line.log();
        }
        if let Some(doing) = held_in {
            report::note!(
                warn,
                "the voice chain's thread has not turned for its bound",
                doing = doing,
                still_after_ms = report::millis(crate::voice_log::STILL_AFTER),
            );
        }
        if let Some(doing) = microphone {
            report::note!(
                warn,
                "the microphone has not answered for its bound; the call goes on without it",
                doing = doing,
                still_after_ms = report::millis(crate::voice_log::STILL_AFTER),
            );
        }
        if let Some(doing) = speaker {
            report::note!(
                warn,
                "the speaker has not answered for its bound; the call goes on silent",
                doing = doing,
                still_after_ms = report::millis(crate::voice_log::STILL_AFTER),
            );
        }
        health
    }
}
