//! Leaving a call while it shows many screens: how soon the others see the member gone, and how soon everything the
//! call held on this device is let go. Eight members each send the recorded screen; a ninth watches them all, as
//! the window does, and leaves.

use crate::common;

use call::room::Report;
use common::{Server, join_voice, registered_as, told, voice_of};
use proto::chatkord_v1::event;
use screens::Watched;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SCREENS: usize = 8;
const FPS: u32 = 30;
const SIZE: (u32, u32) = (320, 180);
/// How soon the others must see a leave, whatever the leaver was watching.
const SEEN_WITHIN: Duration = Duration::from_secs(1);

/// The shared screen `record_share_fixture` recorded: each unit and whether it is a key frame.
fn fixture() -> Vec<(Vec<u8>, bool)> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/share-h264-320x180.txt"))
        .expect("the recorded screen");
    text.lines()
        .map(|line| {
            let (kind, hex) = line.split_once(' ').expect("a kind and a unit");
            let digit = |digit: u8| u8::try_from((digit as char).to_digit(16).expect("hex")).expect("a digit");
            let data = hex.as_bytes().chunks(2).map(|pair| digit(pair[0]) << 4 | digit(pair[1])).collect();
            (data, kind == "key")
        })
        .collect()
}

/// Sends the recorded screen over `feeder` again and again until `stop`, from its key frame whenever one is wanted.
fn feed(feeder: &webrtc::Feeder, units: &[(Vec<u8>, bool)], stop: &AtomicBool) {
    let (began, mut at) = (Instant::now(), 0);
    let every = Duration::from_secs(1) / FPS;
    for n in 0.. {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let captured = began + every * n;
        crate::clock::frame_clock(captured.saturating_duration_since(Instant::now()));
        if feeder.key_frame_wanted() {
            at = 0;
        }
        let (data, key) = &units[at];
        feeder.push(data, *key, SIZE.0, SIZE.1, captured);
        at = if at + 1 == units.len() { 1 } else { at + 1 };
    }
}

mod server {
    use super::*;

    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_leave_is_seen_by_the_others_at_once_whatever_was_watched() {
            let units = Arc::new(fixture());
            let (server, invites) = Server::start("e2e_leave", SCREENS + 1).await;
            let stop = Arc::new(AtomicBool::new(false));
            let mut sharers = Vec::new();
            for (at, invite) in invites[..SCREENS].iter().enumerate() {
                let (link, events, joined) = join_voice(&server.url(), &format!("sharer{at}"), invite).await;
                let (voice, hearing) = voice_of(&joined);
                let (sharing, feeder) =
                    voice.share(&link, webrtc::Codec::H264, SIZE.0, SIZE.1).await.expect("sharing starts");
                let envelope = net::Envelope::for_screen(SIZE.0, SIZE.1, FPS, None);
                sharing.set_rates(envelope.min, envelope.start, envelope.max).expect("the share states its rates");
                let (units, stop) = (Arc::clone(&units), Arc::clone(&stop));
                let feeding = tokio::task::spawn_blocking(move || feed(&feeder, &units, &stop));
                sharers.push((link, events, voice, hearing, sharing, feeding));
            }

            let (link, events, booth, me) = registered_as(&server.url(), "watcher", &invites[SCREENS]).await;
            let watcher = common::running::start(link, events, booth, me.clone());
            let cards = |reports: &[Report]| {
                reports.iter().filter(|report| matches!(report, Report::Watch(Watched::Card(_)))).count()
            };
            watcher
                .when(&format!("{SCREENS} screens shown"), Duration::from_secs(60), || {
                    (cards(&watcher.reports.lock().unwrap()) >= SCREENS).then_some(())
                })
                .await;

            let asked = Instant::now();
            let leaving = tokio::spawn(watcher.leave());
            let gone =
                |kind: &event::Kind| matches!(kind, event::Kind::MemberLeft(left) if left.user_id == me.as_str());
            told(&mut sharers[0].1, gone).await;
            let seen = asked.elapsed();
            leaving.await.expect("the call ended by being left");
            let ended = asked.elapsed();
            eprintln!(
                "with {SCREENS} screens watched: the others saw the leave after {seen:?}, all was let go after {ended:?}"
            );
            stop.store(true, Ordering::Relaxed);
            assert!(seen < SEEN_WITHIN, "the others saw the leave after {seen:?}");
            for (_, _, _, _, _, feeding) in sharers {
                feeding.await.expect("the feeder ended");
            }
            drop(server);
        }
    }
}
