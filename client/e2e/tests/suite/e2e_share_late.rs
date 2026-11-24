//! A busy share its encoder cannot make at the rate asked does not fall behind its sound. A hardware encoder at its
//! coarsest quantiser makes more bits for a busy picture than a low rate gives it (AMF at 1080p60 asked 2.4 Mbit/s
//! made 5.5), and with libwebrtc's frame dropper off the surplus waited in the pacer: a packet's wait grew from 287 to
//! 1317 ms and the picture reached the viewer 1.6 s late and growing, over 5 % uplink loss. The share owns its rate
//! before the encoder now (`net::Budget`): a picture waits while more bits than the rate allows are owed, and a
//! scene the rate cannot hold steps the share down.

use crate::common::{Server, join_voice, voice_of};
use crate::e2e_share_keeps_up::painted;
use crate::screen_support;

use call::screen::{Fail, Frames, Progress, begin_into};
use capture::CaptureError;
use choices::{Ceiling, Kbits, ShareQuality};
use media::Session;
use proto::chatkord_v1::TrackKind;
use screen_support::{candidates_of, layout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use testserver::{Loss, Way};
use tier::skipped;

/// How long the share runs over the loss: long enough for a queue that grows to show it.
const FOR: Duration = Duration::from_secs(40);

/// The longest the viewer may wait between two pictures once the share has settled.
const LONGEST_HOLD: Duration = Duration::from_millis(1_500);

/// Pictures of the busy screen: blocks of 32 pixels in colours none repeats, which no encoder makes small.
const PICTURES: u64 = 16;

/// The `n`th picture of the busy screen, `width` by `height`.
fn noise(n: u64, width: usize, height: usize) -> Vec<[u8; 3]> {
    let mut seed = n.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let across = width.div_ceil(32);
    let blocks: Vec<[u8; 3]> = (0..across * height.div_ceil(32))
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let [red, green, blue, ..] = seed.to_le_bytes();
            [red, green, blue]
        })
        .collect();
    (0..width * height).map(|i| blocks[(i / width / 32) * across + (i % width) / 32]).collect()
}

mod lab {
    use super::*;

    /// The probe's case: 1080p60 of a busy screen asked 2.5 Mbit/s, over 5 % loss on the way up. A fault is a second
    /// whose packets waited to leave the sender longer than the share holds itself to (the encoder's own key frame
    /// budget of its rate), a wait that keeps growing, or a viewer that waits more than [`LONGEST_HOLD`] for a picture.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_busy_share_over_loss_keeps_its_send_delay_and_the_viewers_hold_bounded() {
        let hardware = candidates_of(media::Codec::H264).into_iter().find(|(e, _)| e.path() != media::Path::Software);
        let Some((encoder, canvas)) = hardware else {
            return skipped("no hardware H.264 encoder converts a screen here");
        };
        let surveyed = screen_support::kept(media::Report { encoders: vec![encoder], unavailable: vec![] });
        let ask = Kbits::new(2_500).expect("a step in the bounds");
        let quality = ShareQuality { ceiling: Ceiling::Custom(ask), ..ShareQuality::default() };
        let (width, height) = (1920, 1080);
        let started = tokio::task::spawn_blocking(move || {
            let pictures = || (0..PICTURES).map(|n| noise(n, width, height));
            let screen = |session: &Session| match painted(&canvas, session, width, height, pictures()) {
                Some(screen) => Ok(Box::new(screen) as Box<dyn Frames>),
                None => Err(Fail::Capture(CaptureError::Format)),
            };
            match begin_into(layout(width, height), &quality, &surveyed, screen) {
                Ok(started) => Some(started),
                Err(Fail::Capture(CaptureError::Format)) => None,
                Err(Fail::Encode(why)) => panic!("the share begins: {why}"),
                Err(_) => panic!("the share begins"),
            }
        })
        .await
        .expect("the share opens");
        let Some(started) = started else { return skipped("the device makes no 1080p BGRx surface") };

