//! The audio devices of a call, listed and chosen on ours ([`crate::chain`]) by the sound server's ids. Echo
//! cancellation needs capture and playout in one place, so both are held on the one [`Engine`] a call's connections
//! are made from.

use crate::chain::{Chain, ChainHealth, Listed, Listing, Sounding, Turn};
use crate::engine::Engine;
use crate::processing::Processing;
use std::sync::Arc;
use tokio::sync::watch;

/// An input or output the sound server lists. A device is asked for by its id, and no id means the system's
/// default: `None` is how a choice is kept and passed everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What a person reads. The default's is `default: ` and the name of the device it follows now.
    pub name: String,
    /// What it is chosen by, which two devices of one name never share: the sound server's name for it on Linux,
    /// the endpoint's id on Windows. None for the entry that follows the system's default, chosen with no id.
    pub id: Option<String>,
}

impl Device {
    /// Whether this is the entry that follows the system's default.
    pub fn is_default(&self) -> bool {
        self.id.is_none()
    }

    /// The name a person knows it by: for the default, the device it follows now.
    pub fn label(&self) -> &str {
        if self.is_default() { self.name.strip_prefix(DEFAULT_PREFIX).unwrap_or(&self.name) } else { &self.name }
    }

    /// How to ask for this device again: its id, or none for the default.
    pub fn asked_as(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

/// What is put before the name of the device the default entry follows.
const DEFAULT_PREFIX: &str = "default: ";

/// Why a device step did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicesRefused {
    /// The sound server is not there, or the chain's thread is not: it never started, or it stopped after
    /// panicking twice within a minute.
    Unavailable,
    /// No device by that name is present now.
    Missing,
    /// The sound server would not use the device.
    Refused,
}

/// A hold on the call's devices: it lists and chooses them. What it plays and captures are [`Playout`] and
/// [`Capture`], so letting go of a hold never silences a call. Nothing here waits for a device: each ask returns at
/// once, and what became of it is read from [`Devices::sounding`].
pub struct Devices {
    chain: Arc<Chain>,
}

impl Devices {
    pub(crate) fn open(engine: &Engine) -> Result<Self, DevicesRefused> {
        Ok(Self { chain: Arc::clone(engine.chain()?) })
    }

    /// The microphones: the default first, then the system's order. It waits on the sound server for its listing.
    pub fn inputs(&self) -> Vec<Device> {
        self.lists().0
    }

    /// The speakers: the default first, then the system's order. It waits on the sound server for its listing.
    pub fn outputs(&self) -> Vec<Device> {
        self.lists().1
    }

    /// The microphones and the speakers of one listing. It waits on the sound server.
    pub fn lists(&self) -> (Vec<Device>, Vec<Device>) {
        self.chain.listing().map_or_else(|_| (Vec::new(), Vec::new()), lists)
    }

    /// Asks for the microphone called `asked` (none: the default) as the one the call captures from, moving a
    /// microphone already running. Refused here only when no such device is listed; whether the device took it is
    /// in [`Devices::sounding`].
    pub fn choose_input(&self, asked: Option<&str>) -> Result<(), DevicesRefused> {
        if asked.is_some() && !self.inputs().iter().any(|device| device.asked_as() == asked) {
            return Err(DevicesRefused::Missing);
        }
        self.choose_input_listed(asked)
    }

    /// Asks for the microphone `asked` as [`Devices::choose_input`] does, for an id the caller has just read from a
    /// listing: it does not list again, so it waits on nothing.
    pub fn choose_input_listed(&self, asked: Option<&str>) -> Result<(), DevicesRefused> {
        self.chain.choose_microphone(asked);
        Ok(())
    }

    /// Asks for the speaker called `asked` (none: the default) as the one the call plays to, moving a speaker
    /// already running. Refused here only when no such device is listed; whether the device took it is in
    /// [`Devices::sounding`].
    pub fn choose_output(&self, asked: Option<&str>) -> Result<(), DevicesRefused> {
        if asked.is_some() && !self.outputs().iter().any(|device| device.asked_as() == asked) {
            return Err(DevicesRefused::Missing);
        }
        self.choose_output_listed(asked)
    }

    /// Asks for the speaker `asked` as [`Devices::choose_output`] does, for an id the caller has just read from a
    /// listing: it does not list again, so it waits on nothing.
    pub fn choose_output_listed(&self, asked: Option<&str>) -> Result<(), DevicesRefused> {
        self.chain.choose_speaker(asked);
        Ok(())
    }

    /// What became of the microphone and the speaker, told as the devices' keepers answer each ask. Copy the value
    /// out: a borrow held across another call here would wait on itself.
    pub fn sounding(&self) -> watch::Receiver<Sounding> {
        self.chain.sounding()
    }

