//! A camera sent and watched from inside the window's own call: one account turns on a camera of made-up pictures,
//! and they reach the slot the other's call paints its card from, while its own card is given each one; turned off,
//! only the camera ends. No camera is needed: the pictures are the test's own, encoded as a camera's are.

use crate::common;

use call::camera::{Pick, Pictures};
use call::room::{Command, Report};
use capture::camera::{CameraError, Mode};
use common::running::{Running, start};
use common::{Server, registered_as};
use media::Nv12;
use media::camera::Picture;
use proto::video::Video;
use screens::{CameraSent, RoomEvent, Track, Watched};
use std::time::{Duration, Instant};

const MODE: Mode = Mode { width: 640, height: 360, frames_per_second: 15 };

/// A camera of made-up pictures at its rate: a gradient that moves a step each picture.
struct Made {
    made: u32,
    next: Instant,
}

impl Pictures for Made {
    fn mode(&self) -> Mode {
        MODE
    }

    fn read(&mut self) -> Result<(Picture, Instant), CameraError> {
        crate::clock::frame_clock(self.next.saturating_duration_since(Instant::now()));
        self.next += Duration::from_secs(1) / MODE.frames_per_second;
        self.made += 1;
        let (width, height) = (MODE.width as usize, MODE.height as usize);
        let mut data = vec![128_u8; width * height * 3 / 2];
        for (at, luma) in data[..width * height].iter_mut().enumerate() {
            *luma = u8::try_from((at % width + self.made as usize) % 220 + 16).unwrap();
        }
        let nv12 = Nv12::new(width, height, data).unwrap().in_colours(media::camera::ToNv12::COLOURS);
        Ok((Picture::Nv12(nv12), Instant::now()))
    }
}

fn made() -> Pick {
    Pick::Pictures(Box::new(Made { made: 0, next: Instant::now() }))
}

