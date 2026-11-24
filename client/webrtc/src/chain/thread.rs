//! The mix thread: its turns, each taking the microphone through its steps and the received tracks to the speaker.

use super::devices::{MicrophoneEnd, SpeakerEnd};
use super::handle::Ends;
use super::mixer::mix;
use super::report::Turn;
use super::shared::{Cadence, Shared, lock};
use super::{FRAME, RATE_HZ, RESTART_WITHIN, SAMPLES, Stereo, TRACK_QUEUE};
use crate::processing::{Processing, Steps};
use futures_util::FutureExt;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use std::borrow::Cow;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Wake, Waker};
use std::thread;
use std::thread::Thread;
use std::time::{Duration, Instant};

/// Wakes the chain's thread when a received track has a frame.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// What the mix thread alone holds: both ends, the microphone's steps and what a turn reuses.
pub(super) struct Local {
    /// How `steps` were last told to process, for steps made anew.
    processing: Processing,
    steps: Steps,
    microphone: Option<Box<dyn MicrophoneEnd>>,
    speaker: Option<Box<dyn SpeakerEnd>>,
    /// The playout delay the engine was last told.
    told_late: Option<u16>,
    /// The frames one turn mixed, played once the lock is let go.
    mixed: Vec<Stereo>,
}

impl Local {
    pub(super) fn new(processing: Processing) -> Self {
        Self {
            processing,
            steps: Steps::new(&processing),
            microphone: None,
            speaker: None,
            told_late: None,
            mixed: Vec::with_capacity(TRACK_QUEUE),
        }
    }

    /// How the mix thread sleeps before its next turn: parked while no device is asked, none of their ends is held
    /// and no track is mixed.
    fn cadence(&self, shared: &Shared) -> Cadence {
        let state = lock(&shared.state);
        let asked = state.microphone.holds > 0 || state.speaker.holds > 0 || !state.tracks.is_empty();
        if asked || self.microphone.is_some() || self.speaker.is_some() { Cadence::Turning } else { Cadence::Parked }
    }
}

pub(super) fn run(shared: &Shared, processing: Processing, factory: &PeerConnectionFactory, ends: &mut Ends) {
    let _ahead = shared.sound.ahead();
    let mut local = Local::new(processing);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut panicked: Option<Instant> = None;
    while !shared.stop.load(Ordering::Acquire) {
        shared.doing(Turn::Idle);
        local.cadence(shared).sleep();
        let now = Instant::now();
        // Sound to go on after: everything a turn can leave half done is in `local`, which is dropped and made
        // anew below; what is shared is counts, queues and what is asked, each whole at every step.
        let playout = |late| factory.set_playout_delay_ms(late);
        let done = panic::catch_unwind(AssertUnwindSafe(|| turn(shared, &playout, &mut local, ends, &mut context)));
        if done.is_ok() {
            shared.counts.turns.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let doing = Turn::of(shared.doing.load(Ordering::Relaxed));
        if panicked.is_some_and(|first| now.saturating_duration_since(first) < RESTART_WITHIN) {
            report::note!(
                error,
                "the voice chain panicked again within its restart bound; it is stopped",
                doing = doing,
                bound_ms = report::millis(RESTART_WITHIN),
            );
            return;
        }
        panicked = Some(now);
        shared.counts.restarts.fetch_add(1, Ordering::Relaxed);
        report::note!(
            error,
            "the voice chain panicked; its ends and steps are dropped and its devices opened again",
            doing = doing,
        );
        local = Local::new(local.processing);
        shared.epoch.fetch_add(1, Ordering::AcqRel);
    }
}

/// One turn: the ends the keepers made the ones in use taken up, the microphone's frames processed and sent, what
/// is received mixed and played.
pub(super) fn turn(
    shared: &Shared,
    playout: &dyn Fn(u16),
    local: &mut Local,
    ends: &mut Ends,
    context: &mut Context<'_>,
) {
    if let Some(asked) = lock(&shared.asked).take() {
        local.steps.set(&asked);
        local.processing = asked;
    }
    while let Ok(end) = ends.microphone.try_recv() {
        local.microphone = end;
    }
    while let Ok(end) = ends.speaker.try_recv() {
        local.speaker = end;
    }
    let Local { steps, microphone, speaker, told_late, mixed, .. } = local;
    let counts = &shared.counts;

    shared.doing(Turn::Processing);
    let echo = speaker.as_ref().map_or(Duration::ZERO, |speaker| speaker.delay());
    let heard_after = u32::try_from(echo.as_millis()).unwrap_or(u32::MAX);
    counts.speaker_delay_ms.store(heard_after, Ordering::Relaxed);
    counts.speaker_delay_most_ms.fetch_max(heard_after, Ordering::Relaxed);
    if let Some(microphone) = microphone {
        let delay = i32::try_from((echo + microphone.delay()).as_millis()).unwrap_or(i32::MAX);
        while let Some(mut frame) = microphone.frame() {
            let heard = steps.capture(&mut frame, delay);
            shared.level.fetch_max(i32::from(heard.peak), Ordering::AcqRel);
            let mut loudest = lock(&shared.loudness);
            *loudest = Some(loudest.map_or(heard.loudness, |was| was.max(heard.loudness)));
            drop(loudest);
            let sent = AudioFrame {
                data: Cow::Borrowed(&frame),
                sample_rate: RATE_HZ,
                num_channels: 1,
                samples_per_channel: SAMPLES,
            };
            // Without a queue the source hands the frame on as it is called, so the future is already done.
            let _ = shared.source.capture_frame(&sent).now_or_never();
            counts.captured.fetch_add(1, Ordering::Relaxed);
            let late = lock(&shared.state).testing.late(frame);
            if let Some(heard) = late {
                let mut played: Stereo = [0; 2 * FRAME];
                for (pair, sample) in played.as_chunks_mut::<2>().0.iter_mut().zip(heard) {
                    *pair = [sample; 2];
                }
                // The canceller hears what plays, so the microphone hearing it again is not sent twice.
                steps.reverse(&mut played);
                if let Some(speaker) = speaker.as_mut() {
                    speaker.play(&played);
                    counts.played.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    shared.doing(Turn::Mixing);
    mixed.clear();
    let testing = mix(&mut lock(&shared.state), context, mixed, &counts.received);
    if !testing {
        for frame in mixed.iter_mut() {
            steps.reverse(frame);
            if let Some(speaker) = speaker.as_mut() {
                speaker.play(frame);
                counts.played.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    let late = u16::try_from(echo.as_millis()).unwrap_or(u16::MAX);
    if *told_late != Some(late) {
        playout(late);
        *told_late = Some(late);
    }
}
