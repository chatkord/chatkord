//! What one media worker spends on a call, read from its thread: real members each sending a tone and hearing
//! everyone, then a busy 1080p60 screen with its sound every other member takes, then eight busy 720p30 cameras every
//! other member takes instead, each picture encoded and never decoded. A measurement, run on request; its numbers go
//! in the commit that sets the worker's budget, and each step is held to the budget's claim on one core.

use crate::common;
use crate::screen_support;

use capture::Pixels;
use common::{Server, registered_as};
use media::Session;
use proto::chatkord_v1::{TrackKind, VoiceJoin};
use screen_support::{candidates_of, layout, settings};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tier::skipped;
use tokio::sync::watch;
use tokio::time::timeout;

const MEMBERS: usize = 28;
const STEADY: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(5);
const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;
const FPS: u32 = 60;
const CAMERAS: usize = 8;
const CAMERA_WIDTH: usize = 1280;
const CAMERA_HEIGHT: usize = 720;
const CAMERA_FPS: u32 = 30;
/// The pace takes are asked at, within the socket's rate: a take is three asks.
const TAKE_PACE: Duration = Duration::from_millis(250);
/// What the budget claims of one worker's thread at its full room: four fifths of one core, so it never falls behind.
const MOST_OF_A_CORE: f64 = 0.8;

/// The ids in a `/proc` folder: its entries whose names are numbers.
fn numbered(folder: &Path) -> Vec<(u32, PathBuf)> {
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    entries.filter_map(|entry| Some((entry.file_name().to_string_lossy().parse().ok()?, entry.path()))).collect()
}

/// The media worker threads on this machine, by process and thread id.
fn workers() -> BTreeSet<(u32, u32)> {
    let threads = numbered(Path::new("/proc"))
        .into_iter()
        .flat_map(|(pid, path)| numbered(&path.join("task")).into_iter().map(move |(tid, task)| (pid, tid, task)));
    threads
        .filter(|(_, _, task)| {
            std::fs::read_to_string(task.join("comm")).is_ok_and(|comm| comm.starts_with("mediasoup-worke"))
        })
        .map(|(pid, tid, _)| (pid, tid))
        .collect()
}

/// The thread's user and system time, in clock ticks.
fn ticks((pid, tid): (u32, u32)) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/stat")).expect("the worker's thread");
    let fields: Vec<&str> = stat.rsplit_once(')').expect("a stat line").1.split_whitespace().collect();
    // Counted from the state after the name: utime is the 12th, stime the 13th.
    fields[11].parse::<u64>().expect("utime") + fields[12].parse::<u64>().expect("stime")
}

fn clock_ticks() -> f64 {
    let out = std::process::Command::new("getconf").arg("CLK_TCK").output().expect("getconf");
    String::from_utf8_lossy(&out.stdout).trim().parse().expect("a number")
}

/// The share of one core the worker spent over a steady window.
async fn core_share(worker: (u32, u32), per_second: f64) -> f64 {
    let mut looks = common::looks::Looks::every(screens::LOOK_EVERY);
    looks.over(SETTLE).await;
    let (before, at) = (ticks(worker), Instant::now());
    looks.over(STEADY).await;
    (ticks(worker) - before) as f64 / per_second / at.elapsed().as_secs_f64()
}

/// A member of the call: its connection, its media and its tone, on an engine of its own as each person's client is,
/// so its chain mixes what one client hears and never the whole room's.
struct Member {
    link: link::Link,
    _events: link::Events,
    id: String,
    voice: webrtc::Voice,
    hearing: webrtc::Hearing,
    _speaking: webrtc::Speaking,
    _engine: webrtc::Engine,
}

