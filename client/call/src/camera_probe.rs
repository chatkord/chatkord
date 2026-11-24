//! The settings page's cameras with no call: listed at the start and whenever one comes or goes, and one shown in a
//! preview on request, its pictures in one's own camera slot. Nothing is encoded or sent; it ends when told to close.

use crate::camera::{Pick, Preview, preview};
use capture::camera::Mode;
use choices::{CameraAsk, CameraResolution};
use picture::Slot;
use screens::{Listed, ListedCamera};
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use words::Failure;

/// What the host asks of the camera probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Show this camera in the preview, in place of any shown there.
    Test(CameraAsk),
    /// Let go of the camera the preview shows.
    EndTest,
    /// Let go of every camera and end.
    Close,
}

/// What the camera probe reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The cameras there are, or why none can be listed.
    Cameras(Result<Vec<ListedCamera>, Failure>),
    /// The preview's camera is open, or why it would not open or stopped.
    Test(Result<(), Failure>),
}

/// The cameras this system offers, each with the resolutions at which it captures 60 a second, read on the blocking
/// pool without streaming.
async fn listed() -> Result<Vec<ListedCamera>, Failure> {
    tokio::task::spawn_blocking(|| {
        crate::camera::listed().map(|cameras| {
            cameras
                .into_iter()
                .map(|camera| {
                    let modes = capture::camera::modes(&camera.id).unwrap_or_default();
                    ListedCamera {
                        sixty_at: sixty_at(&modes),
                        camera: Listed { asked: Some(camera.id), label: camera.label },
                    }
                })
                .collect()
        })
    })
    .await
    .map_err(|_| Failure::CameraUnavailable)?
}

/// The resolutions at which the mode a camera opens with, asked for 60, captures 60 a second: decided by the camera's
/// own [`Mode::nearest`], as an open does.
fn sixty_at(modes: &[Mode]) -> Vec<CameraResolution> {
    CameraResolution::ALL
        .into_iter()
        .filter(|resolution| {
            let (width, height) = resolution.size();
            let asked = Mode { width, height, frames_per_second: 60 };
            Mode::nearest(modes, asked).is_some_and(|opened| opened.frames_per_second >= 60)
        })
        .collect()
}

/// Lists the cameras, again whenever one comes or goes, and shows one on request until told to close or `commands`
/// closes, which lets every camera go.
pub async fn run(own: Slot, mut commands: mpsc::Receiver<Command>, mut report: impl FnMut(Report)) {
    let changed = Arc::new(Notify::new());
    let told = Arc::clone(&changed);
    // A system that does not tell has its cameras listed once.
    let _watch = capture::camera::watch(move || told.notify_one()).ok();
    report(Report::Cameras(listed().await));
    let mut shown: Option<Preview> = None;
    loop {
        tokio::select! {
            asked = commands.recv() => match asked {
                Some(Command::Test(ask)) => {
                    if let Some(before) = shown.take() {
                        before.stop().await;
                    }
                    match preview(Pick::Device(ask), own.clone()).await {
                        Ok(opened) => {
                            shown = Some(opened);
                            report(Report::Test(Ok(())));
                        }
                        Err(failed) => report(Report::Test(Err(failed))),
                    }
                }
                Some(Command::EndTest) => {
                    if let Some(before) = shown.take() {
                        before.stop().await;
                    }
                }
                Some(Command::Close) | None => break,
            },
            () = changed.notified() => report(Report::Cameras(listed().await)),
            failed = async { shown.as_mut()?.ended().await.into() }, if shown.is_some() => {
                if let Some(ended) = shown.take() {
                    ended.stop().await;
                }
                if let Some(failed) = failed {
                    report(Report::Test(Err(failed)));
                }
            }
        }
    }
    if let Some(before) = shown.take() {
        before.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    const WAIT: Duration = Duration::from_secs(10);

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_cameras_are_listed_at_the_start_and_close_ends_it() {
        let (commands, queued) = mpsc::channel(8);
        let (told, mut reports) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(Slot::new(), queued, move |report| drop(told.send(report))));
        let first = timeout(WAIT, reports.recv()).await.expect("listed at the start");
        assert!(matches!(first, Some(Report::Cameras(_))), "{first:?}");
        commands.send(Command::Close).await.expect("the probe is listening");
        timeout(WAIT, task).await.expect("ended when told to close").expect("ended well");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_chosen_camera_not_plugged_in_is_gone_and_none_stands_in() {
        // A system that refuses to list its cameras refuses this camera the same way.
        let refused = capture::camera::list().err().map_or(Failure::CameraGone, crate::failure::camera);
        let (commands, queued) = mpsc::channel(8);
        let (told, mut reports) = mpsc::unbounded_channel();
        let own = Slot::new();
        let task = tokio::spawn(run(own.clone(), queued, move |report| drop(told.send(report))));
        let ask = CameraAsk { device: Some("a camera nobody has".into()), ..CameraAsk::default() };
        commands.send(Command::Test(ask)).await.expect("the probe is listening");
        let answered = timeout(WAIT, async {
            while let Some(report) = reports.recv().await {
                if let Report::Test(answered) = report {
                    return answered;
                }
            }
            Ok(())
        })
        .await
        .expect("the test answered");
        assert_eq!(answered, Err(refused));
        assert_eq!(own.stats().received, 0, "no other camera's pictures");
        drop(commands);
        timeout(WAIT, task).await.expect("ended with its commands").expect("ended well");
    }

    #[test]
    fn sixty_is_offered_where_the_mode_opened_captures_it() {
        let mode = |width, height, frames_per_second| Mode { width, height, frames_per_second };
        let modes = [mode(1920, 1080, 30), mode(1280, 720, 60), mode(1280, 720, 30), mode(640, 480, 30)];
        assert_eq!(sixty_at(&modes), [CameraResolution::P720], "480p opens at 640 by 480, which is 30 alone");
        assert!(sixty_at(&[mode(1920, 1080, 30)]).is_empty());
        assert!(sixty_at(&[]).is_empty(), "a camera whose modes cannot be read offers none");
    }

    mod lab {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_test_puts_the_camera_s_pictures_in_the_slot_and_its_end_empties_it() {
            let _turn = tier::turn("camera");
            let Ok(cameras) = capture::camera::list() else { return tier::skipped("no camera capture on this system") };
            let Some(camera) = cameras.into_iter().next() else { return tier::skipped("no camera plugged in") };
            let (commands, queued) = mpsc::channel(8);
            let (told, mut reports) = mpsc::unbounded_channel();
            let own = Slot::new();
            let task = tokio::spawn(run(own.clone(), queued, move |report| drop(told.send(report))));
            let ask = CameraAsk { device: Some(camera.id), ..CameraAsk::default() };
            commands.send(Command::Test(ask)).await.expect("the probe is listening");
            let opened = timeout(WAIT, async {
                while let Some(report) = reports.recv().await {
                    if let Report::Test(opened) = report {
                        return opened;
                    }
                }
                Err(Failure::CameraGone)
            })
            .await
            .expect("the test answered");
            assert_eq!(opened, Ok(()), "the camera opened");
            let begun = std::time::Instant::now();
            while own.stats().received == 0 && begun.elapsed() < WAIT {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert!(own.stats().received > 0, "its pictures reached the slot");
            commands.send(Command::EndTest).await.expect("the probe is listening");
            commands.send(Command::Close).await.expect("the probe is listening");
            timeout(WAIT, task).await.expect("ended").expect("ended well");
            assert!(own.size().is_none(), "the slot was emptied");
        }
    }
}
