//! The engine on its own, with no server: two peer connections in one process negotiate, find a path, carry audio
//! and decode it.

use crate::local::Local;
use crate::tone;
use futures_util::StreamExt;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::AudioSourceOptions;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::audio_track::RtcAudioTrack;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const WAIT: Duration = Duration::from_secs(15);

fn loudest(frame: &AudioFrame<'_>) -> i16 {
    frame.data.iter().map(|sample| sample.saturating_abs()).max().unwrap_or(0)
}

/// Two connections joined, a tone fed into one and its track as the other was handed it: on an engine's factory,
/// another member of a call whose sound the engine plays.
pub(crate) struct Toned {
    local: Local,
    pub(crate) track: RtcAudioTrack,
    feeding: JoinHandle<()>,
}

impl Toned {
    pub(crate) async fn start(factory: &PeerConnectionFactory) -> Self {
        let local = Local::new(factory).expect("two connections");

        let heard: Arc<Mutex<Option<MediaStreamTrack>>> = Arc::default();
        let slot = Arc::clone(&heard);
        local.answerer.on_track(Some(Box::new(move |event| {
            if let Ok(mut held) = slot.lock() {
                *held = Some(event.track);
            }
        })));

        let source = NativeAudioSource::new(AudioSourceOptions::default(), tone::RATE_HZ, 1, tone::QUEUE_MS);
        let track = factory.create_audio_track("tone", source.clone());
        local.offerer.add_track(track.into(), &["stream"]).expect("track added");

        let feeding = tokio::spawn(async move { tone::feed(&source, 0..3_000).await });

        local.join(WAIT).await.expect("the two ends connected in time");

        let track = timeout(WAIT, async {
            loop {
                if let Some(MediaStreamTrack::Audio(track)) = heard.lock().ok().and_then(|mut held| held.take()) {
                    break track;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the receiver was handed an audio track");
        Self { local, track, feeding }
    }

    /// The loudest sample of the next frames the track is metered by, up to `enough`.
    async fn metered(&self, enough: i16) -> i16 {
        let mut stream =
            NativeAudioStream::new(self.track.clone(), i32::try_from(tone::RATE_HZ).expect("48 kHz fits"), 1);
        timeout(WAIT, async {
            let mut peak = 0;
            for _ in 0..200 {
                let Some(frame) = stream.next().await else { break };
                peak = peak.max(loudest(&frame));
                if peak > enough {
                    break;
                }
            }
            peak
        })
        .await
        .expect("audio arrived in time")
    }

    pub(crate) fn stop(self) {
        self.feeding.abort();
        self.local.close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tone_goes_from_one_peer_connection_to_another_with_no_server() {
    let factory = PeerConnectionFactory::default();
    let toned = Toned::start(&factory).await;
    let peak = toned.metered(1_000).await;
    assert!(peak > 1_000, "the tone came through audible, loudest sample {peak}");
    toned.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_after_the_last_one_ended_still_hears() {
    let factory = PeerConnectionFactory::default();
    Toned::start(&factory).await.stop();
    let toned = Toned::start(&factory).await;
    let peak = toned.metered(1_000).await;
    assert!(peak > 1_000, "the second call heard the tone, loudest sample {peak}");
    toned.stop();
}
