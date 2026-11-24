//! How often a call's speaker runs out of sound while every core of the machine is busy: a measurement whose numbers
//! go in the commit that changes how the voice's threads are run. One member sends a tone through the real server;
//! another plays it on this device's speaker through the chain, on devices opened as a call opens them, and the
//! speaker's own count of the times it had nothing to play is read once with the machine quiet and once loaded.

use crate::common;

use common::{Server, hears_a_tone, join_voice, voice_of};
use proto::chatkord_v1::TrackKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::Thread;
use std::time::{Duration, Instant};
use tier::skipped;
use webrtc::DevicesRefused;
use webrtc::chain::{self, Listing, Stereo};

/// How long the speaker plays before its counts are read, quiet and then loaded.
const QUIET: Duration = Duration::from_secs(20);
const LOADED: Duration = Duration::from_secs(30);
/// Busy threads for each of the machine's threads, so nothing of ordinary standing gets a core to itself.
const BUSY_PER_CORE: usize = 2;
/// Quiet enough to sit beside, loud enough that the stream is measured as a tone.
const GAIN: f64 = 0.05;

/// How far past its delay the speaker is stepped, to time its catch-up: 50 ms, five frames of silence handed over at
/// once, past the 40 ms over its aim the speaker skips from.
const STEP_FRAMES: u32 = 5;
/// How close to its delay before the step the speaker must come back to be caught up, in milliseconds.
const BACK_WITHIN_MS: f64 = 10.0;
/// The speaker's delay is read this often, and a level is the mean of this many reads: a quantum's swing evened out.
const DELAY_EVERY: Duration = Duration::from_millis(50);
const DELAY_READS: usize = 6;
/// How long a catch-up is waited for before it is said not to have come.
const CATCH_UP_WAITS: Duration = Duration::from_secs(15);

/// What the speaker lost so far: frames it had no room for, and times it ran out and played silence; and the frames
/// of silence it is to be handed at once, to step its delay.
#[derive(Default)]
struct Losses {
    dropped: AtomicU64,
    ran_out: AtomicU64,
    step: AtomicU32,
}

impl Losses {
    fn read(&self) -> (u64, u64) {
        (self.dropped.load(Ordering::Relaxed), self.ran_out.load(Ordering::Relaxed))
    }
}

/// The system's devices as a call opens them, the speaker's losses told to the test.
struct Counted(Arc<Losses>);

struct Played(sound::voice::Speaker, Arc<Losses>);

impl chain::Speaker for Played {
    fn keep(&mut self, now: Instant) {
        self.0.keep(now);
        let (dropped, ran_out) = self.0.losses();
        self.1.dropped.store(dropped, Ordering::Relaxed);
        self.1.ran_out.store(ran_out, Ordering::Relaxed);
    }

    fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
        self.0.choose(device).map_err(|_| DevicesRefused::Refused)
    }

    fn is_open(&self) -> bool {
        self.0.health() != sound::voice::Health::Stopped
    }
}

struct Playing(sound::voice::Voiced, Arc<Losses>);

impl chain::SpeakerEnd for Playing {
    fn play(&mut self, frame: &Stereo) {
        for _ in 0..self.1.step.swap(0, Ordering::Relaxed) {
            self.0.play(&[0; 2 * sound::voice::FRAME]);
        }
        self.0.play(frame);
    }

    fn delay(&self) -> Duration {
        self.0.delay()
    }
}

impl chain::Sound for Counted {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> chain::MicrophoneOpened {
        Err(DevicesRefused::Unavailable)
    }

    fn speaker(&self, device: Option<&str>, now: Instant) -> chain::SpeakerOpened {
        let (speaker, voiced) = sound::voice::Speaker::open(device, now).map_err(|_| DevicesRefused::Refused)?;
        Ok((Box::new(Played(speaker, Arc::clone(&self.0))), Box::new(Playing(voiced, Arc::clone(&self.0)))))
    }

    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }

    fn ahead(&self) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(sound::voice::realtime()))
    }
}

