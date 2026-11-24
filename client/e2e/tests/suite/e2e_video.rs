//! Video over the real wire, with the encoder outside the engine: a hardware-encoded H.264 stream enters libwebrtc
//! pre-encoded, crosses the real `chatkord-server` process, and a second engine's decoded picture is compared with it.

use crate::common;

use common::running::start;
use common::{Server, join_voice, registered_as, voice_of};
use media::{Codec, Nv12, Rate, Session, Settings, survey};
use net::{RateGate, Verdict};
use proto::video::Video;
use screens::Track;
use std::num::NonZeroU32;
use std::time::Duration;
use tier::skipped;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const FPS: u32 = 30;
const SETTINGS: Settings = Settings {
    width: WIDTH,
    height: HEIGHT,
    frames_per_second: FPS,
    bits_per_second: 4_000_000,
    key_frame_every: NonZeroU32::new(60),
};

/// A gradient with a block that moves, so a frame can be told from its neighbours.
fn picture(n: usize) -> Nv12 {
    let mut data = vec![0u8; WIDTH * HEIGHT * 3 / 2];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let in_block = (x + n * BLOCK_STEP) % WIDTH < 160 && (y / 160) % 2 == 0;
            data[y * WIDTH + x] = if in_block { 235 } else { ((x + y) / 8 % 200 + 16) as u8 };
        }
    }
    data[WIDTH * HEIGHT..].fill(128);
    Nv12::new(WIDTH, HEIGHT, data).expect("a valid frame")
}

/// Pixels the block moves a frame.
const BLOCK_STEP: usize = 8;
/// Frames until the picture repeats.
const PERIOD: usize = WIDTH / BLOCK_STEP;
/// Pictures are compared on every 4th pixel each way: enough to tell them apart, fast in a debug build.
const STEP: usize = 4;

