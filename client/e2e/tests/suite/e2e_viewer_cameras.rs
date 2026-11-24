//! A viewer of 27 cameras: 27 members each send a 1280 x 720 camera at 30 pictures a second and one account's call
//! takes every one, as the window runs it. Once every camera is taken, over a measured stretch: each stream's pictures
//! decoded and pictures dropped (replaced before a window painting at 60 Hz took them), the decoder's route where the
//! call tells one, and the CPU time the viewing process used. A measurement, run on request.
//!
//! The 27 cameras are one encoder's units pushed to 27 senders, so the sending side costs one encode: the viewer's
//! decoders are what is measured.

use crate::common;
use crate::screen_support::candidates_of;

use call::room::Report;
use common::members::{Camera, Stops, joined, send};
use common::running::start;
use common::{Server, registered_as};
use proto::video::Video;
use screens::Watched;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use terms::MemberId;
use tokio::sync::watch;

const CAMERAS: usize = 27;
const CAMERA: Camera = Camera { width: 1280, height: 720, frames_per_second: 30 };
const WITHIN: Duration = Duration::from_secs(120);
/// How long the viewer is measured once it has every camera.
const MEASURED: Duration = Duration::from_secs(20);
/// How often a window painting at 60 Hz takes the newest picture of each stream.
const REFRESH: Duration = Duration::from_millis(16);
/// The kernel counts a process's CPU time in ticks of this fraction of a second (`USER_HZ`, 100 on every Linux).
const TICKS_A_SECOND: u64 = 100;

/// The CPU time a process has used, user and system together, from the text of its `/proc/<pid>/stat`.
fn cpu_time(stat: &str) -> Option<Duration> {
    // The command's name is in parentheses and may hold spaces, so the fields are counted from after its last one: the
    // state is the third field, and the user and system times are the fourteenth and fifteenth.
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    let user: u64 = fields.nth(11)?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    Some(Duration::from_millis((user + system) * 1000 / TICKS_A_SECOND))
}

/// The CPU time this process has used so far.
fn used() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("the kernel's account of this process");
    cpu_time(&stat).expect("a stat in the kernel's shape")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cpu_time_is_read_from_after_the_command_s_closing_parenthesis() {
        let stat = "4242 (e2e (a b)) R 1 4242 4242 0 -1 4194560 1000 0 0 0 250 50 0 0 20 0 9 0 100 1000000 500 0";
        assert_eq!(cpu_time(stat), Some(Duration::from_secs(3)), "250 and 50 ticks of a hundredth");
        assert_eq!(cpu_time("4242 (e2e) R 1 2"), None, "a line cut short");
        assert_eq!(cpu_time("4242 (e2e) R 1 4242 4242 0 -1 0 0 0 0 0 many 50"), None, "a count that is not one");
        assert_eq!(cpu_time(""), None);
    }
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[ignore = "a measurement, run on request"]
    async fn what_a_viewer_of_27_cameras_decodes_drops_and_costs() {
        let Some((encoder, canvas)) = candidates_of(media::Codec::H264).into_iter().next() else {
            return eprintln!("no hardware H.264 encoder opens on this machine: no camera can be sent");
        };
        let (server, invites) =
            Server::start_with("e2e_viewer_cameras", CAMERAS + 1, &[(testserver::Limit::MaxPerAddress, 64)]).await;

        let mut members = Vec::new();
        let mut feeders = Vec::new();
        for (n, invite) in invites[..CAMERAS].iter().enumerate() {
            let (member, feeder) = joined(&server.url(), &format!("m{n}"), invite, Some(CAMERA), false).await;
            members.push(member);
            feeders.extend(feeder);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let _stops = Stops(Arc::clone(&stop));
        let stopped = Arc::clone(&stop);
        let (fed, mut shared) = watch::channel(0);
        let feeding = tokio::task::spawn_blocking(move || send(&encoder, &canvas, CAMERA, &feeders, &stopped, &fed));
        // The cameras are two seconds old when the viewer comes.
        let old = 2 * CAMERA.frames_per_second as usize;
        let running = shared.wait_for(|fed| *fed >= old);
        tokio::time::timeout(common::WAIT, running).await.expect("the cameras run").expect("fed");

        let (link, events, booth, id) = registered_as(&server.url(), "viewer", &invites[CAMERAS]).await;
        let viewer = start(link, events, booth, id);
        let senders: Vec<MemberId> =
            members.iter().map(|member| MemberId::parse(&member.id).expect("the server's own id")).collect();
        let slots: Vec<call::Slot> = senders.iter().map(|sender| viewer.picture(sender, Video::Camera)).collect();
        viewer
            .when("every camera is taken", WITHIN, || slots.iter().all(|slot| slot.stats().received > 0).then_some(()))
            .await;

        let before: Vec<_> = slots.iter().map(call::Slot::stats).collect();
        let (began, cpu_began) = (Instant::now(), used());
        // As the window paints: the newest picture of each stream taken at every refresh.
        let mut looks = crate::common::looks::Looks::every(REFRESH);
        while began.elapsed() < MEASURED {
            looks.next().await;
            for slot in &slots {
                drop(slot.take());
            }
        }
        let (measured, cpu) = (began.elapsed(), used().saturating_sub(cpu_began));
        let after: Vec<_> = slots.iter().map(call::Slot::stats).collect();

        let seconds = measured.as_secs_f64();
        let mut silent = Vec::new();
        for (n, ((before, after), sender)) in before.iter().zip(&after).zip(&senders).enumerate() {
            let (decoded, dropped) = (after.received - before.received, after.dropped - before.dropped);
            eprintln!(
                "camera {n:2} ({}): {decoded} pictures decoded ({:.1} a second), {dropped} dropped",
                sender.as_str(),
                decoded as f64 / seconds
            );
            if after.received == 0 {
                silent.push(n);
            }
        }
        let cameras_seen = |report: &Report| match report {
            Report::Watch(
                Watched::HandedOver { video: Video::Camera, .. } | Watched::Failed { video: Video::Camera, .. },
            ) => Some(format!("{report:?}")),
            _ => None,
        };
        let changed: Vec<String> = viewer.reports.lock().unwrap().iter().filter_map(cameras_seen).collect();
        eprintln!(
            "decode path: the call tells a camera's route only when it changes (a card is a screen's); {} changed: {changed:?}",
            changed.len()
        );
        eprintln!(
            "the viewer used {:.1} s of CPU time in {seconds:.1} s ({:.0} % of one core)",
            cpu.as_secs_f64(),
            100.0 * cpu.as_secs_f64() / seconds
        );
        assert!(silent.is_empty(), "every stream decoded at least one picture; none from cameras {silent:?}");

        stop.store(true, Ordering::Relaxed);
        feeding.await.expect("the cameras' encoder ends");
        viewer.leave().await;
        drop(members);
        drop(server);
    }
}
