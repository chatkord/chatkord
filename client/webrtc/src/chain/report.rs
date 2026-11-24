//! What the chain says of itself: what became of each device, what its thread is doing, and its counts.

use crate::devices::DevicesRefused;
use crate::keeper::Keeper;

/// What became of a device the chain was asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standing {
    /// Nobody holds it.
    #[default]
    Closed,
    /// It is held, and the chain's thread has not said yet what the host made of what is asked.
    Opening,
    /// The host opened what is asked.
    Open,
    /// The host refused what is asked, which is tried again only once what is asked changes: a new hold, a new
    /// choice. A device chosen before a refused choice may still sound.
    Refused(DevicesRefused),
    /// The host held its keeper in one call past [`super::STILL_AFTER`]: a fresh keeper tries what is asked again, and this
    /// stands until the device answers or what is asked changes.
    Unanswered,
    /// The keeper was held past [`super::STILL_AFTER`] and no fresh one could take its place, as the one given up on
    /// before has not returned: nothing more is tried of what is asked until that one returns. It stands until the
    /// device answers or what is asked changes.
    Hung,
}

/// What became of both devices; copied out of its watch, never held across a call into the chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sounding {
    /// The microphone.
    pub microphone: Standing,
    /// The speaker.
    pub speaker: Standing,
}

/// What the chain's mix thread is doing now, so a turn that never ends is named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Turn {
    /// Waiting for a frame or an ask.
    Idle,
    /// Taking the microphone's frames through its steps.
    Processing,
    /// Mixing what is received onto the speaker.
    Mixing,
}

impl Turn {
    pub(super) const ALL: [Self; 3] = [Self::Idle, Self::Processing, Self::Mixing];

    pub(super) fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Processing => 1,
            Self::Mixing => 2,
        }
    }

    pub(super) fn of(code: u8) -> Self {
        Self::ALL.into_iter().find(|turn| turn.code() == code).unwrap_or(Self::Idle)
    }
}

impl std::fmt::Display for Turn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Idle => "idle",
            Self::Processing => "processing the microphone",
            Self::Mixing => "mixing what is received",
        })
    }
}

/// How the chain is doing at one moment, by its own counts. Each count only grows while its device stays open;
/// the caller takes differences. A frame is 10 ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainHealth {
    /// Turns the mix thread has finished: one that stops growing is a thread held in [`ChainHealth::doing`].
    pub turns: u64,
    /// Microphone frames processed and handed to the engine.
    pub captured: u64,
    /// Frames handed to the speaker.
    pub played: u64,
    /// Frames taken from the received tracks, before they are mixed.
    pub received: u64,
    /// Received tracks being mixed now.
    pub tracks: usize,
    /// Holders of the speaker now: none plays nothing whatever arrives.
    pub speaker_holds: usize,
    /// Microphone tests held now.
    pub tests: usize,
    /// Times a turn panicked and the thread dropped its ends and steps, its keepers opening the devices again.
    pub restarts: u32,
    /// What the mix thread is doing now.
    pub doing: Turn,
    /// The microphone's keeper.
    pub microphone: Keeper,
    /// The speaker's keeper.
    pub speaker: Keeper,
    /// Frames the microphone open now dropped because the chain read them too late.
    pub microphone_dropped: u64,
    /// Frames the speaker open now dropped because too many already waited.
    pub speaker_dropped: u64,
    /// Times the speaker open now ran out and played silence.
    pub speaker_underruns: u64,
    /// How long a frame handed to the speaker takes to be heard, in milliseconds, as the last turn read it.
    pub speaker_delay_ms: u32,
    /// The most of it any turn read since the health was last read: a reader that keeps the largest it sees has
    /// the most since its own last line.
    pub speaker_delay_most_ms: u32,
    /// Whether the mix thread runs; once it does not, nothing asked is answered and every hold is refused.
    pub alive: bool,
}