        let (server, relay, invites) = Server::start_relayed("e2e_share_late", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) = ada_voice
            .share(&ada, webrtc::Codec::H264, started.layout.width, started.layout.height)
            .await
            .expect("sharing starts");
        let envelope = started.envelope;
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let progress = Arc::new(Progress::default());
        let stop = Arc::new(AtomicBool::new(false));
        let running = {
            let (progress, stop) = (Arc::clone(&progress), Arc::clone(&stop));
            tokio::task::spawn_blocking(move || {
                started.run(Some(&feeder), None, &progress, || stop.load(Ordering::Relaxed), || true)
            })
        };

        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
        let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
        let (_bo_voice, mut bo_hearing) = voice_of(&state);
        let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let record = Arc::clone(&arrivals);
        let _taken = bo_hearing
            .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, move |_| {
                if let Ok(mut arrivals) = record.lock() {
                    arrivals.push(Instant::now());
                }
            })
            .await
            .expect("the screen is taken");
        relay.lose(Loss { way: Way::Up, per_thousand: 50, seed: 0x230 });

        let began = Instant::now();
        let mut ticks = tokio::time::interval(Duration::from_secs(1));
        let mut last = webrtc::SendStats::default();
        // Each second's mean wait of a packet to leave the sender.
        let mut delays: Vec<Option<Duration>> = Vec::new();
        while began.elapsed() < FOR {
            ticks.tick().await;
            let now = sharing.stats().await.expect("the share's stats");
            let packets = now.packets_sent.saturating_sub(last.packets_sent);
            let waited = (now.send_delay_seconds - last.send_delay_seconds).max(0.0);
            let delay = (packets > 0).then(|| Duration::from_secs_f64(waited / packets as f64));
            let counted = progress.stages.counted();
            eprintln!(
                "second {}: {packets} packets each waiting {} ms to leave; {} pictures held by the budget; {:?}",
                delays.len(),
                delay.map_or_else(|| "-".to_string(), |delay| format!("{:.0}", delay.as_secs_f64() * 1000.0)),
                counted.held,
                progress.reduced.lock().map(|reduced| *reduced).unwrap_or_default()
            );
            delays.push(delay);
            last = now;
        }
        stop.store(true, Ordering::Relaxed);
        let report = running.await.expect("the share ran").expect("it runs");
        eprintln!("{}", report.line());
        let counted = progress.stages.counted();
        let reduced = progress.reduced.lock().map(|reduced| *reduced).unwrap_or_default();
        assert!(
            counted.held > 0 || reduced.is_some(),
            "nothing was held or stepped down: the screen did not need more than its rate, so nothing was tested"
        );

        let mean = |part: &[Option<Duration>]| {
            let delays: Vec<Duration> = part.iter().filter_map(|delay| *delay).collect();
            delays.iter().sum::<Duration>().checked_div(u32::try_from(delays.len()).unwrap_or(u32::MAX))
        };
        let third = delays.len() / 3;
        let (first, middle, end) =
            (mean(&delays[..third]), mean(&delays[third..2 * third]), mean(&delays[2 * third..]));
        eprintln!("the send delay by thirds: {first:?}, {middle:?}, {end:?}");
        let held_to = Duration::from_millis(media::KEY_FRAME_BUDGET_MS.unsigned_abs());
        let worst = delays.iter().flatten().max().copied();
        assert!(worst.is_none_or(|worst| worst <= held_to), "a second's send delay, {worst:?}, is past {held_to:?}");
        let growing = matches!((first, middle, end), (Some(first), Some(middle), Some(end))
            if first < middle && middle < end && end > first * 2 && end > Duration::from_millis(20));
        assert!(!growing, "the send delay keeps growing: {first:?}, then {middle:?}, then {end:?}");

        // The viewer's hold: the longest wait between two pictures once the share has had a third of the run to settle.
        let arrivals = arrivals.lock().expect("the arrivals").clone();
        let settled: Vec<Instant> = arrivals.into_iter().filter(|at| at.duration_since(began) >= FOR / 3).collect();
        assert!(settled.len() > 10, "the viewer took {} pictures after the share settled", settled.len());
        let longest = settled.windows(2).map(|pair| pair[1].duration_since(pair[0])).max().unwrap_or_default();
        eprintln!("the viewer took {} pictures, the longest wait for one {longest:?}", settled.len());
        assert!(longest <= LONGEST_HOLD, "the viewer waited {longest:?} for a picture");
        drop(sharing);
        drop(server);
    }
}