    /// What the chain's mix thread is doing now: the step a stalled turn holds it in. What each device's keeper is
    /// doing is in [`Devices::health`].
    pub fn doing(&self) -> Turn {
        self.chain.doing()
    }

    /// Plays what the call receives to the chosen speaker until the guard is dropped; the speaker stops with the
    /// last guard. It only asks: the speaker's opening, or its refusal, is in [`Devices::sounding`]. Refused here
    /// only when the chain's thread is gone.
    pub fn play(&self) -> Result<Playout, DevicesRefused> {
        self.chain.play()?;
        Ok(Playout { chain: Arc::clone(&self.chain) })
    }

    /// Captures from the chosen microphone until the guard is dropped; it closes with the last guard. It only asks:
    /// the microphone's opening, or its refusal, is in [`Devices::sounding`]. Refused here only when the chain's
    /// thread is gone.
    pub fn capture(&self) -> Result<Capture, DevicesRefused> {
        self.chain.capture()?;
        Ok(Capture { chain: Arc::clone(&self.chain) })
    }

    /// Plays the processed microphone back on the speaker a little late, in place of the call, until the guard is
    /// dropped; the test ends with the last guard. The speaker plays only while someone holds it
    /// ([`Devices::play`]).
    pub fn test(&self) -> Test {
        self.chain.test();
        Test { chain: Arc::clone(&self.chain) }
    }

    /// How the chain is doing now, by its own counts. It takes the most delay since the last read, so the chain's one
    /// watch reads it; anyone else asks [`Devices::alive`].
    pub fn health(&self) -> ChainHealth {
        self.chain.health()
    }

    /// Whether the chain's thread runs; once it does not, nothing asked is answered and every hold is refused.
    pub fn alive(&self) -> bool {
        self.chain.alive()
    }

    /// Processes the microphone as `processing` says, from its next frame, in a call and out of one.
    pub fn process(&self, processing: Processing) {
        self.chain.process(processing);
    }

    /// Our chain.
    pub(crate) fn chain(&self) -> Arc<Chain> {
        Arc::clone(&self.chain)
    }

    /// Whether a microphone is held and open now, for whichever reason.
    pub fn is_capturing(&self) -> bool {
        self.chain.is_capturing()
    }
}

/// The microphones and the speakers of one `listing`, each with the default first.
pub fn lists(listing: Listing) -> (Vec<Device>, Vec<Device>) {
    (
        ours(listing.microphones, listing.default_microphone.as_deref()),
        ours(listing.speakers, listing.default_speaker.as_deref()),
    )
}

/// Our devices as listed: the default first, labelled by the device it follows now, then each by its id.
fn ours(listed: Vec<Listed>, default: Option<&str>) -> Vec<Device> {
    let follows = listed.iter().find(|device| Some(device.id.as_str()) == default).map(|device| device.label.clone());
    let first = Device { name: format!("{DEFAULT_PREFIX}{}", follows.unwrap_or_default()), id: None };
    let named = listed.into_iter().map(|device| Device { name: device.label, id: Some(device.id) });
    std::iter::once(first).chain(named).collect()
}

/// The speaker running for a call. Dropping the last one stops it.
pub struct Playout {
    chain: Arc<Chain>,
}

impl Playout {
    /// Whether the speaker is open: false until its keeper has opened it.
    pub fn is_active(&self) -> bool {
        self.chain.is_playing()
    }
}

impl Drop for Playout {
    fn drop(&mut self) {
        self.chain.release_play();
    }
}

/// The microphone running for a call. Dropping the last one stops it.
pub struct Capture {
    chain: Arc<Chain>,
}

impl Capture {
    /// Whether the microphone is open: false until its keeper has opened it.
    pub fn is_active(&self) -> bool {
        self.chain.is_capturing()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.chain.release_capture();
    }
}

/// A microphone test running. Dropping the last one ends it.
pub struct Test {
    chain: Arc<Chain>,
}

impl Drop for Test {
    fn drop(&mut self) {
        self.chain.release_test();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, id: Option<&str>) -> Device {
        Device { name: name.into(), id: id.map(str::to_string) }
    }

    #[test]
    fn the_default_is_known_by_the_device_it_follows_and_asked_for_without_an_id() {
        let default = device("default: Headset", None);
        assert_eq!(default.label(), "Headset");
        assert_eq!(default.asked_as(), None);
        let named = device("default: Headset", Some("usb-headset"));
        assert_eq!(named.label(), "default: Headset", "only the default entry carries the prefix");
        assert_eq!(named.asked_as(), Some("usb-headset"));
    }
}
