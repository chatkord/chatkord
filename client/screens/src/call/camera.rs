//! One's own camera in a call: what the person asks of it and what the call says it did.

use crate::Route;
use crate::call::health::Outgoing;
use choices::CameraResolution;
use words::Failure;

/// Whether this account can turn on its camera here, and if not why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanCamera {
    /// It can: the camera button is enabled.
    Yes,
    /// Its roles here do not allow a camera: the button is dimmed and says so.
    NotAllowed,
}

impl CanCamera {
    /// Why the button is dimmed, if it is.
    pub fn why_not(self) -> Option<words::Phrase> {
        match self {
            CanCamera::Yes => None,
            CanCamera::NotAllowed => Some(words::Phrase::CantCameraNotAllowed),
        }
    }
}

/// Where one's own camera is, as the call last told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OwnCamera {
    /// Off.
    #[default]
    Off,
    /// Asked for, and opening or being sent.
    Starting,
    /// Sent: how it goes, from the call's last look at it, none until the first.
    On(Option<Outgoing>),
}

impl OwnCamera {
    /// How the camera is going, while it is sent and once the call has looked.
    pub fn outgoing(&self) -> Option<&Outgoing> {
        match self {
            OwnCamera::On(outgoing) => outgoing.as_ref(),
            OwnCamera::Off | OwnCamera::Starting => None,
        }
    }
}

/// A camera this system offers, as the settings page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedCamera {
    /// Its id and its name.
    pub camera: crate::Listed,
    /// The resolutions at which the mode it opens with captures 60 a second.
    pub sixty_at: Vec<CameraResolution>,
}

/// What one's own camera did, as the call tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraSent {
    /// It is sent at `width` by `height`, encoded by `encoder`.
    Started {
        /// Pixels across.
        width: u32,
        /// Pixels down.
        height: u32,
        /// How it is encoded.
        encoder: Route,
    },
    /// How it is going, once a look.
    Looked(Outgoing),
    /// It ended by itself, and why; the call goes on.
    Failed(Failure),
    /// It was turned off.
    Stopped,
    /// The server ended it: this account's roles no longer allow a camera.
    Denied,
}

#[cfg(test)]
mod tests {
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::call::sound::Keep;
    use crate::harness::{Clock, id, in_call_offering_a_camera, member, tell, the_call, two_signed_in};
    use crate::place::{self, Lobby};
    use crate::window::Window;
    use crate::{Ask, WindowMessage};
    use choices::{Picked, Remembered, State};
    use proto::rules::Grants;
    use proto::video::Video;
    use std::collections::BTreeMap;
    use std::time::Instant;
    use words::Phrase;

