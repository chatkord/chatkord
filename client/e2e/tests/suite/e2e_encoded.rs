//! A screen taken as the sender encoded it, in each codec, decoded by `media` alone: the engine decodes no H.264 or
//! AV1 in this process, so the colours painted coming out prove the host's decode. A key frame asked for comes.

use crate::common;
use crate::screen_support;

use capture::Pixels;
use common::{Server, join_voice, voice_of};
use media::{Decoder, Session, Sources};
use proto::chatkord_v1::TrackKind;
use screen_support::{
    FPS, HEIGHT, WIDTH, candidates_of, decoded_is_the_painting, feed, layout, obu_types, quadrants, settings,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tier::skipped;
use tokio::time::timeout;

struct Frame {
    data: Vec<u8>,
    render_time_us: i64,
    key_frame: bool,
}

/// The NAL unit types in an Annex B access unit.
fn nal_types(data: &[u8]) -> Vec<u8> {
    data.windows(4).enumerate().filter(|(_, w)| w[..3] == [0, 0, 1]).map(|(_, w)| w[3] & 0x1f).collect()
}

/// Whether a key frame carries what a decoder starts from: H.264's parameter sets and an IDR, AV1's sequence header.
fn starts_a_decoder(codec: media::Codec, data: &[u8]) -> bool {
    match codec {
        media::Codec::H264 => {
            let types = nal_types(data);
            types.contains(&7) && types.contains(&8) && types.contains(&5)
        }
        media::Codec::Av1 => obu_types(data).contains(&1),
    }
}

/// The shared screen `record_share_fixture` recorded: each unit and whether it is a key frame.
fn fixture() -> Vec<(Vec<u8>, bool)> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/share-h264-320x180.txt"))
        .expect("the recorded screen");
    text.lines()
        .map(|line| {
            let (kind, hex) = line.split_once(' ').expect("a kind and a unit");
            let digits: Vec<u8> = hex.bytes().map(|digit| (digit as char).to_digit(16).expect("hex") as u8).collect();
            let data = digits.chunks(2).map(|pair| pair[0] << 4 | pair[1]).collect();
            (data, kind == "key")
        })
        .collect()
}

/// The fixture's unit after `index`: past the last it goes on from the first that is not a key frame.
fn after(index: usize, units: usize) -> usize {
    if index + 1 == units { 1 } else { index + 1 }
}

/// The last NAL unit of an Annex B access unit: the picture's slice, which no packetizer rewrites.
fn last_nal(data: &[u8]) -> &[u8] {
    let start = data.windows(3).rposition(|w| w == [0, 0, 1]).map_or(0, |at| at + 3);
    &data[start..]
}

