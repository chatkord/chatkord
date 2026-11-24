//! The owner thread's system on Linux: PipeWire's connections are made by what uses them.

use super::{Call, Served};
use crate::share::{Listen, SoundError};
use crate::speaker::Stage;
use crate::voice::{Listing, VoiceError};

/// What the owner thread holds: the chimes' stage.
pub(crate) struct Host {
    stage: Stage,
    served: Served,
}

impl Host {
    pub(crate) fn enter(served: Served) -> Self {
        served.record(Call::Owner);
        Self { stage: Stage::default(), served }
    }

    /// The devices PipeWire lists now.
    pub(crate) fn listing(&self) -> Result<Listing, VoiceError> {
        self.served.record(Call::Listing);
        crate::voice::listing()
    }

    /// A listen for the applications playing, begun at `now`.
    pub(crate) fn apps(&self, now: std::time::Instant) -> Result<Listen, SoundError> {
        self.served.record(Call::Apps);
        Listen::start(now)
    }

    /// The registration of the devices' watch, recorded.
    pub(crate) fn watching(&self) {
        self.served.record(Call::Watch);
    }

    /// Where the chimes play from.
    pub(crate) fn stage(&self) -> &Stage {
        &self.stage
    }
}
