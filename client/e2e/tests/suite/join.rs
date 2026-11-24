//! How long joining a voice channel takes once the engine is up: a join borrows the engine and never builds one.

use crate::common;

use common::{Server, engine, registered};
use proto::chatkord_v1::{VoiceJoin, VoiceLeave};
use std::time::{Duration, Instant};
use webrtc::Voice;

/// Between what a join on a shared engine costs (a few milliseconds) and what building a factory costs (over 150):
/// a join that builds one again fails it.
const BOUND: Duration = Duration::from_millis(100);

mod server {
    use super::*;

    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_join_on_a_built_engine_prepares_its_voice_without_building_another_factory() {
            let (server, invites) = Server::start("join", 1).await;
            let (link, _events, booth) = registered(&server.url(), "ada", &invites[0]).await;
            let _warm = engine();
            for round in 1..=3 {
                let joined = Instant::now();
                let Ok(state) = link.ask(VoiceJoin { channel_id: booth.clone(), ..Default::default() }).await else {
                    panic!("no state")
                };
                let asked = joined.elapsed();
                let made = Instant::now();
                let (mut voice, _hearing) = Voice::new(
                    engine(),
                    &state,
                    webrtc::Located::of(&state, &webrtc::Resolved::new(), webrtc::Scope::Loopback).expect("addresses"),
                )
                .expect("both connections");
                let prepared = made.elapsed();
                let played = Instant::now();
                let _ = voice.play();
                eprintln!(
                    "join {round}: ask {asked:?}, voice {prepared:?}, play {:?}, all {:?}",
                    played.elapsed(),
                    joined.elapsed()
                );
                assert!(
                    prepared < BOUND,
                    "round {round}: the voice took {prepared:?}, as if the engine were built again"
                );
                assert!(asked < BOUND, "round {round}: the server's join took {asked:?}");
                drop(voice);
                link.ask(VoiceLeave {}).await.expect("left");
            }
        }

        /// What the window waits for: the loop it runs, from the call to its first report of the room.
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn the_window_s_loop_reports_the_room_within_a_tenth_of_a_second_of_a_built_engine() {
            use call::room::{Leaving, Report, run};
            use screens::RoomEvent;
            use std::sync::{Arc, Mutex};
            let (server, invites) = Server::start("join_loop", 1).await;
            let (link, mut events, booth) = registered(&server.url(), "ada", &invites[0]).await;
            let _warm = engine();
            let first: Arc<Mutex<Option<Duration>>> = Arc::default();
            let started = Instant::now();
            let into = Arc::clone(&first);
            let (_commands, queued) = tokio::sync::mpsc::channel(8);
            let leaving = Leaving::default();
            let left = leaving.clone();
            let listening = run(
                &link,
                engine(),
                &mut events,
                common::listening(booth, Duration::from_millis(100)),
                (tokio::sync::watch::channel(call::room::Wants::default()).1, queued),
                leaving,
                move |report| {
                    if let Report::Room(RoomEvent::Joined { .. }) = report {
                        *into.lock().unwrap() = Some(started.elapsed());
                        left.leave();
                    }
                },
            )
            .await;
            listening.expect("left by the command");
            let reported = first.lock().unwrap().expect("the room was reported");
            eprintln!("the room was reported after {reported:?}");
            assert!(reported < BOUND, "the room was reported after {reported:?}");
        }
    }
}