mod server {
    use super::*;
    use std::collections::HashMap;
    use testserver::Schedule;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_over_a_lossy_path_is_handed_over_whole_and_in_order() {
        let units = fixture();
        let index: HashMap<Vec<u8>, usize> =
            units.iter().enumerate().map(|(at, (data, _))| (last_nal(data).to_vec(), at)).collect();
        assert_eq!(index.len(), units.len(), "every recorded picture differs");
        assert!(units[0].1 && units[1..].iter().all(|(_, key)| !key), "one key frame, first");

        let (server, relay, invites) = Server::start_relayed("e2e_encoded_lossy", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 320, 180).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(320, 180, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        const FOR: Duration = Duration::from_secs(38);
        let sent = units.clone();
        let feeding = tokio::task::spawn_blocking(move || {
            let (began, mut at, mut last) = (Instant::now(), 0, 0);
            let thirtieth = Duration::from_secs(1) / FPS;
            for n in 0.. {
                let captured = began + thirtieth * n;
                if captured > began + FOR {
                    break;
                }
                crate::clock::frame_clock(captured.saturating_duration_since(Instant::now()));
                if feeder.key_frame_wanted() {
                    at = 0;
                }
                let (data, key) = &sent[at];
                feeder.push(data, *key, 320, 180, captured);
                (last, at) = (at, after(at, sent.len()));
            }
            last
        });

        relay.impair(Schedule {
            late_at: 150,
            late_by: Duration::from_millis(400),
            lost_at: 300,
            // Past libwebrtc's asks to send it again (2 s was recovered), so its frame is lost for good.
            lost_for: Duration::from_secs(6),
            held_at: 650,
            held_by: Duration::from_millis(120),
            drift_at: 750,
            drift_to: Duration::from_millis(150),
            drift_over: Duration::from_secs(5),
        });
        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
        let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
        let (_bo_voice, mut bo_hearing) = voice_of(&state);
        let handed = common::Shared::new(Vec::<(Vec<u8>, bool)>::new());
        let record = Arc::clone(&handed);
        let _taken = bo_hearing
            .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, move |frame| {
                record.change(|handed| handed.push((frame.data.to_vec(), frame.key_frame)));
            })
            .await
            .expect("the screen is taken");
        let last = last_nal(&units[feeding.await.expect("the feeder ran")].0).to_vec();
        handed
            .wait(common::WAIT, |handed| handed.iter().any(|(data, _)| last_nal(data) == last).then_some(()))
            .await
            .expect("the last picture pushed is handed over");
        let handed = std::mem::take(&mut *handed.lock().expect("the frames"));
        let done = relay.impaired();
        println!(
            "{} frames handed over, {} key frames; the path did {done:?}",
            handed.len(),
            handed.iter().filter(|f| f.1).count()
        );

        assert!(done.late && done.lost && done.held && done.drifted > 100, "every part of the schedule ran: {done:?}");
        assert!(done.late_retransmissions > 0, "the late packet was sent again, and held");
        let order: Vec<usize> =
            handed.iter().map(|(data, _)| *index.get(last_nal(data)).expect("a recorded picture")).collect();
        for (n, pair) in order.windows(2).enumerate() {
            let (before, now) = (pair[0], pair[1]);
            assert!(
                now == 0 || now == after(before, units.len()),
                "frame {} is picture {now} after picture {before}: handed over out of order or past one lost",
                n + 1
            );
        }
        assert!(order.iter().filter(|at| **at == 0).count() >= 2, "a key frame came again after the loss");
        assert!(handed.len() >= 600, "the screen kept coming through it all ({})", handed.len());
        drop(sharing);
        drop(server);
    }
}

mod lab {
    use super::*;
    use testserver::{Loss, Way};

    /// Arrival gaps between handed frames longer than this are a freeze a viewer sees.
    const FREEZE: Duration = Duration::from_millis(250);

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_over_a_lossy_uplink_keeps_coming() {
        let units = fixture();
        let (server, relay, invites) = Server::start_relayed("e2e_encoded_uplink", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 320, 180).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(320, 180, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        const FOR: Duration = Duration::from_secs(30);
        relay.lose(Loss { way: Way::Up, per_thousand: 20, seed: 0x6464 });
        let sent = units.clone();
        let feeding = tokio::task::spawn_blocking(move || {
            let (began, mut at, mut asked, mut rates) = (Instant::now(), 0, 0usize, Vec::new());
            let thirtieth = Duration::from_secs(1) / FPS;
            for n in 0.. {
                let captured = began + thirtieth * n;
                if captured > began + FOR {
                    break;
                }
                crate::clock::frame_clock(captured.saturating_duration_since(Instant::now()));
                if feeder.key_frame_wanted() {
                    (at, asked) = (0, asked + 1);
                }
                rates.extend(feeder.bits_wanted());
                let (data, key) = &sent[at];
                feeder.push(data, *key, 320, 180, captured);
                at = after(at, sent.len());
            }
            (asked, rates)
        });
        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
        let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
        let (_bo_voice, mut bo_hearing) = voice_of(&state);
        let arrived = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let record = Arc::clone(&arrived);
        let _taken = bo_hearing
            .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, move |_| {
                if let Ok(mut arrived) = record.lock() {
                    arrived.push(Instant::now());
                }
            })
            .await
            .expect("the screen is taken");
        let (asked, mut rates) = feeding.await.expect("the feeder ran");
        let arrived = std::mem::take(&mut *arrived.lock().expect("the frames"));
        let gaps: Vec<Duration> = arrived.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let freezes = gaps.iter().filter(|gap| **gap > FREEZE).count();
        let longest = gaps.iter().max().copied().unwrap_or_default();
        let lost = relay.lost();
        let last = rates.last().copied().unwrap_or_default();
        rates.sort_unstable();
        let at = |share: usize| rates.get(rates.len() * share / 100).copied().unwrap_or_default();
        eprintln!(
            "uplink lost {} of {} RTP packets; {} frames handed over, {freezes} freezes over {} ms, the longest {} ms; \
             {asked} key frames asked; the engine's rate p10 {} p50 {} last {} kbit/s over {} asks",
            lost.lost,
            lost.seen,
            arrived.len(),
            FREEZE.as_millis(),
            longest.as_millis(),
            at(10) / 1000,
            at(50) / 1000,
            last / 1000,
            rates.len()
        );
        assert!(lost.lost > 0 && lost.lost * 100 / lost.seen.max(1) <= 3, "the uplink lost about 2 %: {lost:?}");
        assert!(arrived.len() >= 600, "the screen kept coming through the loss ({})", arrived.len());
        drop(sharing);
        drop(server);
    }

