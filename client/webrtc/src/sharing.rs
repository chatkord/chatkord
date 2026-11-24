//! Sending a picture already encoded, a screen's or a camera's: the caller hands the units to a [`Feeder`] and answers
//! what the engine asks of it, so this crate never captures and never encodes. A screen's sound goes by a
//! [`SoundFeeder`].

use crate::bounds::Rates;
use crate::codec::Codec;
use crate::sending::Sending;
use crate::stats::{outbound_video, selected_pair};
use crate::voice::VoiceRefused;
use futures_util::FutureExt;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::AudioSourceOptions;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use libwebrtc::rtp_sender::{RtpSender, VideoEncoderBackend};
use libwebrtc::video_frame::{EncodedFrameType, EncodedVideoFrame};
use libwebrtc::video_source::VideoResolution;
use libwebrtc::video_source::native::NativeVideoSource;
use link::Link;
use proto::chatkord_v1::TrackKind;
use proto::mediasoup_types::rtp_parameters::{RtpCodecParameters, RtpParameters};
use proto::video::Video;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// A picture being sent. [`SentVideo::stop`] ends its track; what else is sent over the connection carries on.
pub struct SentVideo {
    sending: Sending,
    sender: RtpSender,
    video: Video,
}

/// What the engine says of the picture sent as `kind` over `sending`.
async fn send_stats(sending: &Sending, kind: TrackKind) -> Result<SendStats, VoiceRefused> {
    let all = sending.stats().await?;
    let mut stats = SendStats::default();
    if let Some(pair) = selected_pair(&all) {
        stats.estimate_bits_per_second = pair.candidate_pair.available_outgoing_bitrate;
        stats.round_trip_seconds = pair.candidate_pair.current_round_trip_time;
    }
    let mid = sending.mid_of(kind).await;
    if let Some(out) = mid.and_then(|mid| outbound_video(&all, &mid)) {
        stats.target_bits_per_second = out.outbound.target_bitrate;
        stats.bytes_sent = out.sent.bytes_sent;
        stats.retransmitted_bytes = out.outbound.retransmitted_bytes_sent;
        stats.key_frames_encoded = out.outbound.key_frames_encoded;
        stats.huge_frames = out.outbound.huge_frames_sent;
        stats.picture_losses = out.outbound.pli_count;
        stats.full_intra_requests = out.outbound.fir_count;
        stats.nacks = out.outbound.nack_count;
        stats.packets_sent = out.sent.packets_sent;
        stats.send_delay_seconds = out.outbound.total_packet_send_delay;
    }
    Ok(stats)
}

/// A screen being sent, with its sound once [`Sharing::sound`] starts it. [`Sharing::stop`] ends both.
pub struct Sharing {
    picture: SentVideo,
}

/// The way encoded units get to the engine. It is `Send`, so the thread that encodes holds it.
#[derive(Clone)]
pub struct Feeder {
    source: NativeVideoSource,
    codec: Codec,
    /// The first unit's capture, from which every unit's time is counted.
    epoch: Arc<OnceLock<Instant>>,
}

impl Feeder {
    /// Hands the engine one unit in the share's codec (Annex B for H.264, OBUs for AV1), of a picture `width` by
    /// `height` captured at `captured`, which the stream's clock follows. The engine takes every unit: one it drops
    /// later (its congestion window, a paused sender) holds back the units after it and asks for a key frame, read by
    /// [`Feeder::key_frame_wanted`].
    pub fn push(&self, unit: &[u8], key_frame: bool, width: u32, height: u32, captured: Instant) {
        // The binding answers true for every encoded unit; a drop is the pass-through encoder's, after it.
        let _ = self.source.capture_encoded_frame(&EncodedVideoFrame {
            codec: crate::codec::encoded(self.codec),
            payload: unit,
            timestamp_us: self.microseconds(captured),
            frame_type: if key_frame { EncodedFrameType::Key } else { EncodedFrameType::Delta },
            resolution: VideoResolution { width, height },
            frame_metadata: None,
        });
    }

    /// `captured` in microseconds from the first unit's capture, counted from one so it is never zero, which the
    /// binding reads as no time and replaces with the wall clock as the unit arrives.
    fn microseconds(&self, captured: Instant) -> i64 {
        let since = captured.saturating_duration_since(*self.epoch.get_or_init(|| captured));
        i64::try_from(since.as_micros()).map_or(i64::MAX, |micros| micros.saturating_add(1))
    }

    /// Whether the engine asked for a key frame since the last ask (a receiver lost the picture, or joined).
    pub fn key_frame_wanted(&self) -> bool {
        self.source.take_keyframe_request()
    }

    /// The bits a second the engine newly wants the encoder to average, if it said since the last ask. Its frame rate
    /// is not given: it is the rate pictures were pushed at, which a still screen's repeats make about two a second.
    pub fn bits_wanted(&self) -> Option<u64> {
        self.source.take_rate_control_request().map(|target| target.target_bitrate_bps)
    }
}