async fn joined(url: &str, name: &str, invite: &str) -> Member {
    let (link, events, booth, id) = registered_as(url, name, invite).await;
    let Ok(state) = link.ask(VoiceJoin { channel_id: booth, ..Default::default() }).await else {
        panic!("not a voice state")
    };
    let engine = tokio::task::spawn_blocking(call::devices::engine).await.expect("an engine");
    let (voice, hearing) = webrtc::Voice::new(
        &engine,
        &state,
        webrtc::Located::of(&state, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses"),
    )
    .expect("both connections");
    let speaking = voice.speak_tone(&link).await.expect("speaks");
    Member { link, _events: events, id: id.as_str().to_owned(), voice, hearing, _speaking: speaking, _engine: engine }
}

/// Every member takes the microphone of every other it does not hear yet.
async fn everyone_hears_everyone(members: &mut [Member]) {
    let ids: Vec<String> = members.iter().map(|member| member.id.clone()).collect();
    let taking = members.iter_mut().map(|member| {
        let heard = member.hearing.taken(TrackKind::Microphone);
        let others: Vec<String> = ids.iter().filter(|id| **id != member.id && !heard.contains(id)).cloned().collect();
        async move {
            let mut pace = common::looks::paced(TAKE_PACE);
            for other in others {
                pace.tick().await;
                member.hearing.take(&member.link, &other, TrackKind::Microphone).await.expect("heard");
            }
        }
    });
    futures_util::future::join_all(taking).await;
}

/// Eight pictures of a busy screen, a block moving over fine lines.
fn busy(
    canvas: &screen_support::Canvas,
    session: &Session,
    width: usize,
    height: usize,
) -> Vec<screen_support::Surface> {
    (0..8)
        .map(|n| {
            let mut surface = canvas.surface(session, width, height).expect("a surface");
            let painted: Vec<[u8; 3]> = (0..width * height)
                .map(|i| {
                    let (x, y) = (i % width, i / width);
                    if (x + n * width / 8) % width < width / 6 && y < height / 3 {
                        [235, 235, 235]
                    } else if (x / 2 + y + n) % 9 < 2 {
                        [40, 40, 40]
                    } else {
                        [200, 200, 200]
                    }
                })
                .collect();
            surface.paint_rgb(&painted);
            surface
        })
        .collect()
}

/// Encodes a busy picture at the envelope's most and pushes each unit to every feeder until `stop`: one encoder stands
/// in for as many senders as there are feeders.
fn send_busy(
    encoder: &media::Encoder,
    canvas: &screen_support::Canvas,
    (width, height, fps, bits): (usize, usize, u32, u64),
    feeders: &[webrtc::Feeder],
    stop: &AtomicBool,
    fed: &watch::Sender<usize>,
) {
    let wanted = media::Settings { frames_per_second: fps, bits_per_second: bits as u32, ..settings(width, height) };
    let mut session = Session::open_screen(encoder, wanted, layout(width, height)).expect("the encoder opens");
    let surfaces = busy(canvas, &session, width, height);
    let frame_time = Duration::from_micros(1_000_000 / u64::from(fps));
    let mut n = 0;
    while !stop.load(Ordering::Relaxed) {
        if feeders.iter().any(webrtc::Feeder::key_frame_wanted) {
            session.request_key_frame();
        }
        let frame = surfaces[n % surfaces.len()].frame(Pixels::Bgrx);
        for unit in session.encode_frame(frame).expect("encodes") {
            for feeder in feeders {
                feeder.push(&unit.data, unit.key_frame, width as u32, height as u32, unit.captured);
            }
        }
        n += 1;
        fed.send_replace(n);
        crate::clock::frame_clock(frame_time);
    }
}

/// A 440 Hz tone in 10 ms stereo frames, pushed in time until the task is ended.
async fn sound(feeder: webrtc::SoundFeeder) {
    let mut every = tokio::time::interval(Duration::from_millis(10));
    let mut at = 0_u64;
    loop {
        every.tick().await;
        let frame: Vec<i16> = (0..480)
            .flat_map(|i| {
                let phase = (at + i) as f64 * 440.0 * std::f64::consts::TAU / 48_000.0;
                let sample = (phase.sin() * 8_000.0) as i16;
                [sample, sample]
            })
            .collect();
        at += 480;
        assert!(feeder.push(&frame).await, "the engine takes 10 ms of stereo");
    }
}

/// Measures one step, held to the budget's claim; what it spent, said.
async fn step(worker: (u32, u32), per_second: f64, what: &str) -> String {
    let spent = core_share(worker, per_second).await;
    assert!(spent < MOST_OF_A_CORE, "{what} took {:.1} % of a core", spent * 100.0);
    format!("{what}: {:.1} % of a core", spent * 100.0)
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[ignore = "a measurement, run on request"]
    async fn what_one_media_worker_spends_on_a_call_of_voice_a_share_and_cameras() {
        let Some((encoder, canvas)) = candidates_of(media::Codec::H264).into_iter().next() else {
            return skipped("no hardware H.264 encoder opens on this machine");
        };
        let per_second = clock_ticks();
        let before = workers();
        let (server, invites) =
            Server::start_with("e2e_budget", MEMBERS, &[(testserver::Limit::MaxPerAddress, 64)]).await;
        let started: Vec<_> = workers().difference(&before).copied().collect();
        let [worker] = started[..] else { panic!("one new media worker, not {started:?}") };
        let idle = core_share(worker, per_second).await;
        eprintln!("idle: {:.1} % of a core", idle * 100.0);

        let mut members = Vec::new();
        for (n, invite) in invites.iter().enumerate() {
            members.push(joined(&server.url(), &format!("m{n}"), invite).await);
            if members.len() == MEMBERS / 2 || members.len() == MEMBERS {
                everyone_hears_everyone(&mut members).await;
                let count = members.len();
                let what = format!("{count} members, {} microphone consumers", count * (count - 1));
                eprintln!("{}", step(worker, per_second, &what).await);
            }
        }

        let sharer = members[0].id.clone();
        let (sharing, feeder) = members[0]
            .voice
            .share(&members[0].link, webrtc::Codec::H264, WIDTH as u32, HEIGHT as u32)
            .await
            .expect("sharing starts");
        let envelope = net::Envelope::for_screen(WIDTH as u32, HEIGHT as u32, FPS, None);
        sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
        let sound_feeder = sharing.sound(&members[0].link, 48_000, 2).await.expect("the share's sound starts");
        let shown = (WIDTH, HEIGHT, FPS, envelope.max);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (fed, mut shared) = watch::channel(0);
        let feeding = tokio::task::spawn_blocking(move || {
            send_busy(&encoder, &canvas, shown, &[feeder], &stopped, &fed);
            (encoder, canvas)
        });
        let sounding = tokio::spawn(sound(sound_feeder));
        // The share is two seconds old when it is watched.
        let old = 2 * FPS as usize;
        timeout(common::WAIT, shared.wait_for(|fed| *fed >= old)).await.expect("the share runs").expect("fed");
        let mut watching = Vec::new();
        for member in members.iter_mut().skip(1) {
            let link = member.link.clone();
            member.hearing.take(&link, &sharer, TrackKind::ScreenAudio).await.expect("the share's sound is taken");
            watching.push(
                member
                    .hearing
                    .watch_encoded(&link, &sharer, webrtc::Video::Screen, |_| {})
                    .await
                    .expect("the screen is taken"),
            );
        }
        let what = format!("{MEMBERS} members and one 1080p60 share with its sound taken by {}", watching.len());
        eprintln!("{}", step(worker, per_second, &what).await);
        let sent = sharing.stats().await.expect("the sender reports");
        eprintln!("the share sent {} bytes at a cap of {} bits a second", sent.bytes_sent, envelope.max);
        sharing.stop(&members[0].link).await.expect("the share ends");
        stop.store(true, Ordering::Relaxed);
        sounding.abort();
        let (encoder, canvas) = feeding.await.expect("the share's encoder ends");
        drop(watching);

        let envelope = net::Envelope::for_camera(CAMERA_WIDTH as u32, CAMERA_HEIGHT as u32, CAMERA_FPS, None);
        let mut cameras = Vec::new();
        let mut feeders = Vec::new();
        for member in &members[..CAMERAS] {
            let (camera, feeder) = member
                .voice
                .camera(&member.link, webrtc::Codec::H264, CAMERA_WIDTH as u32, CAMERA_HEIGHT as u32)
                .await
                .expect("the camera starts");
            camera.set_rates(envelope.min, envelope.start, envelope.max).expect("the camera states its rates");
            cameras.push(camera);
            feeders.push(feeder);
        }
        let shown = (CAMERA_WIDTH, CAMERA_HEIGHT, CAMERA_FPS, envelope.max);
        stop.store(false, Ordering::Relaxed);
        let stopped = Arc::clone(&stop);
        let (fed, mut shared) = watch::channel(0);
        let feeding =
            tokio::task::spawn_blocking(move || send_busy(&encoder, &canvas, shown, &feeders, &stopped, &fed));
        // The cameras are two seconds old when they are watched.
        let old = 2 * CAMERA_FPS as usize;
        timeout(common::WAIT, shared.wait_for(|fed| *fed >= old)).await.expect("the cameras run").expect("fed");
        let senders: Vec<String> = members[..CAMERAS].iter().map(|member| member.id.clone()).collect();
        let mut watching = Vec::new();
        let mut pace = common::looks::paced(TAKE_PACE);
        for member in &mut members {
            let link = member.link.clone();
            for sender in senders.iter().filter(|sender| **sender != member.id) {
                pace.tick().await;
                watching.push(
                    member
                        .hearing
                        .watch_encoded(&link, sender, webrtc::Video::Camera, |_| {})
                        .await
                        .expect("a camera is taken"),
                );
            }
        }
        let what = format!("{MEMBERS} members and {CAMERAS} 720p30 cameras, {} takes", watching.len());
        eprintln!("{}", step(worker, per_second, &what).await);
        for camera in &cameras {
            let sent = camera.stats().await.expect("the sender reports");
            eprintln!("a camera sent {} bytes at a cap of {} bits a second", sent.bytes_sent, envelope.max);
        }
        stop.store(true, Ordering::Relaxed);
        feeding.await.expect("the cameras' encoder ends");
    }
}
