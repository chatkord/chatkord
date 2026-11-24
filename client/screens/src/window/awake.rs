//! What this device is kept awake for: the call's pictures and its voice place.

use super::*;

/// How much of the device a call keeps awake, as the window last has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Awake {
    /// Nothing: the device sleeps and dims as its owner set it.
    #[default]
    Free,
    /// The machine stays up, as a voice place needs, and its display may dim.
    System,
    /// The display stays on too, for a picture sent or watched.
    Display,
}

impl Window {
    /// What the calls here keep awake: the display while this account sends a screen or camera or plays a picture it
    /// did not hold, the machine while it holds a voice place, else nothing.
    pub fn awake(&self) -> Awake {
        let calls = || self.places.iter().filter_map(Place::call).filter(|call| call.live());
        if calls().any(place::Call::shows_pictures) {
            Awake::Display
        } else if calls().next().is_some() {
            Awake::System
        } else {
            Awake::Free
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, ada_in_call, sharer_in_call, tell, watching_bo};
    use crate::{Shared, Watched};

    fn call(window: &mut Window, clock: &Clock, place: PlaceId, message: place::CallMessage) {
        tell(window, clock, place, place::Message::Call(message));
    }

    #[test]
    fn a_window_in_no_call_keeps_nothing_awake() {
        let clock = Clock(Instant::now());
        let (window, _) = Window::launch(Vec::new(), None, clock.0);
        assert_eq!(window.awake(), Awake::Free);
    }

    #[test]
    fn a_voice_place_keeps_the_machine_awake_and_leaving_it_frees_it() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        assert_eq!(window.awake(), Awake::System);
        tell(&mut window, &clock, place, crate::harness::moved());
        assert_eq!(window.awake(), Awake::Free, "a call that ended holds no place");
    }

    #[test]
    fn sending_a_screen_keeps_the_display_awake_from_the_moment_it_starts() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        assert_eq!(window.awake(), Awake::System);
        call(&mut window, &clock, place, place::CallMessage::Starting);
        assert_eq!(window.awake(), Awake::Display);
        let started = Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software };
        call(&mut window, &clock, place, place::CallMessage::Shared(started));
        assert_eq!(window.awake(), Awake::Display);
        call(&mut window, &clock, place, place::CallMessage::Shared(Shared::Stopped));
        assert_eq!(window.awake(), Awake::System, "the share ended: only the voice place is left");
    }

    #[test]
    fn sending_a_camera_keeps_the_display_awake() {
        let clock = Clock(Instant::now());
        let grants = proto::rules::Grants::from_words(&["SHARE_CAMERA"]);
        let (mut window, place) = crate::harness::in_call_offering_a_camera(&clock, grants);
        call(&mut window, &clock, place, place::CallMessage::ToggleCamera);
        assert_eq!(window.awake(), Awake::Display);
        call(&mut window, &clock, place, place::CallMessage::ToggleCamera);
        assert_eq!(window.awake(), Awake::System);
    }

    #[test]
    fn watching_a_screen_keeps_the_display_awake_until_it_is_held_or_ends() {
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        assert_eq!(window.awake(), Awake::Display);
        let pin = crate::Pinned::Screen(crate::harness::member("bo"));
        call(&mut window, &clock, place, place::CallMessage::TogglePause(pin.clone()));
        assert_eq!(window.awake(), Awake::System, "a held picture shows nothing new");
        call(&mut window, &clock, place, place::CallMessage::TogglePause(pin));
        assert_eq!(window.awake(), Awake::Display);
        let ended = Watched::Ended { user_id: crate::harness::member("bo"), video: proto::video::Video::Screen };
        call(&mut window, &clock, place, place::CallMessage::Watched(ended));
        assert_eq!(window.awake(), Awake::System);
    }
}