/// The way a share's sound gets to the engine, 10 ms at a time.
#[derive(Clone)]
pub struct SoundFeeder {
    source: NativeAudioSource,
}

impl SoundFeeder {
    /// Hands the engine 10 ms of interleaved samples. False when the engine refused them (a wrong length).
    pub async fn push(&self, samples: &[i16]) -> bool {
        let Some(frame) = self.frame(samples) else { return false };
        self.source.capture_frame(&frame).await.is_ok()
    }

    /// The same without waiting, for a thread that is not the runtime's: the source has no queue, so the frame is
    /// handed on as it is called. False when the engine refused it.
    pub fn push_now(&self, samples: &[i16]) -> bool {
        let Some(frame) = self.frame(samples) else { return false };
        self.source.capture_frame(&frame).now_or_never().is_none_or(|handed| handed.is_ok())
    }

    fn frame<'a>(&self, samples: &'a [i16]) -> Option<AudioFrame<'a>> {
        let num_channels = self.source.num_channels();
        let samples_per_channel = u32::try_from(samples.len() / num_channels as usize).ok()?;
        Some(AudioFrame {
            data: samples.into(),
            sample_rate: self.source.sample_rate(),
            num_channels,
            samples_per_channel,
        })
    }
}

/// What the engine says about the screen it is sending, at one moment. Counters only grow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SendStats {
    /// The engine's estimate of what the link can carry, in bits per second; zero before it has one.
    pub estimate_bits_per_second: f64,
    /// The selected route's latest round trip.
    pub round_trip_seconds: f64,
    /// What the encoder was last told to aim at, by the engine.
    pub target_bits_per_second: f64,
    /// The picture's payload, retransmissions included.
    pub bytes_sent: u64,
    /// The picture's payload sent again for a NACK.
    pub retransmitted_bytes: u64,
    /// Key frames sent.
    pub key_frames_encoded: u32,
    /// Frames the engine counts as huge: several times the average, which a key frame at 4K can be.
    pub huge_frames: u32,
    /// Key frames the receivers asked for by PLI.
    pub picture_losses: u32,
    /// Key frames the receivers asked for by FIR.
    pub full_intra_requests: u32,
    /// Packets the receivers asked for again by NACK.
    pub nacks: u32,
    /// The picture's packets sent, retransmissions included.
    pub packets_sent: u64,
    /// The time each of those packets waited in the engine's pacer before it left, summed, in seconds.
    pub send_delay_seconds: f64,
}

impl SentVideo {
    /// States the rates the picture may run between, in bits per second, once at its start: the estimate starts at
    /// `start` within `min` and `max`, and the encoder is never asked for more than `max`. Each picture's are its
    /// own, a camera's never taken from a share's. Without it the estimate starts at a few hundred kilobits under a
    /// cap far below a screen's need; a microphone's part of it is negligible.
    pub fn set_rates(&self, min: u64, start: u64, max: u64) -> Result<(), VoiceRefused> {
        self.rates(min, Some(start), max)
    }

    /// Moves the bounds of a running picture, as when its screen changed size, and leaves the engine's estimate where
    /// it is: a start stated again would begin the estimate anew.
    pub fn bound_rates(&self, min: u64, max: u64) -> Result<(), VoiceRefused> {
        self.rates(min, None, max)
    }

    fn rates(&self, min: u64, start: Option<u64>, max: u64) -> Result<(), VoiceRefused> {
        self.sending.bound(self.video.kind(), Rates { min, max }, start)?;
        let mut parameters = self.sender.parameters();
        for encoding in &mut parameters.encodings {
            encoding.max_bitrate = Some(max);
        }
        self.sender.set_parameters(parameters).map_err(|_| VoiceRefused::Engine)
    }

    /// Asks the engine how the picture is going.
    pub async fn stats(&self) -> Result<SendStats, VoiceRefused> {
        self.stats_ask().await
    }

    /// The same ask, on handles of its own: whoever awaits it holds nothing of this picture, so a loop that has other
    /// work never waits on the engine's answer.
    pub fn stats_ask(&self) -> impl Future<Output = Result<SendStats, VoiceRefused>> + Send + use<> {
        let (sending, kind) = (self.sending.clone(), self.video.kind());
        async move { send_stats(&sending, kind).await }
    }

    /// Ends the picture, at the server first; what else is sent over the connection carries on.
    pub async fn stop(self, link: &Link) -> Result<(), VoiceRefused> {
        self.sending.end(link, self.video.kind()).await
    }

