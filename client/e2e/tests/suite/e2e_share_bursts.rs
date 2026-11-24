//! A share's sound as the sharer's capture hands it over: a burst after half a second of exact silence must reach the
//! other member's mix, because every 10 ms goes to the engine and the silence is zeros, not a gap. The measurement
//! reads the receiver's mix every few milliseconds, as `e2e_voice_trip` reads a voice's.

use crate::common;

use common::looks::Looks;
use common::{Server, join_voice, voice_of};
use proto::chatkord_v1::TrackKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const RATE_HZ: u32 = 48_000;
/// One chunk of a share's sound: 10 ms of both channels.
const CHUNK: usize = RATE_HZ as usize / 100 * 2;
const CHUNK_EVERY: Duration = Duration::from_millis(10);
const EVERY_CHUNKS: usize = 50;
const BURST_CHUNKS: usize = 3;
const BURSTS: usize = 20;
const SETTLE: Duration = Duration::from_secs(3);
/// How often the receiver's mix is read for a burst: finer than a burst and the quiet between two.
const MIX_LOOKS: Duration = Duration::from_millis(5);
/// The mixed peak that is a burst and not the quiet between bursts.
const LOUD: u32 = 2_000;
/// How long a stretch of zeros is sent before what it costs is read, and for how long it is read.
const QUIET_SETTLES: Duration = Duration::from_secs(2);
const QUIET_READ: Duration = Duration::from_secs(5);
/// The most a stretch of zeros may cost with the encoder's DTX on, in bits a second on the wire: without it Opus
/// sends a packet each 20 ms of digital silence, some 19 kbit/s.
const QUIET_MOST: f64 = 5_000.0;

/// Hands `feeder` a stereo tone for `BURST_CHUNKS` chunks in each `EVERY_CHUNKS` and zeros between, a chunk each 10 ms
/// as a share's feed thread does, until `stop`; zeros alone while `quiet`.
fn feed_bursts(
    feeder: webrtc::SoundFeeder,
    stop: Arc<AtomicBool>,
    quiet: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let begun = Instant::now();
        let mut n = 0_u32;
        while !stop.load(Ordering::Relaxed) {
            let at = n as usize % EVERY_CHUNKS;
            let mut chunk = [0_i16; CHUNK];
            if at < BURST_CHUNKS && !quiet.load(Ordering::Relaxed) {
                for (i, pair) in chunk.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    let wave = (std::f64::consts::TAU * 440.0 * (i as f64 + (n as usize * CHUNK / 2) as f64)
                        / f64::from(RATE_HZ))
                    .sin();
                    *pair = [(8_000.0 * wave) as i16; 2];
                }
            }
            feeder.push_now(&chunk);
            n += 1;
            let next = begun + CHUNK_EVERY * n;
            crate::clock::frame_clock(next.saturating_duration_since(Instant::now()));
        }
    })
}

mod lab {
    use super::*;

    // Hearing nine bursts in ten is this machine keeping up in real time: the test runs with nothing beside it.
    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn bursts_of_a_shares_sound_after_half_a_second_of_silence_reach_the_other_members_mix() {
            let (server, invites) = Server::start("e2e_share_bursts", 2).await;
            let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
            let (ada_voice, _ada_hearing) = voice_of(&joined);
            let (sharing, _picture) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
            let feeder = sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts");
            let (stop, quiet) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
            let feeding = feed_bursts(feeder, Arc::clone(&stop), Arc::clone(&quiet));

            let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
            let ada_id = state
                .streams
                .iter()
                .find(|stream| stream.kind == i32::from(TrackKind::ScreenAudio))
                .expect("ada's sound is live")
                .user_id
                .clone();
            let (_bo_voice, mut hearing) = voice_of(&state);
            hearing.take(&bo, &ada_id, TrackKind::ScreenAudio).await.expect("bo takes ada's sound");

            let mut looks = Looks::every(MIX_LOOKS);
            looks.over(SETTLE).await;
            let (began, window) = (Instant::now(), CHUNK_EVERY * u32::try_from(EVERY_CHUNKS * BURSTS).unwrap());
            // A burst already sounding when the window opens began before it: only a start inside it counts, and
            // the window holds one each half second.
            hearing.mixed(&ada_id, TrackKind::ScreenAudio);
            looks.next().await;
            let mut loud = hearing.mixed(&ada_id, TrackKind::ScreenAudio).unwrap_or(0) >= LOUD;
            let (mut heard, mut quiet_since) = (0_usize, began.checked_sub(Duration::from_secs(1)).unwrap_or(began));
            while began.elapsed() < window {
                looks.next().await;
                let peak = hearing.mixed(&ada_id, TrackKind::ScreenAudio).unwrap_or(0);
                if peak >= LOUD {
                    if !loud && quiet_since.elapsed() >= Duration::from_millis(200) {
                        heard += 1;
                    }
                    loud = true;
                } else if loud {
                    loud = false;
                    quiet_since = Instant::now();
                }
            }
            quiet.store(true, Ordering::Relaxed);
            looks.over(QUIET_SETTLES).await;
            let before = ada_voice.connection().await.expect("the engine's report");
            looks.over(QUIET_READ).await;
            let after = ada_voice.connection().await.expect("the engine's report");
            stop.store(true, Ordering::Relaxed);
            let _ = feeding.join();
            #[expect(clippy::cast_precision_loss, reason = "a few kilobytes")]
            let rate = |bytes: u64| bytes as f64 * 8.0 / QUIET_READ.as_secs_f64();
            let (payload, floor) = (rate(after.sent_bytes - before.sent_bytes), rate(after.up_bytes - before.up_bytes));
            eprintln!("{heard} of {BURSTS} bursts of the share's sound reached bo's mix");
            eprintln!("a stretch of zeros cost {floor:.0} bit/s sent, {payload:.0} of it payload");
            assert!(heard * 10 >= BURSTS * 9, "nine bursts in ten are heard: {heard} of {BURSTS}");
            assert!(floor <= QUIET_MOST, "the encoder rests on zeros: {floor:.0} bit/s sent");
        }
    }
}