    #[test]
    fn the_camera_button_turns_the_first_camera_on_at_720p30_and_off_again() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let press =
            |window: &mut Window| tell(window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        assert_eq!(window.open().unwrap().can_camera(), crate::CanCamera::Yes);
        let todo = press(&mut window);
        let first = choices::CameraAsk { device: None, width: 1280, height: 720, frames_per_second: 30 };
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Camera(Some(asked))))] if *asked == first),
            "{todo:?}"
        );
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::Starting);
        let todo = press(&mut window);
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Camera(None)))]), "{todo:?}");
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::Off);
    }

    #[test]
    fn without_its_word_the_camera_is_refused() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_SCREEN"]));
        assert_eq!(window.open().unwrap().can_camera(), crate::CanCamera::NotAllowed);
        assert_eq!(crate::CanCamera::NotAllowed.why_not(), Some(Phrase::CantCameraNotAllowed));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        assert!(todo.is_empty(), "{todo:?}");
    }

    #[test]
    fn a_camera_fills_its_person_s_card_unless_this_device_cannot_show_it() {
        use crate::CameraSeen::{Shown, Unshown};
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let camera_of = |window: &Window, user_id: &str| {
            the_call(window)
                .tiles()
                .iter()
                .find(|tile| tile.user_id.as_ref().map(terms::MemberId::as_str) == Some(user_id))
                .unwrap()
                .camera
        };
        let room = |window: &mut Window, event| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Room(event)));
        };
        room(&mut window, RoomEvent::Started { user_id: member("bo"), track: Track::Camera });
        room(&mut window, RoomEvent::Started { user_id: member("ada"), track: Track::Camera });
        assert_eq!(
            (camera_of(&window, "bo"), camera_of(&window, "ada")),
            (Shown, Shown),
            "one's own card shows one's own"
        );
        let pictured = the_call(&window).cards().iter().filter(|card| card.pictured()).count();
        assert_eq!(pictured, 2, "two cards with a picture");
        let failed = crate::Watched::Failed {
            user_id: member("bo"),
            video: Video::Camera,
            failed: words::Failure::CannotDecode,
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(failed)));
        assert_eq!(camera_of(&window, "bo"), Unshown, "a camera this device can't show leaves the plain tile, marked");
        room(&mut window, RoomEvent::Stopped { user_id: member("bo"), track: Track::Camera });
        let ended = crate::Watched::Ended { user_id: member("bo"), video: Video::Camera };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(ended)));
        room(&mut window, RoomEvent::Started { user_id: member("bo"), track: Track::Camera });
        assert_eq!(camera_of(&window, "bo"), Shown, "a camera started again shows again");
    }

    #[test]
    fn a_camera_told_by_the_server_marks_its_person_in_the_list() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        let person = |user_id: &str, camera: bool| place::Occupant {
            user_id: member(user_id),
            colour: None,
            name: user_id.into(),
            state: State::default(),
            sharing: false,
            camera,
            held: false,
            place: proto::rules::Place::BOTTOM,
        };
        let people = vec![person("grace", true), person("bo", false)];
        let occupancy = place::LobbyMessage::Occupancy { channel_id: crate::harness::channel_id("lounge"), people };
        tell(&mut window, &clock, owls, place::Message::Lobby(occupancy));
        let place = window.open().unwrap();
        let Some(Lobby::Ready(listed)) = place.lobby() else { panic!("listed") };
        let marks: Vec<_> = place.seats(&listed.voice[0]).into_iter().map(|seat| (seat.name, seat.camera)).collect();
        assert_eq!(
            marks,
            [("grace".to_string(), crate::CameraSeen::Unshown), ("bo".to_string(), crate::CameraSeen::Off)],
            "marked, and no card here to fill"
        );
    }

    #[test]
    fn a_camera_in_the_call_marks_the_row_whether_shown_or_not() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let row = |window: &Window| {
            let place = window.open().unwrap();
            let Some(Lobby::Ready(listed)) = place.lobby() else { panic!("listed") };
            place.seats(&listed.voice[0]).into_iter().find(|seat| seat.user_id == Some(member("bo"))).unwrap().camera
        };
        let started = RoomEvent::Started { user_id: member("bo"), track: Track::Camera };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(started)));
        assert_eq!(row(&window), crate::CameraSeen::Shown);
        let failed = crate::Watched::Failed {
            user_id: member("bo"),
            video: Video::Camera,
            failed: words::Failure::CannotDecode,
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(failed)));
        assert_eq!(row(&window), crate::CameraSeen::Unshown, "still marked, though not shown");
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::Camera };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(stopped)));
        assert_eq!(row(&window), crate::CameraSeen::Off);
    }

    #[test]
    fn a_stopped_camera_is_marked_on_its_control_and_toasted_with_a_way_to_turn_it_on_again() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        let fault = |window: &Window| the_call(window).fault(crate::Device::Camera).map(words::Sentence::in_english);
        call(&mut window, place::CallMessage::ToggleCamera);
        call(&mut window, place::CallMessage::Camera(crate::CameraSent::Failed(words::Failure::CameraGone)));
        assert_eq!(fault(&window).as_deref(), Some("The camera was disconnected."), "its control says why");
        let toast = window.toast().unwrap();
        assert_eq!(
            (toast.kind(), toast.said().in_english()),
            (design::Status::Danger, "The camera was disconnected.".into())
        );
        assert_eq!(toast.action(), Some(&crate::Action::TurnOnCamera));
        assert_eq!(crate::Action::TurnOnCamera.said().in_english(), "Turn on camera");
        let todo = window.update(WindowMessage::ToastAct, clock.at());
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Camera(Some(_))))]), "{todo:?}");
        assert_eq!(fault(&window), None, "pressed: the mark goes");
        assert!(window.toast().is_none(), "and the toast with it");
        call(&mut window, place::CallMessage::Camera(crate::CameraSent::Denied));
        assert!(fault(&window).is_some());
        assert_eq!(window.toast().unwrap().action(), None, "a camera not allowed offers nothing");
        let started = crate::CameraSent::Started { width: 1280, height: 720, encoder: crate::Route::Software };
        call(&mut window, place::CallMessage::ToggleCamera);
        call(&mut window, place::CallMessage::Camera(started));
        assert_eq!(fault(&window), None, "working again: the mark goes");
    }

    #[test]
    fn the_kept_ask_is_what_the_camera_button_sends() {
        let clock = Clock(Instant::now());
        let (window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let kept = choices::CameraSettings {
            device: Some(Picked { id: "usb".into(), label: "USB camera".into() }),
            resolution: choices::CameraResolution::P1080,
            rate: choices::CameraRate::Fifteen,
            mirror: choices::Mirror::Off,
        };
        let remembered = Remembered { camera: kept, ..Remembered::default() };
        let mut window = window.remembering(remembered, BTreeMap::new());
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        let asked = choices::CameraAsk { device: Some("usb".into()), width: 1920, height: 1080, frames_per_second: 15 };
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Camera(Some(sent))))] if *sent == asked),
            "{todo:?}"
        );
        assert_eq!(window.open().unwrap().mirror(), choices::Mirror::Off, "the card is shown as kept");
    }

    #[test]
    fn a_choice_made_while_on_reopens_the_camera_with_it_and_says_nothing() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::ToggleCamera);
        let started = crate::CameraSent::Started {
            width: 1280,
            height: 720,
            encoder: crate::Route::Hardware { via: "AMF".into() },
        };
        call(&mut window, place::CallMessage::Camera(started));
        let todo = window.update(WindowMessage::PickCameraResolution(choices::CameraResolution::P1080), clock.at());
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Camera(kept)), Ask::At(_, place::Ask::Call(place::Control::Camera(Some(asked))))]
                if kept.resolution == choices::CameraResolution::P1080 && asked.width == 1920),
            "{todo:?}"
        );
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::Starting);
        assert!(window.toast().is_none(), "nothing said");
        let todo = window.update(WindowMessage::ToggleMirror, clock.at());
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Camera(kept))] if kept.mirror == choices::Mirror::Off),
            "{todo:?}"
        );
    }

    #[test]
    fn a_camera_that_stops_by_itself_or_is_denied_says_so_and_is_off() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        let said = |window: &mut Window, sent| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Camera(sent)));
            the_call(window).fault(crate::Device::Camera).is_some()
        };
        let started = crate::CameraSent::Started { width: 1280, height: 720, encoder: crate::Route::Software };
        assert!(!said(&mut window, started));
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::On(None));
        assert!(said(&mut window, crate::CameraSent::Failed(words::Failure::CameraGone)), "unplugged: said once");
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::Off);
        assert!(said(&mut window, crate::CameraSent::Denied));
    }

    #[test]
    fn a_camera_s_look_is_kept_while_it_is_on_and_one_after_it_stopped_is_not() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let call = |window: &mut Window, sent| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Camera(sent)));
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        let looked = crate::Outgoing {
            codec: "H.264".into(),
            encoder: crate::Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            frame_rate: Some(30),
            bits_per_second: 1_500_000,
            frames: 300,
            key_frames_made: 1,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited: false,
            reduced: None,
        };
        call(&mut window, crate::CameraSent::Looked(looked.clone()));
        assert_eq!(window.open().unwrap().own_camera().outgoing(), None, "a look before it is sent says nothing");
        call(&mut window, crate::CameraSent::Started { width: 1280, height: 720, encoder: crate::Route::Software });
        call(&mut window, crate::CameraSent::Looked(looked.clone()));
        assert_eq!(window.open().unwrap().own_camera().outgoing(), Some(&looked));
        call(&mut window, crate::CameraSent::Stopped);
        call(&mut window, crate::CameraSent::Looked(looked));
        assert_eq!(*window.open().unwrap().own_camera(), crate::OwnCamera::Off, "a late look turns nothing on");
    }

    #[test]
    fn the_popover_shows_one_s_own_video_alone_or_a_tab_each_and_keeps_the_tab_picked_for_the_call() {
        use crate::{OwnVideo, OwnWatch, Shared};
        let clock = Clock(Instant::now());
        let grants = Grants::from_words(&["SHARE_SCREEN", "SHARE_CAMERA"]);
        let (mut window, place) = in_call_offering_a_camera(&clock, grants);
        let call = |window: &mut Window, message| {
            tell(window, &clock, place, place::Message::Call(message));
        };
        let looked = |encoder: crate::Route, width: u32| crate::Outgoing {
            codec: "H.264".into(),
            encoder,
            width,
            height: width * 9 / 16,
            scaled_from: None,
            frame_rate: Some(30),
            bits_per_second: 1_000_000,
            frames: 30,
            key_frames_made: 1,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited: false,
            reduced: None,
        };
        let (share, camera) = (looked(crate::Route::Software, 1920), looked(crate::Route::Software, 1280));
        assert_eq!(the_call(&window).own_video(), None, "nothing sent");
        call(&mut window, place::CallMessage::Starting);
        call(
            &mut window,
            place::CallMessage::Shared(Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software }),
        );
        call(&mut window, place::CallMessage::Shared(Shared::Looked(share.clone())));
        assert_eq!(the_call(&window).own_video(), Some(OwnVideo::Alone(Video::Screen, &share, OwnWatch::Watched)));
        call(&mut window, place::CallMessage::ToggleCamera);
        call(
            &mut window,
            place::CallMessage::Camera(crate::CameraSent::Started {
                width: 1280,
                height: 720,
                encoder: crate::Route::Software,
            }),
        );
        call(&mut window, place::CallMessage::Camera(crate::CameraSent::Looked(camera.clone())));
        assert_eq!(
            the_call(&window).own_video(),
            Some(OwnVideo::Tabbed { shown: Video::Screen, outgoing: &share, watch: OwnWatch::Watched }),
            "the share's tab first"
        );
        call(&mut window, place::CallMessage::ShowOwn(Video::Camera));
        assert_eq!(
            the_call(&window).own_video(),
            Some(OwnVideo::Tabbed { shown: Video::Camera, outgoing: &camera, watch: OwnWatch::Watched })
        );
        call(&mut window, place::CallMessage::Stats);
        call(&mut window, place::CallMessage::CloseOver);
        call(&mut window, place::CallMessage::Stats);
        assert_eq!(
            the_call(&window).own_video(),
            Some(OwnVideo::Tabbed { shown: Video::Camera, outgoing: &camera, watch: OwnWatch::Watched }),
            "kept for the call"
        );
        call(&mut window, place::CallMessage::ToggleShare);
        assert_eq!(
            the_call(&window).own_video(),
            Some(OwnVideo::Alone(Video::Camera, &camera, OwnWatch::Watched)),
            "the share stopped: the camera alone"
        );
    }
}
