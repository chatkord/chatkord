//! This device's settings: the window's pages, which no server owns, opened from the rail's gear and kept open
//! whatever a server's state, until a press on a server or a page of its own leaves them.

use super::*;

/// A page of this device's own settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePage {
    /// The theme, the zoom and the contrast.
    Appearance,
    /// The microphones, the speakers, joining muted and where a share starts.
    VoiceAndVideo,
    /// Whether the window's own sounds play.
    Notifications,
    /// Whether Chatkord opens at sign-in, where it opens then, and what closing the window does.
    Startup,
    /// This build's version, licence and source, and the notices of the projects it is built with.
    AboutChatkord,
}

/// The rail's gear, the one way into the person's settings, while it is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gear {
    /// Whether the settings it opens are shown now, the profile, the account or a page of this device, which marks it.
    pub current: bool,
}

impl Window {
    /// This device's page put away.
    pub(super) fn on_close_device(&mut self) -> Vec<Ask> {
        self.hardware.device = None;
        Vec::new()
    }

    /// The page of this device's settings shown, if one is.
    pub fn device(&self) -> Option<DevicePage> {
        self.hardware.device
    }

    /// The rail's gear: offered whenever the rail shows, whatever the servers' state, since this device's pages need
    /// none of them.
    pub fn gear(&self) -> Option<Gear> {
        let own = self.open().is_some_and(|place| place.panel() == Panel::Account);
        (!self.places.is_empty()).then_some(Gear { current: self.hardware.device.is_some() || own })
    }

    /// Whether a page of settings, the server's or this device's, is shown in the pane's place and the content's.
    pub fn page_shown(&self) -> bool {
        self.hardware.device.is_some() || self.open().is_some_and(Place::page_open)
    }

    /// The gear pressed: the open server's account while it is signed in, else this device's first page.
    pub(super) fn open_settings(&mut self, at: Moment) -> Vec<Ask> {
        if self.gear().is_none() {
            return Vec::new();
        }
        match self.open().filter(|place| place.signed_in()).map(Place::id) {
            Some(id) => self.at(id, place::Message::Page(place::PageMessage::OpenAccount), at),
            None => self.open_device(DevicePage::Appearance),
        }
    }

    /// `page` shown, in place of every server's page, each left as it asks.
    pub(super) fn open_device(&mut self, page: DevicePage) -> Vec<Ask> {
        if self.places.is_empty() {
            return Vec::new();
        }
        let mut todo = Vec::new();
        for place in &mut self.places {
            let id = place.id();
            todo.extend(place.close_pages().into_iter().map(|asked| Ask::At(id, asked)));
        }
        self.hardware.device = Some(page);
        if page == DevicePage::AboutChatkord {
            todo.extend(self.open_about());
        }
        todo
    }

    /// One page at a time: a server's page opened since closes this device's, and no server left closes it too.
    pub(super) fn settle_device(&mut self) {
        if self.places.is_empty() || self.places.iter().any(Place::page_open) {
            self.hardware.device = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{ADA, Clock, accepted, books, flow, id, owls, tell, token};
    use proto::chatkord_v1::CloseReason;
    use proto::rules::Grants;
    use terms::Ended;

    fn signs_in(window: &mut Window, clock: &Clock, place: PlaceId) {
        tell(window, clock, place, flow(token()));
        tell(window, clock, place, flow(accepted(Grants::default())));
    }

    fn signed_out(window: &mut Window, clock: &Clock, place: PlaceId) {
        let reset = Ended { reason: Some(CloseReason::PasswordReset), ..Ended::default() };
        tell(window, clock, place, place::Message::Session(place::SessionMessage::Disconnected(reset)));
    }

    #[test]
    fn the_gear_is_offered_whenever_the_rail_shows_and_opens_this_devices_pages_with_no_server_signed_in() {
        let clock = Clock(Instant::now());
        let (mut fresh, _) = Window::launch(vec![Opening::Fresh], None, clock.0);
        assert_eq!(fresh.gear(), None, "the welcome steps are full-window: no rail");
        assert!(fresh.update(Message::OpenSettings, clock.at()).is_empty());
        assert_eq!(fresh.device(), None);

        let rows = vec![Opening::Account(owls(), ADA.into()), Opening::Account(books(), ADA.into())];
        let (mut window, _) = Window::launch(rows, None, clock.0);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        assert_eq!(window.gear(), Some(Gear { current: false }), "no server signed in, and the rail shows");
        window.update(Message::OpenSettings, clock.at());
        assert_eq!(window.device(), Some(DevicePage::Appearance), "this device's pages need no server");
        assert_eq!(window.gear(), Some(Gear { current: true }));
        window.update(Message::OpenDevice(DevicePage::AboutChatkord), clock.at());
        assert_eq!(window.device(), Some(DevicePage::AboutChatkord));
        window.update(Message::CloseDevice, clock.at());
        assert_eq!(window.device(), None);
        signs_in(&mut window, &clock, books_id);
        assert_eq!(window.gear(), Some(Gear { current: false }));
        window.update(Message::OpenSettings, clock.at());
        assert_eq!(window.device(), Some(DevicePage::Appearance), "the open server is not signed in: this device's");
        assert_eq!(window.gear(), Some(Gear { current: true }));
        signs_in(&mut window, &clock, owls_id);
        window.update(Message::OpenSettings, clock.at());
        assert_eq!(window.open().map(Place::panel), Some(Panel::Account), "signed in: the server's own first");
        assert_eq!((window.device(), window.gear()), (None, Some(Gear { current: true })));
        window.update(Message::OpenDevice(DevicePage::Notifications), clock.at());
        signed_out(&mut window, &clock, owls_id);
        signed_out(&mut window, &clock, books_id);
        assert_eq!(window.device(), Some(DevicePage::Notifications), "what is open stays");
        assert_eq!(window.gear(), Some(Gear { current: true }), "and so does the gear that shows it open");
        window.update(Message::CloseDevice, clock.at());
        assert_eq!(window.gear(), Some(Gear { current: false }), "none signed in, nothing open: the gear stays");
    }

    #[test]
    fn a_press_on_a_servers_picture_closes_this_devices_page_and_shows_that_server() {
        let clock = Clock(Instant::now());
        let mut window = crate::harness::two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        assert_eq!(window.open().map(Place::panel), Some(Panel::Discussion), "the account page is left");
        window.update(Message::Rail(books_id), clock.at());
        assert_eq!((window.open().map(Place::id), window.device()), (Some(books_id), None), "another server's picture");
        assert_eq!(window.open().map(Place::panel), Some(Panel::Discussion));
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        window.update(Message::Rail(books_id), clock.at());
        assert_eq!(window.device(), None, "the open server's picture");
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
        tell(&mut window, &clock, books_id, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.device(), None, "a server's own page");
        assert_eq!(window.open().map(Place::panel), Some(Panel::Account));
    }
}
