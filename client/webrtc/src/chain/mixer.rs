//! The mix of the received tracks: each a slot the engine's mixer reads, queued and ticked in step.

use super::shared::{State, lock};
use super::{FRAME, RATE_HZ, SAMPLES, Stereo, TRACK_QUEUE};
use futures_util::Stream;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::native::audio_mixer::AudioMixerSource;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// A received track's frame on its way into the mix, and how loud it is mixed.
#[derive(Default)]
pub(super) struct Slot {
    /// The frame of this tick, interleaved, and its channels; none mixes it silent.
    pub(super) frame: Option<(Vec<i16>, u32)>,
}

/// What the mixer reads a received track through.
pub(super) struct Source {
    pub(super) id: i32,
    pub(super) slot: Arc<Mutex<Slot>>,
    pub(super) gain: Arc<AtomicU32>,
    /// The loudest sample handed to the mixer since [`super::Heard::mixed`] last read it, after the gain.
    pub(super) mixed: Arc<AtomicU32>,
}

impl AudioMixerSource for Source {
    fn ssrc(&self) -> i32 {
        self.id
    }

    fn preferred_sample_rate(&self) -> u32 {
        RATE_HZ
    }

    fn get_audio_frame_with_info(&self, target_sample_rate: u32) -> Option<AudioFrame<'_>> {
        let (samples, channels) = lock(&self.slot).frame.take()?;
        // Every frame is the chain's rate; asked another, it is left out rather than handed over at the wrong one.
        if target_sample_rate != RATE_HZ {
            return None;
        }
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        #[expect(clippy::cast_possible_truncation, reason = "a gain from 0 to 1 keeps a sample within i16")]
        let data: Vec<i16> = samples.iter().map(|sample| (f32::from(*sample) * gain) as i16).collect();
        let loudest = data.iter().map(|sample| u32::from(sample.unsigned_abs())).max().unwrap_or(0);
        self.mixed.fetch_max(loudest, Ordering::Relaxed);
        Some(AudioFrame {
            data: Cow::Owned(data),
            sample_rate: RATE_HZ,
            num_channels: channels,
            samples_per_channel: SAMPLES,
        })
    }
}

/// The frames of a received track, from whatever makes them: a track's stream, or a test's own.
pub(super) type Frames = Pin<Box<dyn Stream<Item = AudioFrame<'static>> + Send>>;

/// A received track as the chain holds it.
pub(super) struct Track {
    pub(super) id: i32,
    pub(super) stream: Frames,
    pub(super) channels: u32,
    pub(super) waiting: VecDeque<Vec<i16>>,
    pub(super) slot: Arc<Mutex<Slot>>,
    /// The loudest sample since its meter last read, before its gain.
    pub(super) peak: Arc<AtomicU32>,
    pub(super) ended: bool,
}

/// Whether the frames waiting make a tick: every live track has one, or one has two, so a track that stalls costs
/// the others one frame and no more.
pub(super) fn ticks(waiting: &[usize]) -> bool {
    !waiting.is_empty() && (waiting.iter().all(|count| *count > 0) || waiting.iter().any(|count| *count > 1))
}

/// Mixes every tick the received tracks have into `mixed`; whether a microphone test plays in the mix's place.
pub(super) fn mix(state: &mut State, context: &mut Context<'_>, mixed: &mut Vec<Stereo>, received: &AtomicU64) -> bool {
    let State { tracks, mixer, testing, .. } = state;
    for track in tracks.iter_mut() {
        received.fetch_add(take(track, context), Ordering::Relaxed);
    }
    tracks.retain(|track| !track.ended || !track.waiting.is_empty());
    while ticks(&tracks.iter().map(|track| track.waiting.len()).collect::<Vec<_>>()) {
        for track in tracks.iter_mut() {
            lock(&track.slot).frame = track.waiting.pop_front().map(|frame| (frame, track.channels));
        }
        let Some(mixer) = mixer else { break };
        let mut frame: Stereo = [0; 2 * FRAME];
        let out = mixer.mix(2);
        if out.len() != frame.len() {
            continue;
        }
        frame.copy_from_slice(out);
        mixed.push(frame);
    }
    testing.holds > 0
}

/// Moves what a received track has into its queue, metered before its gain, and says how many frames it took; a
/// frame that is not 10 ms of its channels is left out, since the mixer takes only those.
fn take(track: &mut Track, context: &mut Context<'_>) -> u64 {
    let whole = FRAME * track.channels as usize;
    let mut taken = 0;
    loop {
        match track.stream.as_mut().poll_next(context) {
            Poll::Ready(Some(frame)) => {
                if frame.data.len() != whole {
                    continue;
                }
                let loudest = frame.data.iter().map(|sample| u32::from(sample.unsigned_abs())).max().unwrap_or(0);
                track.peak.fetch_max(loudest, Ordering::Relaxed);
                if track.waiting.len() == TRACK_QUEUE {
                    track.waiting.pop_front();
                }
                track.waiting.push_back(frame.data.into_owned());
                taken += 1;
            }
            Poll::Ready(None) => {
                track.ended = true;
                return taken;
            }
            Poll::Pending => return taken,
        }
    }
}
