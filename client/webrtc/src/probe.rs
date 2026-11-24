//! A microphone level with no call, read from the microphone's own processed samples.

use crate::chain::Chain;
use crate::devices::{Capture, Devices, Test};
use crate::engine::Engine;
use crate::voice::VoiceRefused;
use std::sync::Arc;

/// The microphone being measured. Dropping it lets go of it.
pub struct Probe {
    chain: Arc<Chain>,
    /// On the engine's devices, so what is chosen here is what a call uses.
    devices: Devices,
    _capture: Capture,
    /// The test this probe started, which ends with it and leaves any other's running.
    test: Option<Test>,
}

impl Probe {
    /// Starts measuring the chosen microphone: it is asked for, and whether it opened is in
    /// [`Devices::sounding`].
    pub async fn open(engine: &Engine) -> Result<Self, VoiceRefused> {
        let devices = engine.devices().map_err(VoiceRefused::Sound)?;
        let capture = devices.capture().map_err(VoiceRefused::Sound)?;
        Ok(Self { chain: devices.chain(), devices, _capture: capture, test: None })
    }

    /// Plays the processed microphone back a little late on the speaker, until [`Probe::end_test`]; the speaker
    /// plays only while someone holds it ([`Devices::play`]).
    pub fn test(&mut self) {
        if self.test.is_none() {
            self.test = Some(self.devices.test());
        }
    }

    /// Stops playing the microphone back, unless another holds a test of its own.
    pub fn end_test(&mut self) {
        self.test = None;
    }

    /// The microphones and speakers, to list and to choose.
    pub fn devices(&self) -> &Devices {
        &self.devices
    }

    /// The loudest 10 ms of the microphone since this was last asked, before the gate, in dB below full scale; none
    /// before there is a reading.
    pub fn loudness(&self) -> Option<f32> {
        self.chain.loudness()
    }

    /// How loud the microphone is now, from 0 to 100, as a call would send it; none before there is a reading.
    pub async fn level(&self) -> Option<u8> {
        self.chain.level()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::Processing;

    #[tokio::test]
    async fn a_probe_that_ends_ends_its_own_test_and_no_other() {
        let engine = Engine::start(crate::chain::tests::silent(), Processing::PROCESSOR);
        let (mut testing, idle) =
            (Probe::open(&engine).await.expect("a probe"), Probe::open(&engine).await.expect("one"));
        testing.test();
        testing.test();
        assert_eq!(testing.devices().health().tests, 1, "one probe holds one test");
        drop(idle);
        assert_eq!(testing.devices().health().tests, 1, "a probe that tested nothing ends nothing");
        let devices = engine.devices().expect("ours");
        drop(testing);
        assert_eq!(devices.health().tests, 0, "a probe's test ends with it");
    }
}