    /// How long the steady loss runs.
    const STEADY_FOR: Duration = Duration::from_secs(60);
    /// The span over which a key frame a second is held to: a burst split by its edge is one more.
    const KEY_FRAME_SPAN: usize = 10;

    /// A screen watched over a path that loses a steady share of the viewer's packets for a minute: the key frames
    /// the engine wanted against the viewer's asks the router passed on, and the time a packet waited to leave the
    /// sender, each second. A fault by the rate rules is more than a key frame a second sustained, or a send delay
    /// that keeps growing; the rest is printed, as a measure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_over_steady_loss_wants_a_key_frame_at_most_once_a_second_and_its_send_delay_holds() {
        let units = fixture();
        let (server, relay, invites) = Server::start_relayed("e2e_encoded_steady", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 320, 180).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(320, 180, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let began = Instant::now();
        let wanted = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let (sent, record) = (units.clone(), Arc::clone(&wanted));
        let feeding = tokio::task::spawn_blocking(move || {
            let mut at = 0;
            let thirtieth = Duration::from_secs(1) / FPS;
            for n in 0.. {
                let captured = began + thirtieth * n;
                if captured > began + STEADY_FOR {
                    break;
                }
                crate::clock::frame_clock(captured.saturating_duration_since(Instant::now()));
                if feeder.key_frame_wanted() {
                    at = 0;
                    if let Ok(mut wanted) = record.lock() {
                        wanted.push(Instant::now());
                    }
                }
                let (data, key) = &sent[at];
                feeder.push(data, *key, 320, 180, captured);
                at = after(at, sent.len());
            }
        });
        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
        let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
        let (_bo_voice, mut bo_hearing) = voice_of(&state);
        let _taken =
            bo_hearing.watch_encoded(&bo, &ada_id, webrtc::Video::Screen, |_| {}).await.expect("the screen is taken");
        relay.lose(Loss { way: Way::Down, per_thousand: 20, seed: 0x5eed });
        let mut ticks = tokio::time::interval(Duration::from_secs(1));
        let (mut last, mut counted) = (webrtc::SendStats::default(), 0);
        // Each second's key frames wanted, and the mean time a packet sent in it waited to leave.
        let mut seconds: Vec<(usize, Option<Duration>)> = Vec::new();
        while !feeding.is_finished() {
            ticks.tick().await;
            let now = sharing.stats().await.expect("the share's stats");
            let packets = now.packets_sent.saturating_sub(last.packets_sent);
            let waited = (now.send_delay_seconds - last.send_delay_seconds).max(0.0);
            let delay = (packets > 0).then(|| Duration::from_secs_f64(waited / packets as f64));
            let second = seconds.len();
            let so_far = wanted.lock().expect("the key frames wanted").len();
            let wanted_in = so_far - counted;
            let asks = now.picture_losses + now.full_intra_requests;
            eprintln!(
                "second {second}: {wanted_in} key frames wanted, {so_far} so far; {asks} asks passed on so far; \
                 {packets} packets, each waiting {} ms to leave",
                delay.map_or_else(|| "-".to_string(), |delay| format!("{:.1}", delay.as_secs_f64() * 1000.0))
            );
            seconds.push((wanted_in, delay));
            (last, counted) = (now, so_far);
        }
        feeding.await.expect("the feeder ran");
        let lost = relay.lost();
        let total = wanted.lock().expect("the key frames wanted").len();
        eprintln!(
            "the path down lost {} of {} RTP packets; {total} key frames wanted in {} s; the viewer's asks the router \
             passed on: {} PLI, {} FIR",
            lost.lost,
            lost.seen,
            seconds.len(),
            last.picture_losses,
            last.full_intra_requests
        );
        assert!(lost.lost > 0, "the path lost packets: {lost:?}");
        for (from, span) in seconds.windows(KEY_FRAME_SPAN).enumerate() {
            let in_span: usize = span.iter().map(|(wanted, _)| wanted).sum();
            assert!(
                in_span <= KEY_FRAME_SPAN + 1,
                "{in_span} key frames wanted in the {KEY_FRAME_SPAN} s from second {from}: more than one a second"
            );
        }
        let mean = |part: &[(usize, Option<Duration>)]| {
            let delays: Vec<Duration> = part.iter().filter_map(|(_, delay)| *delay).collect();
            delays.iter().sum::<Duration>().checked_div(u32::try_from(delays.len()).unwrap_or(u32::MAX))
        };
        let third = seconds.len() / 3;
        let (first, middle, end) =
            (mean(&seconds[..third]), mean(&seconds[third..2 * third]), mean(&seconds[2 * third..]));
        // A queue that never drains grows each third; one the pacer keeps up with does not.
        let growing = matches!((first, middle, end), (Some(first), Some(middle), Some(end))
            if first < middle && middle < end && end > first * 2 && end > Duration::from_millis(20));
        assert!(!growing, "the send delay keeps growing: {first:?}, then {middle:?}, then {end:?}");
        drop(sharing);
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_h264_screen_taken_encoded_is_decoded_by_the_host_in_the_colours_painted_and_a_key_frame_comes() {
        taken_encoded(media::Codec::H264, webrtc::Codec::H264).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_av1_screen_taken_encoded_is_decoded_by_the_host_in_the_colours_painted_and_a_key_frame_comes() {
        taken_encoded(media::Codec::Av1, webrtc::Codec::Av1).await;
    }

    #[expect(clippy::print_stderr, reason = "what was taken and decoded is read in the test's output")]
    async fn taken_encoded(codec: media::Codec, sent: webrtc::Codec) {
        let Some((encoder, canvas)) = candidates_of(codec).into_iter().next() else {
            return skipped(format!("no {codec:?} hardware encoder opens on this machine"));
        };
        let (server, invites) = Server::start("e2e_encoded", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) = ada_voice.share(&ada, sent, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        const TEN_SECONDS: usize = 10 * FPS as usize;
        let (fed, mut shared) = tokio::sync::watch::channel(0);
        let feeding = tokio::task::spawn_blocking(move || feed(&encoder, &canvas, &feeder, TEN_SECONDS, None, &fed));
        // Bo joins a share a second old.
        timeout(common::WAIT, shared.wait_for(|fed| *fed >= FPS as usize)).await.expect("the share runs").expect("fed");

        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state
            .streams
            .iter()
            .find(|s| s.kind == i32::from(TrackKind::ScreenVideo))
            .expect("ada's screen is live")
            .user_id
            .clone();
        let (bo_voice, mut bo_hearing) = voice_of(&state);
        let frames = common::Shared::new(Vec::<Frame>::new());
        let record = Arc::clone(&frames);
        let key_frames = bo_hearing
            .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, move |frame| {
                record.change(|frames| {
                    frames.push(Frame {
                        data: frame.data.to_vec(),
                        render_time_us: frame.render_time_us,
                        key_frame: frame.key_frame,
                    });
                });
            })
            .await
            .expect("the screen is taken");
        assert_eq!(key_frames.codec(), sent, "the screen arrives in the codec it was sent in");

        let started = Instant::now();
        let count = || frames.lock().map(|f| f.len()).unwrap_or(0);
        let keys = || frames.lock().map(|f| f.iter().filter(|frame| frame.key_frame).count()).unwrap_or(0);
        frames.wait(Duration::from_secs(20), |frames| (frames.len() >= 60).then_some(())).await;
        // Ask just after a key frame, so the next one the encoder would make on its own is nearly two seconds away.
        let before = keys();
        let asked_at = Instant::now();
        assert!(key_frames.ask(), "the sender is asked");
        frames
            .wait(Duration::from_millis(1_500), |frames| {
                (frames.iter().filter(|frame| frame.key_frame).count() > before).then_some(())
            })
            .await;
        let answered = keys() > before;
        eprintln!(
            "a key frame was asked for after {} frames; answered: {answered} in {:?}",
            count(),
            asked_at.elapsed()
        );
        frames
            .wait(Duration::from_secs(20).saturating_sub(started.elapsed()), |frames| {
                (frames.len() >= 200).then_some(())
            })
            .await;
        let engine = bo_voice.watch_stats_of(&ada_id, webrtc::Video::Screen).await.expect("the engine reports");
        let sent = sharing.stats().await.expect("the sender reports");
        // Nobody speaks: the call's counts are the screen's alone, and must not read zero.
        let (watched, shared) = (
            bo_voice.connection().await.expect("the watcher's engine reports"),
            ada_voice.connection().await.expect("the sharer's engine reports"),
        );
        eprintln!(
            "watcher received {} packets, {} bytes, heard {}; sharer sent {} packets, {} bytes",
            watched.received, watched.received_bytes, watched.heard, shared.sent, shared.sent_bytes
        );
        assert_eq!(watched.heard, 0, "no audio was taken");
        assert!(watched.received >= 100 && watched.received_bytes > 0, "the watched screen counts as received");
        assert!(shared.sent_bytes >= sent.bytes_sent && sent.bytes_sent > 0, "what was sent counts the screen");
        feeding.abort();
        assert!(bo_hearing.forget(&ada_id, TrackKind::ScreenVideo), "the screen was being taken");
        assert!(!key_frames.ask(), "a screen let go has nobody to ask, though its handle is kept");
        // Taken out of the lock: the engine's thread may be inside the callback, which takes it, while this drops the
        // tap.
        let frames = std::mem::take(&mut *frames.lock().expect("the tally"));

        let key_count = frames.iter().filter(|f| f.key_frame).count();
        eprintln!(
            "{} frames, {key_count} key frames, first is key: {}, engine: decoder {:?}, decoded {}, dropped {}; sender made {} key frames",
            frames.len(),
            frames.first().is_some_and(|f| f.key_frame),
            engine.decoder,
            engine.frames_decoded,
            engine.frames_dropped,
            sent.key_frames_encoded
        );
        assert!(frames.len() >= 100, "frames kept arriving with the engine's decoder a stand-in ({})", frames.len());
        assert!(frames.windows(2).all(|pair| pair[1].render_time_us >= pair[0].render_time_us), "in order");
        for key in frames.iter().filter(|f| f.key_frame) {
            assert!(starts_a_decoder(codec, &key.data), "a key frame carries what a decoder starts from");
        }
        assert!(answered, "a key frame came after the sender was asked");

        // The host decodes what the engine did not: from the first key frame on, the picture is the painting.
        let mut decoder = Decoder::open(codec, &Sources::system()).expect("a decoder");
        let mut pictures = 0;
        let mut right = 0;
        for frame in frames.iter().skip_while(|f| !f.key_frame) {
            for decoded in decoder
                .decode(&media::Coded::copied(&frame.data).expect("a unit"), frame.render_time_us)
                .expect("decode")
            {
                pictures += 1;
                right += usize::from(decoded_is_the_painting(&decoded.picture));
            }
        }
        eprintln!("the host decoded on {:?}: {pictures} pictures, {right} right", decoder.path());
        assert!(pictures >= 90, "the host decoded what it was given ({pictures})");
        assert!(pictures - right <= 2, "the pictures are the painting ({right} of {pictures})");
        drop(bo_voice);
        let _ = sharing;
    }

    /// A speaker that takes 10 ms frames and says they reach the device `delay` after they are played, as a real
    /// one's ring and graph do; its microphone hears silence.
    struct Delayed(Duration);

    struct Quiet;

    impl webrtc::chain::MicrophoneEnd for Quiet {
        fn frame(&mut self) -> Option<webrtc::chain::Mono> {
            None
        }
        fn delay(&self) -> Duration {
            Duration::ZERO
        }
    }

    impl webrtc::chain::Microphone for Quiet {
        fn keep(&mut self, _now: Instant) {}
        fn choose(&mut self, _device: Option<&str>) -> Result<(), webrtc::DevicesRefused> {
            Ok(())
        }
        fn is_open(&self) -> bool {
            true
        }
    }

    struct Late(Duration);

    impl webrtc::chain::SpeakerEnd for Late {
        fn play(&mut self, _frame: &webrtc::chain::Stereo) {}
        fn delay(&self) -> Duration {
            self.0
        }
    }

    impl webrtc::chain::Speaker for Late {
        fn keep(&mut self, _now: Instant) {}
        fn choose(&mut self, _device: Option<&str>) -> Result<(), webrtc::DevicesRefused> {
            Ok(())
        }
        fn is_open(&self) -> bool {
            true
        }
    }

    impl webrtc::chain::Sound for Delayed {
        fn microphone(&self, _device: Option<&str>, _wake: std::thread::Thread) -> webrtc::chain::MicrophoneOpened {
            Ok((Box::new(Quiet), Box::new(Quiet)))
        }
        fn speaker(&self, _device: Option<&str>, _now: Instant) -> webrtc::chain::SpeakerOpened {
            Ok((Box::new(Late(self.0)), Box::new(Late(self.0))))
        }
        fn listing(&self) -> Result<webrtc::chain::Listing, webrtc::DevicesRefused> {
            Ok(webrtc::chain::Listing::default())
        }
    }

    /// An engine whose speaker plays `delay` late.
    fn engine_playing_late(delay: Duration) -> webrtc::Engine {
        let plain = webrtc::Processing {
            echo_cancellation: false,
            noise: webrtc::Noise::Off,
            gain: webrtc::Gain::Fixed(0.0),
            gate: None,
        };
        webrtc::Engine::start(Arc::new(Delayed(delay)), plain)
    }

    /// A share with sound watched by a member whose speaker plays at once, then by one whose speaker plays 150 ms
    /// late: the engine holds each picture to its sound, so the second's pictures are held longer by about what its
    /// speaker adds. One at a time: two watchers in one process share the tap of a track.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_watched_screen_waits_for_its_sound_s_speaker() {
        const LATE: Duration = Duration::from_millis(150);
        // libwebrtc's sync moves a picture's delay about ten ms a second, so it is read once it has had time to settle.
        const SETTLE: Duration = Duration::from_secs(24);
        const READ: Duration = Duration::from_secs(4);
        let Some((encoder, canvas)) = candidates_of(media::Codec::H264).into_iter().next() else {
            return skipped("no H.264 hardware encoder opens on this machine");
        };
        let (server, invites) = Server::start("e2e_encoded_sync", 3).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) =
            ada_voice.share(&ada, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let sound = sharing.sound(&ada, 48_000, 2).await.expect("the share's sound starts");
        let tone = tokio::spawn(async move {
            // Paced by the clock, not by sleeps that add up: a sound that falls behind would move the picture with it.
            let mut every = tokio::time::interval(Duration::from_millis(10));
            for chunk in 0_u32.. {
                every.tick().await;
                let samples: Vec<i16> = (0..480)
                    .flat_map(|i| {
                        let t = f64::from(chunk * 480 + i) / 48_000.0;
                        let side = (8_000.0 * (std::f64::consts::TAU * 440.0 * t).sin()) as i16;
                        [side, side]
                    })
                    .collect();
                if !sound.push(&samples).await {
                    return;
                }
            }
        });
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let pushing = tokio::task::spawn_blocking(move || {
            let mut session = Session::open_screen(&encoder, settings(WIDTH, HEIGHT), layout(WIDTH, HEIGHT))
                .expect("the encoder takes the screen");
            let mut surface = canvas.surface(&session, WIDTH, HEIGHT).expect("a surface");
            surface.paint_rgb(&quadrants(WIDTH, HEIGHT));
            let thirtieth = Duration::from_secs(1) / FPS;
            let began = Instant::now() + thirtieth;
            let mut n = 0;
            while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                let captured = began + thirtieth * n;
                crate::clock::frame_clock(captured.saturating_duration_since(Instant::now()));
                if feeder.key_frame_wanted() {
                    session.request_key_frame();
                }
                for unit in session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode the screen") {
                    feeder.push(&unit.data, unit.key_frame, WIDTH as u32, HEIGHT as u32, captured);
                }
                n += 1;
            }
        });

