//! A screen picture on the GPU, over the real wire, from a DMA-BUF or a D3D11 texture in place of the compositor's to
//! a second engine. What arrives is judged by colour, so a wrong colour matrix, a swapped channel or a stale picture
//! fails.

use crate::common;
use crate::screen_support;

use cli::{Account, Act, Command, WatchTo, run};
use common::{Person, Server, hears_a_tone, join_voice, told, voice_of};
use link::Trust;
use media::{Decoder, Sources};
use platform::Platform;
use proto::chatkord_v1::{TrackKind, event};
use screen_support::{
    Canvas, FPS, HEIGHT, QUADRANTS, WIDTH, candidates, candidates_of, decoded_is_the_painting, expected, feed,
    obu_types,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use terms::Address;
use tier::skipped;
use tokio::time::timeout;
use webrtc::Picture;

/// Whether the middle of each quadrant of a decoded picture is the colour that was painted there.
fn is_the_painting(picture: &Picture<'_>) -> bool {
    let (width, height) = (picture.width as usize, picture.height as usize);
    let (luma_stride, blue_stride, red_stride) =
        (picture.strides.0 as usize, picture.strides.1 as usize, picture.strides.2 as usize);
    QUADRANTS.iter().enumerate().all(|(i, rgb)| {
        let (x, y) = (width / 4 + (i % 2) * width / 2, height / 4 + (i / 2) * height / 2);
        let got = [
            f64::from(picture.luma[y * luma_stride + x]),
            f64::from(picture.blue[(y / 2) * blue_stride + x / 2]),
            f64::from(picture.red[(y / 2) * red_stride + x / 2]),
        ];
        got.iter().zip(expected(*rgb)).all(|(g, w)| (g - w).abs() <= 6.0)
    })
}

#[derive(Default)]
struct Seen {
    pictures: usize,
    right: usize,
    sizes: Vec<(u32, u32)>,
    first_right: Option<Instant>,
}

/// Shares for `pictures` frames in each codec on each encoder that takes the screen; a viewer joins once `join_after`
/// pictures were shared and watches until it has seen `wanted` right pictures.
async fn share_and_watch(
    pictures: usize,
    resize_at: Option<(usize, (usize, usize))>,
    join_after: usize,
    wanted: usize,
) {
    for (codec, sent_as) in [(media::Codec::H264, webrtc::Codec::H264), (media::Codec::Av1, webrtc::Codec::Av1)] {
        let encoders = candidates_of(codec);
        if encoders.is_empty() {
            skipped(format!("no {codec:?} encoder takes the screen on this machine"));
        }
        for (encoder, canvas) in encoders {
            watch_one(encoder, canvas, sent_as, pictures, resize_at, join_after, wanted).await;
        }
    }
}

/// One share of `share_and_watch`, on `encoder`, sent as `sent_as`.
async fn watch_one(
    encoder: media::Encoder,
    canvas: Canvas,
    sent_as: webrtc::Codec,
    pictures: usize,
    resize_at: Option<(usize, (usize, usize))>,
    join_after: usize,
    wanted: usize,
) {
    let name = encoder.to_string();
    let (server, invites) = Server::start("e2e_screen", 2).await;
    let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
    let (ada_voice, _ada_hearing) = voice_of(&joined);
    let (sharing, feeder) = ada_voice.share(&ada, sent_as, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");
    let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
    sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
    let (fed, mut shared) = tokio::sync::watch::channel(0);
    let feeding = tokio::task::spawn_blocking(move || feed(&encoder, &canvas, &feeder, pictures, resize_at, &fed));
    timeout(common::WAIT, shared.wait_for(|fed| *fed >= join_after)).await.expect("the share runs").expect("fed");

    let seen = common::Shared::new(Seen::default());
    let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
    let ada_id = state
        .streams
        .iter()
        .find(|s| s.kind == i32::from(TrackKind::ScreenVideo))
        .expect("ada's screen is live")
        .user_id
        .clone();
    let (receiving, mut taking) = voice_of(&state);
    let record = Arc::clone(&seen);
    let watched_at = Instant::now();
    taking
        .watch(&bo, &ada_id, webrtc::Video::Screen, move |picture| {
            record.change(|seen| {
                seen.pictures += 1;
                if is_the_painting(&picture) {
                    seen.right += 1;
                    seen.first_right.get_or_insert_with(Instant::now);
                }
                if seen.sizes.last() != Some(&(picture.width, picture.height)) {
                    seen.sizes.push((picture.width, picture.height));
                }
            });
        })
        .await
        .expect("the screen is taken");

    let enough = |seen: &Seen| {
        seen.right >= wanted && resize_at.is_none_or(|(_, size)| seen.sizes.contains(&(size.0 as u32, size.1 as u32)))
    };
    seen.wait(Duration::from_secs(25), |seen| enough(seen).then_some(())).await;
    let engine = sharing.stats().await.expect("the engine reports");
    let keys = feeding.await.expect("the feeding finished");
    let seen = seen.lock().expect("the tally");
    eprintln!(
        "{name}: after joining {join_after} pictures into the share, bo saw {} pictures, {} right, the first {:?} after watching, sizes {:?}; the engine asked for {keys} key frames",
        seen.pictures,
        seen.right,
        seen.first_right.map(|at| at.duration_since(watched_at)),
        seen.sizes
    );
    eprintln!(
        "{name}: the engine estimates {:.2} Mbit/s and aims the encoder at {:.2} (the share stated {:.2} to {:.2}), sent {} bytes, {} key frames",
        engine.estimate_bits_per_second / 1e6,
        engine.target_bits_per_second / 1e6,
        envelope.min as f64 / 1e6,
        envelope.max as f64 / 1e6,
        engine.bytes_sent,
        engine.key_frames_encoded
    );
    assert!(engine.bytes_sent > 0, "{name}: the engine counts what it sent");
    // Left to itself the estimate starts near 0.3 Mbit/s; a share that states its rates starts at them.
    assert!(
        engine.estimate_bits_per_second >= envelope.start as f64 * 0.9,
        "{name}: the estimate began where the share stated, not at the engine's default"
    );
    assert!(
        seen.right >= wanted,
        "{name}: the pictures arrived in the colours painted ({} of {})",
        seen.right,
        seen.pictures
    );
    assert!(
        seen.pictures - seen.right <= 2,
        "{name}: at most the first pictures are not the painting ({} of {})",
        seen.pictures - seen.right,
        seen.pictures
    );
    if let Some((_, size)) = resize_at {
        assert_eq!(seen.sizes.last(), Some(&(size.0 as u32, size.1 as u32)), "{name}: the viewer follows the new size");
    }
    let _ = (sharing, receiving, taking);
}

/// Bo records ada's share in `sent_as` on the first encoder of `codec` that takes the screen, from the command line
/// into a file of `extension` named by `to`, for two seconds once it has said so; the file and what the command said.
async fn recorded_from_the_command_line(
    codec: media::Codec,
    sent_as: webrtc::Codec,
    extension: &str,
    to: impl FnOnce(String) -> WatchTo,
) -> Option<(Vec<u8>, Vec<String>)> {
    let Some((encoder, canvas)) = candidates_of(codec).into_iter().next() else {
        skipped(format!("no {codec:?} encoder takes the screen on this machine"));
        return None;
    };
    let (server, invites) = Server::start("e2e_screen", 2).await;
    let address = Address::typed(&server.url()).expect("worth trying");
    let (ada, _events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
    let channel_id = joined.channel_id.clone();
    let (ada_voice, _ada_hearing) = voice_of(&joined);
    let (_sharing, feeder) = ada_voice.share(&ada, sent_as, WIDTH as u32, HEIGHT as u32).await.expect("sharing starts");
    let (fed, mut shared) = tokio::sync::watch::channel(0);
    let feeding = tokio::task::spawn_blocking(move || feed(&encoder, &canvas, &feeder, 300, None, &fed));

    let said = common::Shared::new(Vec::new());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut viewer = Person { invite: invites[1].clone(), said: Arc::clone(&said), stop: Some(stopped) };
    run(
        Command::As {
            account: Account { address: address.clone(), identity: "bo".into() },
            act: Act::Register(Some("Bo".into())),
        },
        &mut viewer,
        &Platform::default(),
        &Trust::system(),
    )
    .await
    .expect("registered");
    let path = std::env::temp_dir().join(format!("chatkord-watch-{}.{extension}", std::process::id()));
    let trust = Trust::system();
    let platform = Platform::default();
    let watching = run(
        Command::As {
            account: Account { address, identity: "bo".into() },
            act: Act::Watch {
                channel_id: terms::ChannelId::parse(&channel_id).expect("a channel id"),
                to: to(path.display().to_string()),
            },
        },
        &mut viewer,
        &platform,
        &trust,
    );
    let checking = async {
        said.wait(Duration::from_secs(30), |said| said.iter().any(|line| line.starts_with("Recording ")).then_some(()))
            .await
            .expect("the viewer says it is recording");
        // Two seconds of pictures shared once it is.
        let from = *shared.borrow();
        let recorded = shared.wait_for(|fed| *fed >= from + 2 * FPS as usize);
        timeout(common::WAIT, recorded).await.expect("the share runs").expect("fed");
        stop.send(()).expect("the command is still running");
    };
    let (watched, ()) = tokio::join!(watching, checking);
    watched.expect("it left cleanly");
    feeding.abort();

    let bytes = std::fs::read(&path).expect("the file was written");
    let _ = std::fs::remove_file(&path);
    let said = said.lock().expect("a lock").clone();
    Some((bytes, said))
}

/// AV1's temporal delimiter OBU, which starts a temporal unit.
const TEMPORAL_DELIMITER: u8 = 2;
/// AV1's sequence header OBU, which a decoder starts from.
const SEQUENCE_HEADER: u8 = 1;

/// A stream of low-overhead OBUs cut at its temporal delimiters, each unit starting with one.
fn temporal_units(stream: &[u8]) -> Vec<Vec<u8>> {
    let mut units: Vec<Vec<u8>> = Vec::new();
    let mut at = 0;
    while let Some(&header) = stream.get(at) {
        let start = at;
        at += 1 + usize::from((header >> 2) & 1);
        let (mut size, mut shift) = (0usize, 0);
        while let Some(byte) = stream.get(at) {
            size |= usize::from(byte & 0x7f) << shift;
            (at, shift) = (at + 1, shift + 7);
            if byte & 0x80 == 0 {
                break;
            }
        }
        at = (at + size).min(stream.len());
        if (header >> 3) & 0x0f == TEMPORAL_DELIMITER || units.is_empty() {
            units.push(Vec::new());
        }
        if let Some(unit) = units.last_mut() {
            unit.extend_from_slice(&stream[start..at]);
        }
    }
    units
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_on_the_gpu_reaches_a_member_who_joins_late_in_the_colours_it_was_painted() {
        share_and_watch(300, None, 3 * FPS as usize, 30).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_shared_screen_that_changes_size_is_followed_by_the_viewer() {
        share_and_watch(300, Some((120, (640, 360))), 0, 60).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_command_line_records_an_av1_screen_as_sent_in_temporal_units_a_player_can_split() {
        let Some((bytes, said)) =
            recorded_from_the_command_line(media::Codec::Av1, webrtc::Codec::Av1, "obu", WatchTo::Stream).await
        else {
            return;
        };
        assert_eq!(bytes.first().map(|header| (header >> 3) & 0x0f), Some(TEMPORAL_DELIMITER), "{said:?}");
        let units = temporal_units(&bytes);
        assert!(
            units.first().is_some_and(|unit| obu_types(unit).contains(&SEQUENCE_HEADER)),
            "the file starts at a sequence header, where a player can; {} temporal units",
            units.len()
        );
        let mut decoder = Decoder::open(media::Codec::Av1, &Sources { render_nodes: vec![], ..Sources::default() })
            .expect("libdav1d");
        let mut pictures = Vec::new();
        for (pts, unit) in units.iter().enumerate() {
            pictures.extend(
                decoder
                    .decode(&media::Coded::copied(unit).expect("a unit"), pts as i64)
                    .expect("each temporal unit decodes"),
            );
        }
        pictures.extend(decoder.finish().expect("finish"));
        eprintln!("{} temporal units in {} bytes; {} pictures decoded", units.len(), bytes.len(), pictures.len());
        assert!(units.len() >= 30, "{} temporal units", units.len());
        assert_eq!(pictures.len(), units.len(), "one picture for each temporal unit");
        assert!(
            pictures.last().is_some_and(|last| decoded_is_the_painting(&last.picture)),
            "the recorded stream decodes to the painted colours"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_members_speak_share_hear_and_watch_at_once() {
        let Some((encoder, canvas)) = candidates().into_iter().next() else {
            return;
        };
        let (server, invites) = Server::start("e2e_screen", 2).await;
        let microphone = i32::from(TrackKind::Microphone);

        // Ada speaks and shares a screen, each over her one send connection.
        let (ada, mut ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, mut ada_hearing) = voice_of(&joined);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let (sharing, feeder) =
            ada_voice.share(&ada, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32).await.expect("ada shares");
        let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let fed = tokio::sync::watch::Sender::new(0);
        let feeding = tokio::task::spawn_blocking(move || feed(&encoder, &canvas, &feeder, 240, None, &fed));

        // Bo joins, speaks, takes ada's tone and watches her screen.
        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut bo_hearing) = voice_of(&state);
        let _bo_speaking = bo_voice.speak_tone(&bo).await.expect("bo speaks");
        let ada_id = state.streams.iter().find(|s| s.kind == microphone).expect("ada's tone is live").user_id.clone();
        bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada's tone");
        let seen = common::Shared::new(Seen::default());
        let record = Arc::clone(&seen);
        bo_hearing
            .watch(&bo, &ada_id, webrtc::Video::Screen, move |picture| {
                record.change(|seen| {
                    seen.pictures += 1;
                    seen.right += usize::from(is_the_painting(&picture));
                });
            })
            .await
            .expect("bo takes ada's screen");

        // Ada takes bo's tone as the room tells her it is live.
        let told_of = told(
            &mut ada_events,
            |kind| matches!(kind, event::Kind::ProducerJoined(joined) if joined.kind == microphone),
        )
        .await;
        let event::Kind::ProducerJoined(bo_stream) = told_of else { panic!("not a stream") };
        ada_hearing.take(&ada, &bo_stream.user_id, TrackKind::Microphone).await.expect("ada takes bo's tone");

        // Everything arrives at once: the screen in the colours painted, and each person hears the other.
        let (heard_by_bo, heard_by_ada) = tokio::join!(hears_a_tone(&bo_voice), hears_a_tone(&ada_voice));
        seen.wait(Duration::from_secs(25), |seen| (seen.right >= 30).then_some(())).await;
        let right = seen.lock().expect("the tally").right;
        assert!(right >= 30, "the screen arrived in the colours painted ({right} right pictures)");
        assert!(
            heard_by_bo > 5 && heard_by_ada > 5,
            "both tones crossed while the screen did: bo heard {heard_by_bo}, ada heard {heard_by_ada}"
        );

        // The screen ends; the tones carry on.
        sharing.stop(&ada).await.expect("the screen ends");
        let (heard_by_bo, heard_by_ada) = tokio::join!(hears_a_tone(&bo_voice), hears_a_tone(&ada_voice));
        assert!(
            heard_by_bo > 5 && heard_by_ada > 5,
            "after the screen ended: bo heard {heard_by_bo}, ada heard {heard_by_ada}"
        );
        feeding.await.expect("the feeding finished");
    }
}