/// Ada and Bea in one call, each as the window runs it.
async fn two_in_a_call() -> (Server, Running, Running, terms::MemberId) {
    let (server, invites) = Server::start("e2e_call_camera", 2).await;
    let (ada_link, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
    let (bea_link, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
    let (ada, bea) =
        (start(ada_link, ada_events, booth.clone(), ada_id.clone()), start(bea_link, bea_events, booth, bea_id));
    ada.until("Ada is in the room", Duration::from_secs(30), |report| {
        matches!(report, Report::Room(RoomEvent::Joined { .. }))
    })
    .await;
    (server, ada, bea, ada_id)
}

mod server {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_camera_reaches_the_other_member_s_slot_and_its_own_card_and_ends_alone() {
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        ada.ask(Command::Camera(Some(made()))).await;
        ada.until("the camera is sent", Duration::from_secs(30), |report| {
            matches!(report, Report::Camera(CameraSent::Started { width: 640, height: 360, .. }))
        })
        .await;
        bea.until("Bea sees Ada's camera", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Started { track: Track::Camera, user_id }) if *user_id == ada_id)
        })
        .await;
        let camera = bea.picture(&ada_id, Video::Camera);
        bea.when("pictures of Ada's camera reach Bea", Duration::from_secs(20), || {
            (camera.stats().received > 0).then_some(())
        })
        .await;
        assert!(camera.stats().received > 0, "pictures of Ada's camera reached Bea: {:?}", camera.stats());
        assert_eq!(bea.screen(&ada_id).stats().received, 0, "and no screen of hers");
        assert!(ada.own_camera.stats().received > 0, "Ada's own card is given her pictures");
        ada.until("Ada's camera is looked at as it is sent", Duration::from_secs(10), |report| {
            matches!(report, Report::Camera(CameraSent::Looked(outgoing))
                if outgoing.frames > 0 && outgoing.bits_per_second > 0 && (outgoing.width, outgoing.height) == (640, 360))
        })
        .await;
        ada.ask(Command::Camera(None)).await;
        ada.until("the camera is off", Duration::from_secs(30), |report| {
            matches!(report, Report::Camera(CameraSent::Stopped))
        })
        .await;
        bea.until("Bea's view of it ends", Duration::from_secs(30), |report| {
            matches!(report, Report::Watch(Watched::Ended { user_id, video: Video::Camera }) if *user_id == ada_id)
        })
        .await;
        let left = ada.reports.lock().unwrap().iter().any(|report| matches!(report, Report::Room(RoomEvent::Ended(_))));
        assert!(!left, "the call goes on");
        ada.leave().await;
        bea.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_camera_turned_off_and_on_again_leaves_the_call_answering() {
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        let looked = |report: &Report| matches!(report, Report::Camera(CameraSent::Looked(_)));
        for round in 0..3 {
            ada.ask(Command::Camera(Some(made()))).await;
            ada.until("the camera is sent", Duration::from_secs(30), |report| {
                matches!(report, Report::Camera(CameraSent::Started { .. }))
            })
            .await;
            ada.until("the camera is looked at", Duration::from_secs(10), looked).await;
            ada.ask(Command::Camera(None)).await;
            ada.until("the camera is off", Duration::from_secs(30), |report| {
                matches!(report, Report::Camera(CameraSent::Stopped))
            })
            .await;
            ada.reports.lock().unwrap().clear();
            let muted = round % 2 == 0;
            ada.wants.send_modify(|wants| wants.mute(muted));
            bea.until("Ada's call still answers", Duration::from_secs(5), |report| {
                matches!(report, Report::Room(RoomEvent::State { user_id, state }) if *user_id == ada_id && state.mic_muted == muted)
            })
            .await;
            bea.reports.lock().unwrap().clear();
        }
        ada.leave().await;
        bea.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_camera_turned_off_while_it_opens_lets_the_call_be_left() {
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        let slow = Made { made: 0, next: Instant::now() + Duration::from_millis(300) };
        ada.ask(Command::Camera(Some(Pick::Pictures(Box::new(slow))))).await;
        ada.ask(Command::Camera(None)).await;
        ada.until("the camera is off", Duration::from_secs(10), |report| {
            matches!(report, Report::Camera(CameraSent::Stopped))
        })
        .await;
        ada.leave().await;
        bea.until(
            "Bea sees Ada leave",
            Duration::from_secs(10),
            |report| matches!(report, Report::Room(RoomEvent::MemberLeft { user_id }) if *user_id == ada_id),
        )
        .await;
        bea.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_replace_turns_on_the_new_pick_after_the_old_is_let_go() {
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        let sent = |report: &Report| matches!(report, Report::Camera(CameraSent::Started { .. }));
        ada.ask(Command::Camera(Some(made()))).await;
        ada.until("the first camera is sent", Duration::from_secs(30), sent).await;
        ada.ask(Command::Camera(Some(made()))).await;
        ada.when("the new pick is sent", Duration::from_secs(30), || {
            (ada.reports.lock().unwrap().iter().filter(|report| sent(report)).count() >= 2).then_some(())
        })
        .await;
        let reports = ada.reports.lock().unwrap().clone();
        assert_eq!(reports.iter().filter(|report| sent(report)).count(), 2, "the new pick is sent: {reports:?}");
        let said = reports.iter().any(|report| matches!(report, Report::Camera(CameraSent::Stopped)));
        assert!(!said, "nothing is said of the one let go");
        let camera = bea.picture(&ada_id, Video::Camera);
        let from = camera.stats().received;
        bea.when("Bea sees the new one", Duration::from_secs(20), || (camera.stats().received > from).then_some(()))
            .await;
        assert!(camera.stats().received > from, "Bea sees the new one: {:?}", camera.stats());
        ada.leave().await;
        bea.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_camera_whose_card_is_hidden_reaches_the_other_member_and_gives_its_own_card_none() {
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        ada.own_camera.show(false);
        ada.ask(Command::Camera(Some(made()))).await;
        let camera = bea.picture(&ada_id, Video::Camera);
        bea.when("pictures of Ada's camera reach Bea", Duration::from_secs(30), || {
            (camera.stats().received > 0).then_some(())
        })
        .await;
        assert!(camera.stats().received > 0, "pictures of Ada's camera reached Bea: {:?}", camera.stats());
        assert_eq!(ada.own_camera.stats().received, 0, "a hidden card is given none");
        ada.own_camera.show(true);
        ada.when("the card is given pictures once shown", Duration::from_secs(10), || {
            (ada.own_camera.stats().received > 0).then_some(())
        })
        .await;
        ada.leave().await;
        bea.leave().await;
        drop(server);
    }
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn this_machine_s_camera_turned_off_and_on_again_unwatched_leaves_the_call_answering() {
        let _turn = tier::turn("camera");
        let Some(device) = capture::camera::list().ok().and_then(|cameras| cameras.into_iter().next()) else {
            return tier::skipped("no camera plugged in");
        };
        let (server, invites) = Server::start("e2e_call_camera_alone", 1).await;
        let (link, events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada = start(link, events, booth, ada_id.clone());
        ada.until("Ada is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;
        // Nobody watches and the window is hidden: a picture is read and dropped, never decoded, converted or shown.
        ada.own_camera.show(false);
        for round in 0..3 {
            let ask = choices::CameraAsk { device: Some(device.id.clone()), ..choices::CameraAsk::default() };
            ada.ask(Command::Camera(Some(Pick::Device(ask)))).await;
            ada.until("the camera is sent", Duration::from_secs(30), |report| {
                matches!(report, Report::Camera(CameraSent::Started { .. }))
            })
            .await;
            ada.until("the camera is looked at", Duration::from_secs(10), |report| {
                matches!(report, Report::Camera(CameraSent::Looked(_)))
            })
            .await;
            assert_eq!(ada.own_camera.stats().received, 0, "round {round}: nothing was converted for a hidden card");
            ada.ask(Command::Camera(None)).await;
            ada.until("the camera is off", Duration::from_secs(30), |report| {
                matches!(report, Report::Camera(CameraSent::Stopped))
            })
            .await;
            ada.reports.lock().unwrap().clear();
            let muted = round % 2 == 0;
            ada.wants.send_modify(|wants| wants.mute(muted));
            ada.until("Ada's call still answers", Duration::from_secs(5), |report| {
                matches!(report, Report::Room(RoomEvent::State { user_id, state }) if *user_id == ada_id && state.mic_muted == muted)
            })
            .await;
        }
        ada.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn this_machine_s_camera_turned_off_while_it_opens_is_off_each_time_and_the_call_is_left() {
        let _turn = tier::turn("camera");
        let Some(device) = capture::camera::list().ok().and_then(|cameras| cameras.into_iter().next()) else {
            return tier::skipped("no camera plugged in");
        };
        let (server, invites) = Server::start("e2e_call_camera_let_go", 1).await;
        let (link, events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada = start(link, events, booth, ada_id);
        ada.until("Ada is in the room", Duration::from_secs(30), |report| {
            matches!(report, Report::Room(RoomEvent::Joined { .. }))
        })
        .await;
        for _ in 0..10 {
            let ask = choices::CameraAsk { device: Some(device.id.clone()), ..choices::CameraAsk::default() };
            ada.ask(Command::Camera(Some(Pick::Device(ask)))).await;
            ada.ask(Command::Camera(None)).await;
            ada.until("the camera is off", Duration::from_secs(30), |report| {
                matches!(report, Report::Camera(CameraSent::Stopped))
            })
            .await;
            ada.reports.lock().unwrap().clear();
        }
        ada.leave().await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn this_machine_s_camera_reaches_the_other_member_at_its_rate() {
        let _turn = tier::turn("camera");
        let Some(device) = capture::camera::list().ok().and_then(|cameras| cameras.into_iter().next()) else {
            return tier::skipped("no camera plugged in");
        };
        let (server, ada, bea, ada_id) = two_in_a_call().await;
        let ask = choices::CameraAsk { device: Some(device.id), ..choices::CameraAsk::default() };
        ada.ask(Command::Camera(Some(Pick::Device(ask)))).await;
        ada.until("the camera is sent", Duration::from_secs(30), |report| {
            matches!(report, Report::Camera(CameraSent::Started { .. }))
        })
        .await;
        let camera = bea.picture(&ada_id, Video::Camera);
        bea.when("the camera's first picture reaches Bea", Duration::from_secs(20), || {
            (camera.stats().received > 0).then_some(())
        })
        .await;
        let (from, counted) = (camera.stats().received, Duration::from_secs(10));
        let counting = Instant::now();
        bea.looks(10, counted * 3).await;
        let rate = (camera.stats().received - from) as f64 / counting.elapsed().as_secs_f64();
        let started = ada.reports.lock().unwrap().iter().find_map(|report| match report {
            Report::Camera(CameraSent::Started { width, height, encoder }) => {
                Some(format!("{width}x{height} by {encoder:?}"))
            }
            _ => None,
        });
        println!("camera sent {}, received at {rate:.1} a second", started.unwrap_or_default());
        assert!(rate > 5.0, "Bea's slot is given the camera's pictures steadily: {rate:.1} a second");
        #[cfg(target_os = "linux")]
        if ada.reports.lock().unwrap().iter().any(|report| {
            matches!(report, Report::Camera(CameraSent::Started { encoder: screens::Route::Hardware { via }, .. })
                if via == "VA-API")
        }) {
            let own = ada.own_camera.take();
            assert!(
                matches!(own, Some(picture::Frame::Surface(_))),
                "a VA-API encoder's device lifts the camera, and Ada's own card is given its surface"
            );
        }
        ada.leave().await;
        bea.leave().await;
        drop(server);
    }
}
