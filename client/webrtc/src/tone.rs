//! Generated sound: the frames a [`NativeAudioSource`](libwebrtc::audio_source::native::NativeAudioSource) is fed, the
//! test tone a call or a probe sends.

use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::native::NativeAudioSource;

pub(crate) const RATE_HZ: u32 = 48_000;
const FRAME_MS: u32 = 10;
const SAMPLES: u32 = RATE_HZ / 1000 * FRAME_MS;
/// Milliseconds the source buffers before it pushes back on the feeder.
pub(crate) const QUEUE_MS: u32 = 1000;
/// The test tone: its pitch, and its loudest sample.
const HZ: f32 = 440.0;
const PEAK: f32 = 8_000.0;

/// Feeds `source`, made with [`QUEUE_MS`], the test tone's `frames` until they run out or the source refuses one,
/// paced by the source's push back so its queue stays full: it sends 10 ms of silence for each tick it is short,
/// which a feeder that sleeps a frame between frames was about one tick in ten.
pub(crate) async fn feed(source: &NativeAudioSource, frames: impl IntoIterator<Item = u32>) {
    for index in frames {
        if source.capture_frame(&frame(HZ, PEAK, 1.0, index)).await.is_err() {
            return;
        }
    }
}

/// The frame `index` of a sine at `hz` whose loudest sample is `peak * gain` (`gain` from 0 to 1).
pub(crate) fn frame(hz: f32, peak: f32, gain: f32, index: u32) -> AudioFrame<'static> {
    let first = u64::from(index) * u64::from(SAMPLES);
    #[expect(clippy::cast_possible_truncation, reason = "a float cast saturates: a peak past i16 clips")]
    let data = (0..u64::from(SAMPLES))
        .map(|n| {
            let t = (first + n) as f64 / f64::from(RATE_HZ);
            (f64::from(peak * gain) * (2.0 * std::f64::consts::PI * f64::from(hz) * t).sin()) as i16
        })
        .collect();
    AudioFrame { data, sample_rate: RATE_HZ, num_channels: 1, samples_per_channel: SAMPLES }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_ten_milliseconds_and_never_louder_than_its_peak() {
        let loud = frame(440.0, 8_000.0, 1.0, 7);
        assert_eq!(loud.samples_per_channel, 480);
        assert_eq!(loud.data.len(), 480);
        assert!(loud.data.iter().all(|sample| sample.abs() <= 8_000));
        assert!(loud.data.iter().any(|sample| sample.abs() > 7_000), "a whole period of 440 Hz reaches its peak");
        assert!(frame(440.0, 8_000.0, 0.0, 7).data.iter().all(|sample| *sample == 0), "no gain is silence");
    }
}