        let mut held = Vec::new();
        for (name, invite, late) in [("bo", &invites[1], Duration::ZERO), ("cara", &invites[2], LATE)] {
            let engine = engine_playing_late(late);
            let (link, _events, state) = join_voice(&server.url(), name, invite).await;
            let (mut voice, mut hearing) = webrtc::Voice::new(
                &engine,
                &state,
                webrtc::Located::of(&state, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses"),
            )
            .expect("both connections");
            voice.play().expect("its speaker plays");
            let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
            let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
            hearing.take(&link, &ada_id, TrackKind::ScreenAudio).await.expect("the screen's sound is taken");
            let taken = hearing
                .watch_encoded(&link, &ada_id, webrtc::Video::Screen, |_frame| {})
                .await
                .expect("the screen is taken");
            let mut looks = common::looks::Looks::every(screens::LOOK_EVERY);
            looks.over(SETTLE).await;
            let before = voice.watch_stats_of(&ada_id, webrtc::Video::Screen).await.expect("the engine reports");
            looks.over(READ).await;
            let after = voice.watch_stats_of(&ada_id, webrtc::Video::Screen).await.expect("the engine reports");
            let pictures = after.held.saturating_sub(before.held);
            assert!(pictures > 60, "{name}'s screen kept coming ({pictures})");
            let mean = (after.held_seconds - before.held_seconds) / pictures as f64 * 1000.0;
            eprintln!("{name}, a speaker {} ms late: each picture held {mean:.1} ms", late.as_millis());
            held.push(mean);
            drop(taken);
            hearing.forget(&ada_id, TrackKind::ScreenVideo);
            drop((hearing, voice, link, engine));
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        pushing.await.expect("the feeder ran");
        tone.abort();
        let later = held[1] - held[0];
        eprintln!("the speaker {} ms late holds the picture {later:.1} ms longer", LATE.as_millis());
        assert!((later - LATE.as_secs_f64() * 1000.0).abs() < 45.0, "held to its sound within 45 ms: {later:.1} ms");
    }

    /// Pictures captured every thirtieth of a second, every fifth pushed 25 ms late as a slow encode would: the
    /// stream's clock is the capture's, so the late push barely moves the times the watcher's engine shows them at.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_watched_screen_s_clock_follows_its_capture_not_its_late_pushes() {
        late_pushes().await;
    }