    /// Starts sending `video` of `width` by `height` in `codec` over `sending`. The first unit pushed to the
    /// [`Feeder`] should be a key frame.
    pub(crate) async fn start(
        link: &Link,
        sending: &Sending,
        video: Video,
        codec: Codec,
        width: u32,
        height: u32,
    ) -> Result<(Self, Feeder), VoiceRefused> {
        let source = NativeVideoSource::new_encoded(VideoResolution { width, height });
        let track = sending.factory().create_video_track(crate::video::label(video), source.clone());
        let sender = sending
            .start(
                link,
                video.kind(),
                track.into(),
                crate::video::label(video),
                |sender| sender.set_video_encoder_backend(VideoEncoderBackend::PreEncoded),
                |parameters| codec.alone_in(parameters),
            )
            .await?;
        Ok((Self { sending: sending.clone(), sender, video }, Feeder { source, codec, epoch: Arc::default() }))
    }
}

impl Sharing {
    /// States the share's picture's rates once at its start ([`SentVideo::set_rates`]).
    pub fn set_rates(&self, min: u64, start: u64, max: u64) -> Result<(), VoiceRefused> {
        self.picture.set_rates(min, start, max)
    }

    /// Moves the bounds of the running share's picture ([`SentVideo::bound_rates`]).
    pub fn bound_rates(&self, min: u64, max: u64) -> Result<(), VoiceRefused> {
        self.picture.bound_rates(min, max)
    }

    /// Asks the engine how the share's picture is going.
    pub async fn stats(&self) -> Result<SendStats, VoiceRefused> {
        self.picture.stats().await
    }

    /// The same ask, on handles of its own ([`SentVideo::stats_ask`]).
    pub fn stats_ask(&self) -> impl Future<Output = Result<SendStats, VoiceRefused>> + Send + use<> {
        self.picture.stats_ask()
    }

    /// Sends the share's sound beside its picture, `channels` interleaved at `rate`, in the picture's stream, so a
    /// receiver's engine may time the sound by it; the picture a receiver shows is not held to the sound. It is an
    /// application's sound, not a voice: the engine's echo cancellation,
    /// noise suppression and gain control stay off, and it is sent in stereo.
    pub async fn sound(&self, link: &Link, rate: u32, channels: u32) -> Result<SoundFeeder, VoiceRefused> {
        // No queue: each 10 ms goes to the encoder as it is pushed, so the sound waits nowhere but in the ring.
        let source = NativeAudioSource::new(AudioSourceOptions::default(), rate, channels, 0);
        let sending = &self.picture.sending;
        let track = sending.factory().create_audio_track("share-sound", source.clone());
        sending
            .start(link, TrackKind::ScreenAudio, track.into(), crate::video::label(Video::Screen), |_| {}, stereo)
            .await?;
        Ok(SoundFeeder { source })
    }

    /// Ends the sound and the picture, at the server first; what else is sent over the connection carries on.
    pub async fn stop(self, link: &Link) -> Result<(), VoiceRefused> {
        let sound = self.picture.sending.end(link, TrackKind::ScreenAudio).await;
        self.picture.stop(link).await.and(sound)
    }

    /// Starts sending a screen of `width` by `height` in `codec` over `sending`, its sound not yet.
    pub(crate) async fn start(
        link: &Link,
        sending: &Sending,
        codec: Codec,
        width: u32,
        height: u32,
    ) -> Result<(Self, Feeder), VoiceRefused> {
        let (picture, feeder) = SentVideo::start(link, sending, Video::Screen, codec, width, height).await?;
        Ok((Self { picture }, feeder))
    }
}

/// Opus in stereo both ways: `stereo` has the engine encode two channels (it reads our answer), and `sprop-stereo`
/// travels with the producer to every consumer's offer, so each receiver decodes two. `usedtx` lets the encoder send
/// a stretch of exact zeros as a packet every 400 ms: every chunk is sent, so silence costs little and bursts after it
/// are not lost.
pub(crate) fn stereo(parameters: &mut RtpParameters) {
    for codec in &mut parameters.codecs {
        if let RtpCodecParameters::Audio { parameters, .. } = codec {
            parameters.insert("stereo", 1_u8).insert("sprop-stereo", 1_u8).insert("usedtx", 1_u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shares_sound_is_stereo_with_dtx() {
        let mut parameters: RtpParameters = serde_json::from_str(
            r#"{"codecs": [{"mimeType": "audio/opus", "payloadType": 100, "clockRate": 48000, "channels": 2,
                "parameters": {"useinbandfec": 1}, "rtcpFeedback": []}], "headerExtensions": [], "encodings": [],
                "rtcp": {"reducedSize": true}}"#,
        )
        .expect("parameters");
        stereo(&mut parameters);
        let text = serde_json::to_value(&parameters).expect("serialised");
        let codec = &text["codecs"][0]["parameters"];
        assert_eq!((codec["stereo"].as_u64(), codec["sprop-stereo"].as_u64()), (Some(1), Some(1)));
        assert_eq!(codec["usedtx"].as_u64(), Some(1), "a stretch of zeros costs a packet every 400 ms");
        assert_eq!(codec["useinbandfec"].as_u64(), Some(1), "what the router asked for is kept");
    }
}
