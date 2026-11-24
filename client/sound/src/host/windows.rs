//! The owner thread's system on Windows: its COM apartment and the one device enumerator, which the listing, the
//! applications, the endpoint notifications and the chimes all use.

use super::{Call, Served};
use crate::share::{Listen, SoundError};
use crate::speaker::Stage;
use crate::voice::{Listing, VoiceError};
use crate::watch::{Endpoints, Unavailable, Wake};
use tokio::sync::watch;

/// What the owner thread holds, the stage's apartment and enumerator.
pub(crate) struct Host {
    stage: Stage,
    served: Served,
}

impl Host {
    /// Enters the thread's apartment and opens the enumerator; none where Windows has no audio to open.
    pub(crate) fn enter(served: Served) -> Option<Self> {
        let stage = Stage::enter().ok()?;
        served.record(Call::Owner);
        Some(Self { stage, served })
    }

    /// The active endpoints Windows lists now.
    pub(crate) fn listing(&self) -> Result<Listing, VoiceError> {
        self.served.record(Call::Listing);
        crate::voice::listing(self.stage.devices())
    }

    /// The applications playing, listed at `now`.
    pub(crate) fn apps(&self, now: std::time::Instant) -> Result<Listen, SoundError> {
        self.served.record(Call::Apps);
        Listen::start(self.stage.devices(), now)
    }

    /// Registers for the endpoints' notifications; each one ticks `changed` and wakes the owner through `wake`.
    pub(crate) fn listen(&self, changed: watch::Sender<u64>, wake: Wake) -> Result<Endpoints, Unavailable> {
        self.served.record(Call::Watch);
        Endpoints::listen(self.stage.devices(), changed, wake).map_err(|_| Unavailable)
    }

    /// Where the chimes play from.
    pub(crate) fn stage(&self) -> &Stage {
        &self.stage
    }
}