    #[expect(clippy::print_stderr, reason = "the spacings compared are read in the test's output")]
    async fn late_pushes() {
        let Some((encoder, canvas)) = candidates_of(media::Codec::H264).into_iter().next() else {
            return skipped("no H.264 hardware encoder opens on this machine");
        };
        let (server, invites) = Server::start("e2e_encoded_clock", 2).await;
        let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, feeder) =
            ada_voice.share(&ada, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");
        let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut bo_hearing) = voice_of(&state);
        let ada_id = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
        let ada_id = ada_id.expect("ada's screen is live").user_id.clone();
        let stamps = common::Shared::new(Vec::<i64>::new());
        let record = Arc::clone(&stamps);
        let _taken = bo_hearing
            .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, move |frame| {
                record.change(|stamps| stamps.push(frame.render_time_us));
            })
            .await
            .expect("the screen is taken");

        const PICTURES: u32 = 300;
        let pushed = tokio::task::spawn_blocking(move || {
            let mut session = Session::open_screen(&encoder, settings(WIDTH, HEIGHT), layout(WIDTH, HEIGHT))
                .expect("the encoder takes the screen");
            let mut surface = canvas.surface(&session, WIDTH, HEIGHT).expect("a surface");
            surface.paint_rgb(&quadrants(WIDTH, HEIGHT));
            let thirtieth = Duration::from_secs(1) / FPS;
            let began = Instant::now() + thirtieth;
            let mut pushed = Vec::new();
            for n in 0..PICTURES {
                let captured = began + thirtieth * n;
                let late = if n % 5 == 4 { Duration::from_millis(25) } else { Duration::ZERO };
                crate::clock::frame_clock((captured + late).saturating_duration_since(Instant::now()));
                if feeder.key_frame_wanted() {
                    session.request_key_frame();
                }
                for unit in session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode the screen") {
                    feeder.push(&unit.data, unit.key_frame, WIDTH as u32, HEIGHT as u32, captured);
                    pushed.push(Instant::now());
                }
            }
            pushed
        });
        let pushed = pushed.await.expect("the feeder ran");
        // Every unit pushed is taken, or the count below says how many were not.
        let _ = stamps.wait(common::WAIT, |stamps| (stamps.len() >= pushed.len()).then_some(())).await;
        let stamps = std::mem::take(&mut *stamps.lock().expect("the stamps"));

        // The middle nine tenths of the spacings, in milliseconds: a start's and a key frame's outliers left out.
        let spread = |mut gaps: Vec<f64>| {
            gaps.sort_by(f64::total_cmp);
            let at = |percent: usize| gaps[(gaps.len() - 1) * percent / 100];
            (at(5), at(50), at(95))
        };
        let pushes = spread(pushed.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect());
        let shown = spread(stamps.windows(2).map(|w| (w[1] - w[0]) as f64 / 1000.0).collect());
        eprintln!(
            "{} pushed, {} taken; push spacing {:.1}/{:.1}/{:.1} ms, shown spacing {:.1}/{:.1}/{:.1} ms (5th/50th/95th)",
            pushed.len(),
            stamps.len(),
            pushes.0,
            pushes.1,
            pushes.2,
            shown.0,
            shown.1,
            shown.2
        );
        assert!(stamps.len() >= 200, "the screen kept arriving ({})", stamps.len());
        assert!((shown.1 - 33.3).abs() < 3.0, "a thirtieth apart in the middle ({:.1} ms)", shown.1);
        assert!(
            shown.2 - shown.0 < (pushes.2 - pushes.0) / 2.0,
            "the late pushes left half their spread or less in the stream's clock"
        );
        drop(bo_voice);
        let _ = sharing;
    }
}
