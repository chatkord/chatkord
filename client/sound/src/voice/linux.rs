//! A call's microphone and speaker on PipeWire (`pw::Voice`), by `node.name`.

use std::time::Duration;

use super::{Capture, Health, Listed, Listing, Playback, RATE, VoiceError};

/// The microphones and speakers the sound server lists now, by the names they are opened by, and its defaults. It
/// blocks for up to two seconds: run it off an async thread.
pub fn listing() -> Result<Listing, VoiceError> {
    let listing = pw::devices().map_err(|why| match why {
        pw::PulseError::Unavailable(_) => VoiceError::Unavailable,
        pw::PulseError::Unreached | pw::PulseError::TimedOut => VoiceError::Stream,
    })?;
    let listed = |devices: Vec<pw::Listed>| {
        devices.into_iter().map(|device| Listed { id: device.name, label: device.description }).collect()
    };
    Ok(Listing {
        microphones: listed(listing.microphones),
        speakers: listed(listing.speakers),
        default_microphone: listing.default_microphone,
        default_speaker: listing.default_speaker,
    })
}

fn refused(why: pw::Error) -> VoiceError {
    match why {
        pw::Error::Unavailable(_) => VoiceError::Unavailable,
        pw::Error::Refused(_) | pw::Error::Pod => VoiceError::Stream,
    }
}

fn health(state: pw::State, voice: &pw::Voice) -> Health {
    match state {
        _ if voice.lost() => Health::Stopped,
        pw::State::Streaming => Health::Running,
        pw::State::Connecting | pw::State::Paused => Health::Starting,
        pw::State::Error | pw::State::Unconnected => Health::Stopped,
    }
}

/// A microphone stream on its own connection, so a lost server is seen by it alone.
pub(super) struct Input {
    voice: pw::Voice,
    stream: pw::Microphone,
}

impl Capture for Input {
    fn open(device: Option<&str>, hear: impl FnMut(&[i16]) + Send + 'static) -> Result<Self, VoiceError> {
        let voice = pw::Voice::open().map_err(refused)?;
        let stream = voice.microphone(device, RATE, hear).map_err(refused)?;
        Ok(Self { voice, stream })
    }

    fn health(&self) -> Health {
        health(self.stream.state(), &self.voice)
    }

    fn delay(&self) -> Duration {
        self.stream.timing(RATE).map_or(Duration::ZERO, |timing| timing.delay)
    }
}

/// A speaker stream on its own connection.
pub(super) struct Output {
    voice: pw::Voice,
    stream: pw::Speaker,
}

impl Playback for Output {
    fn open(device: Option<&str>, fill: impl FnMut(&mut [i16]) + Send + 'static) -> Result<Self, VoiceError> {
        let voice = pw::Voice::open().map_err(refused)?;
        let stream = voice.speaker(device, RATE, fill).map_err(refused)?;
        Ok(Self { voice, stream })
    }

    fn health(&self) -> Health {
        health(self.stream.state(), &self.voice)
    }

    fn delay(&self) -> Duration {
        self.stream.timing(RATE).map_or(Duration::ZERO, |timing| timing.delay)
    }

    fn set_rate(&self, ratio: f32) {
        let _ = self.stream.set_rate(ratio);
    }
}