/// Threads that only spin, until dropped.
struct Load {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Load {
    fn start() -> Load {
        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let threads = (0..cores * BUSY_PER_CORE)
            .map(|_| {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut spun = 0_u64;
                    while !stop.load(Ordering::Relaxed) {
                        spun = std::hint::black_box(spun.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
                    }
                })
            })
            .collect();
        Load { stop, threads }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

mod lab {
    use super::*;

    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "a measurement, run on request: a minute of a quiet tone on this device's speaker"]
        async fn the_speaker_s_run_outs_with_every_core_busy() {
            let (server, invites) = Server::start("e2e_voice_load", 2).await;
            let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
            let (ada_voice, _ada_hearing) = voice_of(&joined);
            let _speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

            let losses = Arc::new(Losses::default());
            let plain = webrtc::Processing {
                echo_cancellation: false,
                noise: webrtc::Noise::Off,
                gain: webrtc::Gain::Fixed(0.0),
                gate: None,
            };
            let engine = webrtc::Engine::start(Arc::new(Counted(Arc::clone(&losses))), plain);
            let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
            let (mut bo_voice, mut bo_hearing) = webrtc::Voice::new(
                &engine,
                &state,
                webrtc::Located::of(&state, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses"),
            )
            .expect("both connections");
            if let Err(why) = bo_voice.play() {
                return skipped(format!("no speaker to play on: {why:?}"));
            }
            let ada_id = state.streams[0].user_id.clone();
            bo_voice.set_volume(&ada_id, TrackKind::Microphone, GAIN);
            bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");
            hears_a_tone(&bo_voice).await;
            // The speaker primes and its rate settles before anything is counted.
            let mut looks = crate::common::looks::Looks::every(screens::LOOK_EVERY);
            looks.over(Duration::from_secs(3)).await;

            let devices = bo_voice.devices().expect("ours");
            let before = losses.read();
            looks.over(QUIET).await;
            let quiet = losses.read();
            let quiet_delay = devices.health();
            let load = Load::start();
            looks.over(LOADED).await;
            let loaded = losses.read();
            let loaded_delay = devices.health();
            drop(load);

            // A step in the speaker's delay, as a stall of the mix thread leaves it: how long until it is back.
            let mut watch = crate::common::looks::Looks::every(DELAY_EVERY);
            let mut reads = std::collections::VecDeque::with_capacity(DELAY_READS);
            let read = |reads: &mut std::collections::VecDeque<u32>| {
                if reads.len() == DELAY_READS {
                    reads.pop_front();
                }
                reads.push_back(devices.health().speaker_delay_ms);
                (reads.len() == DELAY_READS).then(|| f64::from(reads.iter().sum::<u32>()) / DELAY_READS as f64)
            };
            let baseline = watch.until(CATCH_UP_WAITS, || read(&mut reads)).await.expect("the delay is read");
            reads.clear();
            let stepped = Instant::now();
            losses.step.store(STEP_FRAMES, Ordering::Relaxed);
            let back = watch
                .until(CATCH_UP_WAITS, || read(&mut reads).filter(|level| *level <= baseline + BACK_WITHIN_MS))
                .await
                .map(|_| stepped.elapsed());

            let cores = std::thread::available_parallelism().map_or(1, usize::from);
            eprintln!(
                "quiet for {}s: ran out {} times, dropped {} frames",
                QUIET.as_secs(),
                quiet.1 - before.1,
                quiet.0 - before.0
            );
            eprintln!(
                "{} busy threads on {cores} for {}s: ran out {} times, dropped {} frames",
                cores * BUSY_PER_CORE,
                LOADED.as_secs(),
                loaded.1 - quiet.1,
                loaded.0 - quiet.0
            );
            eprintln!(
                "speaker's delay quiet: {} ms (most {} ms); loaded: {} ms (most {} ms)",
                quiet_delay.speaker_delay_ms,
                quiet_delay.speaker_delay_most_ms,
                loaded_delay.speaker_delay_ms,
                loaded_delay.speaker_delay_most_ms
            );
            eprintln!(
                "a step of {} ms from a delay of {baseline:.1} ms: back within {BACK_WITHIN_MS} ms after {back:?}",
                STEP_FRAMES * 10
            );
            drop((bo_voice, bo_hearing, server));
        }
    }
}
