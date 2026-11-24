//! A voice's sound as ours: the microphone through its steps (`processing`) into the engine's audio
//! source, and every received track mixed by the binding's mixer, each at its own gain, onto the speaker, the mix
//! handed to the processor as what the echo will be. One thread mixes for an engine and never calls into a device:
//! the devices are a [`Sound`] the host hands in, opened by their names, and each is two halves. Its end (a
//! [`MicrophoneEnd`], a [`SpeakerEnd`]) is what the mix thread holds, only ring ends and atomics; its keeper (a
//! [`Microphone`], a [`Speaker`]) is everything that opens, moves and closes a stream, on a thread of its own per
//! device (`keeper`). A handle states what is asked and returns, and what became of it is read from
//! [`Sounding`].

use std::time::Duration;

mod devices;
mod handle;
mod mixer;
mod report;
mod shared;
mod thread;

pub use devices::{
    Listed, Listing, Microphone, MicrophoneEnd, MicrophoneOpened, Sound, Speaker, SpeakerEnd, SpeakerOpened,
};
pub(crate) use handle::{Chain, Heard};
pub use report::{ChainHealth, Sounding, Standing, Turn};
pub(crate) use shared::{Cadence, Counts, Shared, Which, lock};

/// The rate everything in the chain runs at.
pub const RATE_HZ: u32 = 48_000;
/// The samples of one channel in a frame, as the engine's frames count them.
const SAMPLES: u32 = RATE_HZ / 100;
/// The samples of one channel in a frame: the 10 ms the processor and the engine's source take.
pub const FRAME: usize = SAMPLES as usize;
/// A frame of the microphone.
pub type Mono = [i16; FRAME];
/// A frame of the speaker, interleaved left then right.
pub type Stereo = [i16; 2 * FRAME];

/// How many frames of a received track wait to be mixed before the oldest is dropped.
const TRACK_QUEUE: usize = 4;
/// How late a microphone test plays one's voice back: 120 ms, Sharkord's, in frames.
const TEST_LATE: usize = 12;
/// The longest a thread of the chain sleeps with nothing waking it while a device is asked or open, so the device's
/// keeping runs even in silence; with none, it sleeps until woken.
pub(crate) const IDLE: Duration = Duration::from_millis(10);
/// The longest dropping the chain waits for its thread, which a device that never answers would hold for ever; past
/// it the thread is left behind.
pub(crate) const STOP_BOUND: Duration = Duration::from_secs(2);
/// How long a thread of the chain may go without finishing a turn, a device asked of it, before it is said: a hundred
/// of its 10 ms turns. It is also how long a device's keeper may be in one call before it is given up on and a fresh
/// one tries what is asked again.
pub const STILL_AFTER: Duration = Duration::from_secs(1);
/// A turn that panics again this soon after one that did ends the chain: it is not restarted in a loop.
pub(crate) const RESTART_WITHIN: Duration = Duration::from_secs(60);

#[cfg(test)]
pub(crate) mod tests;
