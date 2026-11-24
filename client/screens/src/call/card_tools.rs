//! Which controls a call's card has on its bar, and in which order: decided here, drawn by the front end.

use super::*;

/// One control of a card's bar. What it is drawn as and sends is the front end's, which reads the card and the call for
/// the state that flips it (pinned or not, zoomed or not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardTool {
    /// The popover of how loud the card's sound plays here: a person's voice, or a screen's sound.
    Volume,
    /// Stop the picture from arriving until it is resumed.
    Pause,
    /// Start a paused picture again.
    Resume,
    /// Zoom into the screen shown large, or back out.
    Zoom,
    /// The screen or camera plays in a window of its own.
    PictureInPicture,
    /// The screen or camera in a window of its own plays in its card again.
    ClosePictureInPicture,
    /// The screen fills the whole display.
    FullScreen,
    /// Pin the card to the stage, or put it back in the grid when it is.
    Pin,
    /// The popover about the picture: its size, rate and how it is made.
    About,
    /// Stop sending this account's own screen.
    StopSharing,
}

impl Call {
    /// The controls on `card`'s bar, left to right. A card in the strip under a pinned one has the fewer, as a small card
    /// has room for.
    pub fn card_tools(&self, card: &Card<'_>) -> Vec<CardTool> {
        let strip = match (card.pin(), self.pinned()) {
            (Some(pin), Some(pinned)) => pin != *pinned,
            (None, _) | (_, None) => false,
        };
        let pin = card.pin().map(|_| CardTool::Pin);
        let paused = |user_id: &MemberId, video| {
            if self.is_paused(user_id, video) { CardTool::Resume } else { CardTool::Pause }
        };
        match card {
            Card::Screen { user_id, received: Received::Playing(_), .. } => {
                let user_id: &MemberId = user_id;
                if self.is_out(user_id, Video::Screen) {
                    return [CardTool::ClosePictureInPicture].into_iter().chain(pin).chain([CardTool::About]).collect();
                }
                let volume = self.shares_sound(user_id).then_some(CardTool::Volume);
                let pause = (!strip).then(|| paused(user_id, Video::Screen));
                let zoom = (!strip && self.zoomable(user_id)).then_some(CardTool::Zoom);
                volume
                    .into_iter()
                    .chain(pause)
                    .chain(zoom)
                    .chain([CardTool::PictureInPicture, CardTool::FullScreen])
                    .chain(pin)
                    .chain([CardTool::About])
                    .collect()
            }
            Card::OwnScreen(Preview::Shown) => {
                [CardTool::StopSharing].into_iter().chain(pin).chain([CardTool::About]).collect()
            }
            Card::Person(tile) => {
                let Some(user_id) = tile.user_id.as_ref().filter(|_| !tile.own()) else {
                    let about = (tile.own() && self.own_camera_pictured()).then_some(CardTool::About);
                    return pin.into_iter().chain(about).collect();
                };
                if self.is_out(user_id, Video::Camera) {
                    return [CardTool::ClosePictureInPicture].into_iter().chain(pin).collect();
                }
                let volume = (self.live() && self.joined()).then_some(CardTool::Volume);
                let pictured = tile.camera.pictured();
                let pause = (!strip && pictured).then(|| paused(user_id, Video::Camera));
                let out = (!strip && pictured).then_some(CardTool::PictureInPicture);
                volume.into_iter().chain(pause).chain(out).chain(pin).collect()
            }
            // A picture paused before it was watched has none yet, and only the person starts it.
            Card::Screen { user_id, received: Received::Waiting, .. }
                if !strip && self.is_paused(user_id, Video::Screen) =>
            {
                [CardTool::Resume].into_iter().chain(pin).collect()
            }
            Card::OwnScreen(_) | Card::Screen { .. } => pin.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CardTool::{self, *};
    use crate::Pinned;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::harness::{Clock, in_call_offering_a_camera, member, sharer_in_call, tell, the_call, watching_bo};
    use crate::place;
    use proto::rules::Grants;
    use proto::video::Video;
    use std::time::Instant;

    fn bo_screen(window: &crate::window::Window) -> Vec<CardTool> {
        let call = the_call(window);
        let card =
            call.cards().into_iter().find(|card| matches!(card, crate::Card::Screen { .. })).expect("bo's screen");
        call.card_tools(&card)
    }

    #[test]
    fn a_watched_screen_has_its_controls_in_the_agreed_order_and_pause_becomes_resume() {
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let say =
            |window: &mut crate::window::Window, message| tell(window, &clock, place, place::Message::Call(message));
        let tools = bo_screen(&window);
        assert!(!tools.contains(&Zoom), "a screen not shown large is not zoomed");
        let order: Vec<_> = tools.iter().copied().filter(|tool| *tool != Volume).collect();
        assert_eq!(order, [Pause, PictureInPicture, FullScreen, Pin, About]);
        let todo = say(&mut window, place::CallMessage::TogglePause(Pinned::Screen(member("bo"))));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::Call(place::Control::Pause { user_id, video: Video::Screen }))] if *user_id == member("bo")),
            "{todo:?}"
        );
        assert!(bo_screen(&window).contains(&Resume));
        let todo = say(&mut window, place::CallMessage::TogglePause(Pinned::Screen(member("bo"))));
        assert!(matches!(&todo[..], [crate::Ask::At(_, place::Ask::Call(place::Control::Resume { .. }))]), "{todo:?}");
        assert!(bo_screen(&window).contains(&Pause));
    }

    #[test]
    fn a_screen_paused_before_it_was_watched_has_resume_before_any_picture_and_resuming_asks_for_it() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let say =
            |window: &mut crate::window::Window, message| tell(window, &clock, place, place::Message::Call(message));
        assert_eq!(bo_screen(&window), [Pin], "a picture awaited has nothing to pause");
        let began = crate::Watched::Paused { user_id: member("bo"), video: Video::Screen };
        say(&mut window, place::CallMessage::Watched(began));
        assert_eq!(bo_screen(&window), [Resume, Pin], "no picture has come, and the person can start it");
        let todo = say(&mut window, place::CallMessage::TogglePause(Pinned::Screen(member("bo"))));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::Call(place::Control::Resume { video: Video::Screen, .. }))]
            ),
            "{todo:?}"
        );
        assert_eq!(bo_screen(&window), [Pin], "started: the picture is awaited again");
    }

    #[test]
    fn a_pinned_screen_adds_zoom_after_pause_and_the_others_in_the_strip_have_the_fewer() {
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Pin(Pinned::Screen(member("bo")))));
        let order: Vec<_> = bo_screen(&window).into_iter().filter(|tool| *tool != Volume).collect();
        assert_eq!(order, [Pause, Zoom, PictureInPicture, FullScreen, Pin, About]);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Pin(Pinned::Person(member("bo")))));
        let strip: Vec<_> = bo_screen(&window).into_iter().filter(|tool| *tool != Volume).collect();
        assert_eq!(strip, [PictureInPicture, FullScreen, Pin, About], "no pause and no zoom in the strip");
    }

    #[test]
    fn another_s_camera_can_be_paused_and_one_s_own_tile_only_pinned() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        for user_id in ["bo", "ada"] {
            let started = RoomEvent::Started { user_id: member(user_id), track: Track::Camera };
            tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(started)));
        }
        let tools_of = |window: &crate::window::Window, name: &str| {
            let call = the_call(window);
            let card = call
                .cards()
                .into_iter()
                .find(|card| matches!(card, crate::Card::Person(tile) if tile.user_id.as_ref() == Some(&member(name))))
                .expect("a tile");
            call.card_tools(&card)
        };
        assert_eq!(tools_of(&window, "bo"), [Volume, Pause, PictureInPicture, Pin]);
        assert_eq!(tools_of(&window, "ada"), [Pin]);
        let todo = tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::TogglePause(Pinned::Person(member("bo")))),
        );
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::Call(place::Control::Pause { video: Video::Camera, .. }))]
            ),
            "{todo:?}"
        );
        assert_eq!(tools_of(&window, "bo"), [Volume, Resume, PictureInPicture, Pin]);
        let paused = the_call(&window)
            .tiles()
            .iter()
            .find(|tile| tile.user_id.as_ref() == Some(&member("bo")))
            .map(|tile| tile.camera);
        assert_eq!(paused, Some(crate::CameraSeen::Paused));
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::Camera };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(stopped)));
        assert!(!the_call(&window).is_paused(&member("bo"), Video::Camera), "a camera that went off is forgotten");
    }

    #[test]
    fn ones_own_camera_has_about_once_looked_at_and_its_toggle_opens_and_closes_the_popover() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let say =
            |window: &mut crate::window::Window, message| tell(window, &clock, place, place::Message::Call(message));
        say(&mut window, place::CallMessage::ToggleCamera);
        say(
            &mut window,
            place::CallMessage::Camera(crate::CameraSent::Started {
                width: 1280,
                height: 720,
                encoder: crate::Route::Software,
            }),
        );
        let started = RoomEvent::Started { user_id: member("ada"), track: Track::Camera };
        say(&mut window, place::CallMessage::Room(started));
        let own_tools = |window: &crate::window::Window| {
            let call = the_call(window);
            let card = call
                .cards()
                .into_iter()
                .find(|card| matches!(card, crate::Card::Person(tile) if tile.own()))
                .expect("own tile");
            call.card_tools(&card)
        };
        assert_eq!(own_tools(&window), [Pin], "nothing to tell before the first look");
        say(&mut window, place::CallMessage::ToggleOwnCameraInfo);
        assert_eq!(the_call(&window).open(), None);
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
        say(&mut window, place::CallMessage::Camera(crate::CameraSent::Looked(looked)));
        assert_eq!(own_tools(&window), [Pin, About]);
        say(&mut window, place::CallMessage::ToggleOwnCameraInfo);
        assert_eq!(the_call(&window).open(), Some(place::Over::OwnCamera));
        let words = words::Words::english();
        let about = the_call(&window).own_camera_about(&words).expect("a look");
        assert_eq!(about.title, "Your camera");
        say(&mut window, place::CallMessage::ToggleOwnCameraInfo);
        assert_eq!(the_call(&window).open(), None);
        say(&mut window, place::CallMessage::ToggleOwnCameraInfo);
        say(&mut window, place::CallMessage::Camera(crate::CameraSent::Stopped));
        assert_eq!(the_call(&window).open(), None, "a camera that went off takes its popover");
    }

    #[test]
    fn ones_own_screen_stops_pins_and_tells_about_itself_and_a_person_without_a_camera_has_volume_and_pin() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let say =
            |window: &mut crate::window::Window, message| tell(window, &clock, place, place::Message::Call(message));
        say(&mut window, place::CallMessage::Starting);
        say(
            &mut window,
            place::CallMessage::Shared(crate::Shared::Started {
                width: 1920,
                height: 1080,
                encoder: crate::Route::Software,
            }),
        );
        let call = the_call(&window);
        let own = call.cards().into_iter().find(|card| matches!(card, crate::Card::OwnScreen(_))).expect("own card");
        assert_eq!(call.card_tools(&own), [StopSharing, Pin, About]);
        let bo = call
            .cards()
            .into_iter()
            .find(|card| matches!(card, crate::Card::Person(tile) if tile.user_id.as_ref() == Some(&member("bo"))))
            .expect("bo");
        assert_eq!(call.card_tools(&bo), [Volume, Pin]);
    }
}
