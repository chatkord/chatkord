//! What the window holds for the errands to fill: the call's picture slots, each woken in the window that paints it,
//! the servers' and the people's pictures and our own picker's stills as image handles, and whether the window closes or hides.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use proto::video::Video;
use shell::window::{Closing, Showing};
use terms::MemberId;

use crate::pictures::Pictures;
use crate::stills::Stills;

/// The slot of each picture sent in the call, made the first time the call puts a picture of it, woken to paint in
/// the window that draws it.
#[derive(Clone)]
pub(crate) struct Screens {
    pub(crate) slots: Arc<Mutex<HashMap<(MemberId, Video), call::Slot>>>,
    pub(crate) wake: shell::window::Wake,
    pub(crate) wake_out: shell::window::Wake,
    /// Whose picture, a screen or a camera, is in its own window, as the window last said after an update; its
    /// pictures wake that window alone, so the main one is not drawn again for a picture it does not show.
    pub(crate) out: Arc<Mutex<Option<(MemberId, Video)>>>,
}

impl Screens {
    pub(crate) fn slot(&self, user_id: &MemberId, video: Video) -> Option<call::Slot> {
        self.slots.lock().ok().and_then(|slots| slots.get(&(user_id.clone(), video)).cloned())
    }

    /// The slot of `user_id`'s picture of `video`, made now if it has none yet.
    fn make(&self, user_id: &MemberId, video: Video) -> call::Slot {
        let made = || {
            let slot = call::Slot::new();
            let (wake, wake_out, out) = (Arc::clone(&self.wake), Arc::clone(&self.wake_out), Arc::clone(&self.out));
            let whose = user_id.clone();
            slot.wake_with(move || {
                let elsewhere = out
                    .lock()
                    .is_ok_and(|out| out.as_ref().is_some_and(|(who, shown)| *who == whose && *shown == video));
                if elsewhere { wake_out() } else { wake() }
            });
            slot
        };
        match self.slots.lock() {
            Ok(mut slots) => slots.entry((user_id.clone(), video)).or_insert_with(made).clone(),
            // Nobody paints it, but the call still has somewhere to put it.
            Err(_) => made(),
        }
    }

    /// Whose picture, and which, is now in its own window.
    pub(crate) fn follow_out(&self, shown: Option<(&MemberId, Video)>) {
        let shown = shown.map(|(user_id, video)| (user_id.clone(), video));
        if let Ok(mut out) = self.out.lock()
            && *out != shown
        {
            *out = shown;
        }
    }

    /// The call is over: its screens' slots go with it.
    fn clear(&self) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.clear();
        }
    }
}

/// The window's side of the errands.
pub(crate) struct Held {
    screens: Screens,
    /// This account's own screen.
    own: call::Slot,
    /// This account's own camera, while it is on.
    own_camera: call::Slot,
    /// The GPU the window paints with, which a screen's decoder decodes onto; replaced when the device is made again.
    painting: Mutex<picture::Painting>,
    /// The pictures held by their file for the servers that asked: the servers', people's, emoji and thumbnails.
    pub(crate) pictures: Pictures,
    /// Our own picker's pictures.
    pub(crate) stills: Stills,
    /// Whether the window is to close: the window said quit.
    closing: Mutex<Closing>,
    /// Whether the window is shown or the scene kept in the tray, as the window last said.
    showing: Mutex<Showing>,
    /// Asks the loop to bring the window forward, with the desktop's token where it gave one.
    forward: Forward,
}

/// What brings the window forward: a send to the event loop, from any thread.
pub(crate) type Forward = Box<dyn Fn(Option<platform::activation::Activation>) + Send + Sync>;

impl Held {
    pub(crate) fn new(
        screens: Screens,
        own: call::Slot,
        own_camera: call::Slot,
        painting: picture::Painting,
        forward: Forward,
    ) -> Held {
        Held {
            screens,
            own,
            own_camera,
            painting: Mutex::new(painting),
            pictures: Pictures::default(),
            stills: Stills::default(),
            closing: Mutex::new(Closing::Stay),
            showing: Mutex::new(Showing::Window),
            forward,
        }
    }

    /// The slot this account's own screen is previewed in.
    pub(crate) fn own(&self) -> &call::Slot {
        &self.own
    }

    pub(crate) fn closing(&self) -> Closing {
        self.closing.lock().map(|closing| *closing).unwrap_or_default()
    }

    pub(crate) fn showing(&self) -> Showing {
        self.showing.lock().map(|showing| *showing).unwrap_or_default()
    }

    /// The GPU the window paints with was made again: decoders made from now on decode onto it.
    pub(crate) fn repaint(&self, painting: picture::Painting) {
        if let Ok(mut held) = self.painting.lock() {
            *held = painting;
        }
    }

    fn set_showing(&self, now: Showing) {
        if let Ok(mut showing) = self.showing.lock() {
            *showing = now;
        }
    }
}

impl errands::Host for Held {
    fn slot(&self, user_id: &MemberId, video: Video) -> call::Slot {
        self.screens.make(user_id, video)
    }

    fn clear_slots(&self) {
        self.screens.clear();
    }

    fn own_screen(&self) -> call::Slot {
        self.own.clone()
    }

    fn own_camera(&self) -> call::Slot {
        self.own_camera.clone()
    }

    fn painting(&self) -> picture::Painting {
        self.painting.lock().map(|painting| painting.clone()).unwrap_or_default()
    }

    fn hold_picture(
        &self,
        place: screens::PlaceId,
        picture: proto::files::FileId,
        square: portrait::Square,
    ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
        self.pictures.hold(place, picture, square)
    }

    fn hold_file(
        &self,
        place: screens::PlaceId,
        file: proto::files::FileId,
        picture: portrait::Decoded,
    ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
        self.pictures.hold_file(place, file, picture)
    }

    fn clear_pictures(&self, place: screens::PlaceId) {
        self.pictures.clear(place);
    }

    fn hold_stills(&self, stills: Vec<errands::Still>) {
        self.stills.replace(stills);
    }

    fn icon(&self, side: u32) -> Option<Vec<u8>> {
        ui::app_icon::rgba(side).ok()
    }

    fn quit(&self) {
        if let Ok(mut closing) = self.closing.lock() {
            *closing = Closing::Now;
        }
    }

    fn hide(&self) {
        self.set_showing(Showing::Tray);
    }

    fn show(&self) {
        self.set_showing(Showing::Window);
    }

    fn come_forward(&self, activation: Option<platform::activation::Activation>) {
        (self.forward)(activation);
    }
}
