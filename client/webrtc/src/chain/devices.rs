//! The seam to the host's devices: what a microphone and a speaker are to the chain, and what is listed of them.

use super::{Mono, Stereo};
use crate::devices::DevicesRefused;
use std::thread::Thread;
use std::time::{Duration, Instant};

pub use sound::voice::{Listed, Listing};

/// A microphone the host opened, by its name: the half that opens, chooses and keeps its stream. It runs on its own
/// keeper thread and may be held in the sound server for as long as the server holds it.
pub trait Microphone: Send {
    /// Opens a failed stream again, hands over to a device chosen anew; called on every look of its keeper.
    fn keep(&mut self, now: Instant);
    /// Takes `device` (none: the default) in place of this one once it runs.
    fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused>;
    /// Whether its stream is open: its sound flows, or it is being linked.
    fn is_open(&self) -> bool;
    /// Frames it dropped since it was opened because the chain read them too late; a host that cannot tell says
    /// none.
    fn dropped(&self) -> u64 {
        0
    }
}

/// The half of a microphone the mix thread reads: it never waits on the sound server, and follows the stream that
/// its [`Microphone`] has made the one in use.
pub trait MicrophoneEnd: Send {
    /// The oldest frame waiting, if any.
    fn frame(&mut self) -> Option<Mono>;
    /// How long a sound took from the device to being read now.
    fn delay(&self) -> Duration;
}

/// A speaker the host opened, by its name: the half that opens, chooses and keeps its stream. It runs on its own
/// keeper thread and may be held in the sound server for as long as the server holds it.
pub trait Speaker: Send {
    /// Opens a failed stream again, hands over to a device chosen anew, follows the clock frames come on; called on
    /// every look of its keeper.
    fn keep(&mut self, now: Instant);
    /// Takes `device` (none: the default) in place of this one once it runs.
    fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused>;
    /// Whether its stream is open: its sound flows, or it is being linked.
    fn is_open(&self) -> bool;
    /// Since it was opened: the frames it dropped because too many already waited, and the times it ran out and
    /// played silence; a host that cannot tell says none.
    fn losses(&self) -> (u64, u64) {
        (0, 0)
    }
}

/// The half of a speaker the mix thread feeds: it never waits on the sound server, and follows the stream that its
/// [`Speaker`] has made the one in use.
pub trait SpeakerEnd: Send {
    /// Queues a frame to play.
    fn play(&mut self, frame: &Stereo);
    /// How long a frame played now takes to reach the device.
    fn delay(&self) -> Duration;
}

/// A microphone opened: its keeper and the end that reads it.
pub type MicrophoneOpened = Result<(Box<dyn Microphone>, Box<dyn MicrophoneEnd>), DevicesRefused>;
/// A speaker opened: its keeper and the end that feeds it.
pub type SpeakerOpened = Result<(Box<dyn Speaker>, Box<dyn SpeakerEnd>), DevicesRefused>;

/// The devices a host opens for the chain.
pub trait Sound: Send + Sync {
    /// Opens the microphone `device` (none: the default) and the end that reads it; each frame that arrives unparks
    /// `wake`.
    fn microphone(&self, device: Option<&str>, wake: Thread) -> MicrophoneOpened;
    /// Opens the speaker `device` (none: the default) at `now` and the end that feeds it.
    fn speaker(&self, device: Option<&str>, now: Instant) -> SpeakerOpened;
    /// The devices there now. It may block on the sound server.
    fn listing(&self) -> Result<Listing, DevicesRefused>;
    /// Asks the system to run the calling thread, the chain's mix thread, ahead of ordinary work; what it returns is
    /// kept until that thread ends. A host with nobody to ask returns nothing.
    fn ahead(&self) -> Option<Box<dyn std::any::Any>> {
        None
    }
}
