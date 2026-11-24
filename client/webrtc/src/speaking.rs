//! Speaking into a voice channel: the microphone through our chain, with its processing, or a generated tone where
//! there is no microphone to use.

use crate::devices::{Capture, Devices, DevicesRefused};
use crate::sending::Sending;
use crate::tone;
use crate::voice::VoiceRefused;
use libwebrtc::audio_source::AudioSourceOptions;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::audio_track::RtcAudioTrack;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use link::Link;
use proto::chatkord_v1::TrackKind;
use proto::mediasoup_types::rtp_parameters::{RtpCodecParameters, RtpParameters};
use tokio::task::JoinHandle;

/// A member who is sending audio. Dropping it stops the tone; [`Speaking::stop`] ends the track.
pub struct Speaking {
    sending: Sending,
    track: RtcAudioTrack,
    /// What generates the tone, and the source it goes into; absent for a microphone.
    feeding: Option<(JoinHandle<()>, NativeAudioSource)>,
    /// The microphone this track reads from; absent for a tone.
    microphone: Option<Microphone>,
}

/// A microphone track's device: the hold that opens it again, and while unmuted its guard. Muted, it is closed.
struct Microphone {
    devices: Devices,
    capture: Option<Capture>,
}

impl Drop for Speaking {
    fn drop(&mut self) {
        if let Some((feeding, _)) = &self.feeding {
            feeding.abort();
        }
    }
}

impl Speaking {
    /// Ends the track, at the server first; what else is sent over the connection carries on.
    pub async fn stop(self, link: &Link) -> Result<(), VoiceRefused> {
        self.sending.end(link, TrackKind::Microphone).await
    }

    /// Whether a microphone is being captured: false for a tone, which has none.
    pub fn is_capturing(&self) -> bool {
        self.microphone.as_ref().is_some_and(|microphone| microphone.devices.is_capturing())
    }

    /// How loud what is sent is now, from 0 to 100; none when there is no reading yet. A microphone's is read from
    /// its own samples, a tone's from the send connection's statistics.
    pub async fn level(&self) -> Option<u8> {
        self.level_ask().await
    }

    /// The same ask, on handles of its own: whoever awaits it holds nothing of this track. A microphone's reading
    /// is taken here, as it is asked; a tone's is the engine's report, bounded as every statistics call is.
    pub fn level_ask(&self) -> impl Future<Output = Option<u8>> + Send + use<> {
        let microphone = self.microphone.as_ref().map(|microphone| microphone.devices.chain().level());
        let sending = self.sending.clone();
        async move {
            match microphone {
                Some(level) => level,
                None => crate::stats::microphone_level(&sending.stats().await.ok()?),
            }
        }
    }

    /// Mutes: the track sends silence and stays in the call, so nobody is told the stream ended and nothing is
    /// renegotiated. A microphone's device is closed until [`Speaking::unmute`].
    pub fn mute(&mut self) {
        self.track.set_enabled(false);
        if let Some(microphone) = &mut self.microphone {
            microphone.capture = None;
        }
    }

    /// Unmutes, asking for a microphone's device again; whether it opened is in [`Devices::sounding`]. Refused,
    /// with the chain's thread gone, the track stays muted.
    pub fn unmute(&mut self) -> Result<(), DevicesRefused> {
        if let Some(microphone) = &mut self.microphone
            && microphone.capture.is_none()
        {
            microphone.capture = Some(microphone.devices.capture()?);
        }
        self.track.set_enabled(true);
        Ok(())
    }

    /// Stops or restarts the tone's frames while the track and its producer stay, as a sender whose loop stalled.
    #[cfg(any(test, feature = "testing"))]
    pub fn stall(&mut self, stalled: bool) {
        if let Some((feeding, source)) = &mut self.feeding {
            feeding.abort();
            if !stalled {
                *feeding = tokio::spawn(feed(source.clone()));
            }
        }
    }

    /// Sends a tone over `sending`.
    pub(crate) async fn tone(link: &Link, sending: &Sending) -> Result<Self, VoiceRefused> {
        let source = NativeAudioSource::new(AudioSourceOptions::default(), tone::RATE_HZ, 1, tone::QUEUE_MS);
        let track = sending.factory().create_audio_track("mic", source.clone());
        sending.start(link, TrackKind::Microphone, track.clone().into(), "tone", |_| {}, |_| {}).await?;
        let feeding = Some((tokio::spawn(feed(source.clone())), source));
        Ok(Self { sending: sending.clone(), track, feeding, microphone: None })
    }

    /// Sends the microphone over `sending`, captured by the chain `devices` holds, asked for before the track is
    /// made: the track sends silence until the microphone has opened.
    pub(crate) async fn microphone(link: &Link, sending: &Sending, devices: Devices) -> Result<Self, VoiceRefused> {
        let capture = devices.capture().map_err(VoiceRefused::Sound)?;
        let track = sending.factory().create_audio_track("mic", devices.chain().source());
        sending.start(link, TrackKind::Microphone, track.clone().into(), "voice", |_| {}, dtx).await?;
        Ok(Self {
            sending: sending.clone(),
            track,
            feeding: None,
            microphone: Some(Microphone { devices, capture: Some(capture) }),
        })
    }
}

/// Opus's discontinuous transmission for the microphone, as Signal sends it: a quiet stretch the encoder judges
/// silent goes out as a packet every 400 ms, about 5 kbit/s against 20. The engine reads our answer.
fn dtx(parameters: &mut RtpParameters) {
    for codec in &mut parameters.codecs {
        if let RtpCodecParameters::Audio { parameters, .. } = codec {
            parameters.insert("usedtx", 1_u8);
        }
    }
}

async fn feed(source: NativeAudioSource) {
    tone::feed(&source, (0..=u32::MAX).cycle()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_microphone_asks_for_opus_dtx_and_keeps_what_the_router_asked() {
        let mut parameters: RtpParameters = serde_json::from_str(
            r#"{"codecs": [{"mimeType": "audio/opus", "payloadType": 100, "clockRate": 48000, "channels": 2,
                "parameters": {"useinbandfec": 1}, "rtcpFeedback": []}], "headerExtensions": [], "encodings": [],
                "rtcp": {"reducedSize": true}}"#,
        )
        .expect("parameters");
        dtx(&mut parameters);
        let text = serde_json::to_value(&parameters).expect("serialised");
        let codec = &text["codecs"][0]["parameters"];
        assert_eq!((codec["usedtx"].as_u64(), codec["useinbandfec"].as_u64()), (Some(1), Some(1)));
    }
}
