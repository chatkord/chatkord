//! How long a member who joins a full room waits for its voices: 27 members each send a tone, four of them a camera
//! begun before it, and a 28th joins as the window does. From the room arriving whole to the first voice heard and to
//! the last of the 27, and when each take landed. A measurement, run on request; its numbers go in the commit that
//! changes the order streams are taken in.

use crate::common;
use crate::screen_support;

use call::room::Report;
use common::members::{Camera, Stops, joined, send};
use common::running::start;
use common::{Server, registered_as};
use screen_support::candidates_of;
use screens::{RoomEvent, Track};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::watch;

const SPEAKERS: usize = 27;
const CAMERAS: usize = 4;
const CAMERA: Camera = Camera { width: 640, height: 360, frames_per_second: 15 };
const WITHIN: Duration = Duration::from_secs(120);
/// The level a tone is heard above, as the other tests read it.
const QUIET: u8 = 5;

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[ignore = "a measurement, run on request"]
    async fn how_long_a_member_joining_a_full_room_waits_for_its_voices() {
        let source = candidates_of(media::Codec::H264).into_iter().next();
        if source.is_none() {
            eprintln!("no hardware H.264 encoder opens on this machine: voices only, no camera listed");
        }
        let (server, invites) =
            Server::start_with("e2e_join_voices", SPEAKERS + 1, &[(testserver::Limit::MaxPerAddress, 64)]).await;

        let mut members = Vec::new();
        let mut feeders = Vec::new();
        for (n, invite) in invites[..SPEAKERS].iter().enumerate() {
            let pictured = source.is_some() && n < CAMERAS;
            let (member, feeder) =
                joined(&server.url(), &format!("m{n}"), invite, pictured.then_some(CAMERA), true).await;
            members.push(member);
            feeders.extend(feeder);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let _stops = Stops(Arc::clone(&stop));
        let stopped = Arc::clone(&stop);
        let (fed, mut shared) = watch::channel(0);
        let feeding = source.map(|(encoder, canvas)| {
            tokio::task::spawn_blocking(move || send(&encoder, &canvas, CAMERA, &feeders, &stopped, &fed))
        });
        if feeding.is_some() {
            // The cameras are two seconds old when the joiner comes.
            let old = 2 * CAMERA.frames_per_second as usize;
            let running = shared.wait_for(|fed| *fed >= old);
            tokio::time::timeout(common::WAIT, running).await.expect("the cameras run").expect("fed");
        }

        let speakers: BTreeSet<&str> = members.iter().map(|member| member.id.as_str()).collect();
        let (link, events, booth, id) = registered_as(&server.url(), "joiner", &invites[SPEAKERS]).await;
        let joiner = start(link, events, booth, id);

        let mut read = 0;
        let mut arrived: Option<Instant> = None;
        let mut heard: Vec<(&str, Duration)> = Vec::new();
        let mut takes = Vec::new();
        let mut odd = Vec::new();
        let begun = Instant::now();
        while heard.len() < SPEAKERS && begun.elapsed() < WITHIN {
            let now = Instant::now();
            {
                let reports = joiner.reports.lock().unwrap();
                for report in &reports[read..] {
                    let since = arrived.map(|at| now.duration_since(at));
                    match report {
                        Report::Room(RoomEvent::Joined { streams, .. }) if arrived.is_none() => {
                            arrived = Some(now);
                            let pictures: Vec<usize> = streams
                                .iter()
                                .enumerate()
                                .filter(|(_, (_, track))| *track == Track::Camera)
                                .map(|(at, _)| at)
                                .collect();
                            eprintln!("the room lists {} streams, cameras at {pictures:?}", streams.len());
                        }
                        Report::Room(RoomEvent::Levels(levels)) => {
                            for (user_id, level) in levels {
                                let known = speakers.get(user_id.as_str()).copied();
                                if let (Some(known), Some(since)) = (known, since)
                                    && *level > QUIET
                                    && heard.iter().all(|(id, _)| *id != known)
                                {
                                    heard.push((known, since));
                                }
                            }
                        }
                        Report::Said(line) if line.starts_with("Hearing ") => {
                            takes.push(since.map_or(0, |since| since.as_millis()));
                        }
                        Report::Said(line) if line.contains("not heard") => odd.push(line.clone()),
                        Report::Watch(screens::Watched::Failed { .. }) | Report::Notice(..) => {
                            odd.push(format!("{report:?}"));
                        }
                        _ => {}
                    }
                }
                read = reports.len();
            }
            joiner.next(WITHIN.saturating_sub(begun.elapsed())).await;
        }
        if !odd.is_empty() {
            eprintln!("refused or failed: {odd:#?}");
        }
        assert_eq!(heard.len(), SPEAKERS, "every voice is heard within {WITHIN:?}; takes landed at {takes:?}");
        let (first, last) = (heard[0].1.as_millis(), heard[SPEAKERS - 1].1.as_millis());
        eprintln!("joined -> first voice {first} ms, last of {SPEAKERS} voices {last} ms, takes: {takes:?}");

        stop.store(true, Ordering::Relaxed);
        if let Some(feeding) = feeding {
            feeding.await.expect("the cameras' encoder ends");
        }
        joiner.leave().await;
        drop(members);
        drop(server);
    }
}