fn thin(luma: &[u8], stride: usize) -> Vec<u8> {
    (0..HEIGHT).step_by(STEP).flat_map(|y| (0..WIDTH).step_by(STEP).map(move |x| luma[y * stride + x])).collect()
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let squared: f64 = a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum();
    let mse = squared / a.len() as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// A moving gradient at any size; only the size is judged below.
fn picture_at(width: usize, height: usize, n: usize) -> Nv12 {
    let mut data = vec![128u8; width * height * 3 / 2];
    for y in 0..height {
        for x in 0..width {
            data[y * width + x] = ((x + y + n * 8) / 8 % 200 + 16) as u8;
        }
    }
    Nv12::new(width, height, data).expect("a valid frame")
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_hardware_encoded_picture_crosses_the_real_server_into_another_engine() {
        let report = survey();
        let Some(encoder) = report
            .encoders
            .iter()
            .find(|e| e.codec() == Codec::H264 && matches!(e.path(), media::Path::Hardware(_)))
            .cloned()
        else {
            skipped("no hardware H.264 encoder opens on this machine");
            return;
        };
        let (server, invites) = Server::start("e2e_video", 2).await;

        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sender, source) =
            ada_voice.share(&ada, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");

        // The encoder is ours; the engine asks for key frames and rates and the loop answers.
        let mut session = Session::open(&encoder, SETTINGS).expect("the hardware encoder opens");
        let feeding = tokio::task::spawn_blocking(move || {
            let (mut sent, mut keys, mut rates) = (0usize, 0usize, 0usize);
            let mut applied: Vec<u32> = Vec::new();
            let started = std::time::Instant::now();
            let mut gate = RateGate::new(session.rate().bits_per_second, Duration::ZERO);
            let mut profile: Option<u8> = None;
            for n in 0..300usize {
                if source.key_frame_wanted() {
                    session.request_key_frame();
                    keys += 1;
                }
                if let Some(bits) = source.bits_wanted() {
                    rates += 1;
                    // Whether a target is worth a key frame is the rate gate's call, not this loop's.
                    let offered = Rate::new(bits, NonZeroU32::new(FPS).expect("a rate"))
                        .map(|rate| (gate.offer(started.elapsed(), rate.bits_per_second), rate));
                    if let Ok((Verdict::Apply, rate)) = offered {
                        session.set_rate(rate).expect("a rate Rate::new made is usable");
                        applied.push(rate.bits_per_second);
                    }
                }
                for unit in session.encode(&picture(n)).expect("encode") {
                    if profile.is_none() {
                        profile =
                            unit.data.windows(5).find(|w| w[..4] == [0, 0, 0, 1] && w[4] & 0x1f == 7).and_then(|_| {
                                unit.data.windows(6).find(|w| w[..4] == [0, 0, 0, 1] && w[4] & 0x1f == 7).map(|w| w[5])
                            });
                    }
                    source.push(&unit.data, unit.key_frame, WIDTH as u32, HEIGHT as u32, unit.captured);
                    sent += 1;
                }
                crate::clock::frame_clock(Duration::from_millis(1000 / u64::from(FPS)));
            }
            (sent, keys, rates, profile, applied)
        });

        let (bo_link, bo_events, booth, bo_id) = registered_as(&server.url(), "bo", &invites[1]).await;
        let bo = start(bo_link, bo_events, booth, bo_id);
        let ada_id =
            bo.when("Ada's screen is live", Duration::from_secs(60), || bo.sender_of(Track::ScreenVideo)).await;
        let slot = bo.picture(&ada_id, Video::Screen);
        // Pictures in memory, so each one's luma is read here.
        slot.refuse_imports();

        let candidates: Vec<Vec<u8>> = (0..PERIOD).map(|n| thin(picture(n).luma(), WIDTH)).collect();
        let mut best = 0f64;
        let mut seen = 0usize;
        bo.when("the decoded picture is the one that was encoded", Duration::from_secs(30), || {
            if let Some((size, luma, stride)) = common::luma_in(&slot) {
                assert_eq!(size, (WIDTH as u32, HEIGHT as u32), "the picture keeps its size");
                let got = thin(&luma, stride);
                let score = candidates.iter().map(|candidate| psnr(&got, candidate)).fold(0f64, f64::max);
                best = best.max(score);
                seen += 1;
            }
            (seen >= 30 && best > 30.0).then_some(())
        })
        .await;
        let (sent, keys, rates, profile, applied) = feeding.await.expect("the feeding finished");
        assert_eq!(profile, Some(100), "the stream is the High the session description promises");
        eprintln!(
            "encoder wrote H.264 profile_idc {profile:?} (66 baseline, 77 main, 100 high); sent {sent} units; the engine asked for {keys} key frames and gave {rates} rate targets (applied {applied:?} bit/s); bo saw {seen} pictures, best luma PSNR {best:.1} dB"
        );
        assert!(
            seen >= 30 && best > 30.0,
            "the decoded picture is the one that was encoded ({best:.1} dB over {seen} pictures)"
        );

        drop(sender);
        bo.leave().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_stream_that_changes_size_is_followed_by_the_source_the_server_and_the_receiving_engine() {
        const SMALL: (usize, usize) = (640, 360);
        let report = survey();
        let Some(encoder) = report
            .encoders
            .iter()
            .find(|e| e.codec() == Codec::H264 && matches!(e.path(), media::Path::Hardware(_)))
            .cloned()
        else {
            skipped("no hardware H.264 encoder opens on this machine");
            return;
        };
        let (server, invites) = Server::start("e2e_video", 2).await;

        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sender, source) =
            ada_voice.share(&ada, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");

        let mut session = Session::open(&encoder, SETTINGS).expect("the hardware encoder opens");
        let feeding = tokio::task::spawn_blocking(move || {
            let (mut width, mut height) = (WIDTH, HEIGHT);
            for n in 0..400usize {
                if n == 150 {
                    (width, height) = SMALL;
                    session.set_size(width, height).expect("a usable size");
                }
                if source.key_frame_wanted() {
                    session.request_key_frame();
                }
                for unit in session.encode(&picture_at(width, height, n)).expect("encode") {
                    source.push(&unit.data, unit.key_frame, width as u32, height as u32, unit.captured);
                }
                crate::clock::frame_clock(Duration::from_millis(1000 / u64::from(FPS)));
            }
        });

        let (bo_link, bo_events, booth, bo_id) = registered_as(&server.url(), "bo", &invites[1]).await;
        let bo = start(bo_link, bo_events, booth, bo_id);
        let ada_id =
            bo.when("Ada's screen is live", Duration::from_secs(60), || bo.sender_of(Track::ScreenVideo)).await;
        let slot = bo.picture(&ada_id, Video::Screen);
        slot.refuse_imports();

        let (mut large, mut small) = (0usize, 0usize);
        bo.when("the receiver followed the new size", Duration::from_secs(40), || {
            if let Some((size, _, _)) = common::luma_in(&slot) {
                if size == (WIDTH as u32, HEIGHT as u32) {
                    large += 1;
                } else if size == (SMALL.0 as u32, SMALL.1 as u32) {
                    small += 1;
                } else {
                    panic!("a picture of neither size: {size:?}");
                }
            }
            (large >= 10 && small >= 30).then_some(())
        })
        .await;
        feeding.abort();
        eprintln!(
            "bo saw {large} pictures at {WIDTH}x{HEIGHT} and {small} at {}x{} after the source changed size",
            SMALL.0, SMALL.1
        );

        drop(sender);
        bo.leave().await;
    }
}
