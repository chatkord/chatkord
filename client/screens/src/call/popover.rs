//! What is open over a call, one thing at a time, and the popovers about a picture.

use super::*;

/// What is open over a call; one thing at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    /// The menu of microphones and speakers.
    Devices,
    /// The popover of how the connection is.
    Connection,
    /// The panel that offers to pick the screen to share.
    Share,
    /// Our own picker of what to share.
    Sources,
    /// The popover about the picture of someone's screen.
    Picture,
    /// The popover about this account's own screen.
    OwnPicture,
    /// The popover about this account's own camera.
    OwnCamera,
    /// The popover of how loud the sound of someone's shared screen plays here.
    ShareSound,
    /// Audio controls: every sound in the call but this account's own, each with its volume.
    Audio,
    /// What the grid leaves out: people with no picture, one's own share.
    View,
}

/// What is open over a call, with whom or what it is about.
#[derive(Debug)]
pub(super) enum Opened {
    Devices,
    Connection,
    Share,
    Sources(Sources),
    Picture(MemberId),
    OwnPicture,
    OwnCamera,
    ShareSound(MemberId),
    Audio,
    View,
}

impl Opened {
    /// Whether the call's page draws it, so it is shown only while that page is; the connection's popover and the
    /// picker of what to share are the window's and show over any page.
    fn on_the_page(&self) -> bool {
        match self {
            Opened::Connection | Opened::Sources(_) => false,
            Opened::Devices
            | Opened::Share
            | Opened::Picture(_)
            | Opened::OwnPicture
            | Opened::OwnCamera
            | Opened::ShareSound(_)
            | Opened::Audio
            | Opened::View => true,
        }
    }

    fn over(&self) -> Over {
        match self {
            Opened::Devices => Over::Devices,
            Opened::Connection => Over::Connection,
            Opened::Share => Over::Share,
            Opened::Sources(_) => Over::Sources,
            Opened::Picture(_) => Over::Picture,
            Opened::OwnPicture => Over::OwnPicture,
            Opened::OwnCamera => Over::OwnCamera,
            Opened::ShareSound(_) => Over::ShareSound,
            Opened::Audio => Over::Audio,
            Opened::View => Over::View,
        }
    }
}

impl Call {
    /// Whose screen the open popover is about.
    pub fn about(&self) -> Option<&MemberId> {
        match &self.open {
            Some(Opened::Picture(user_id)) => Some(user_id),
            _ => None,
        }
    }

    /// Our own picker, while it is open.
    pub fn sources(&self) -> Option<&Sources> {
        match &self.open {
            Some(Opened::Sources(sources)) => Some(sources),
            _ => None,
        }
    }

    /// What the call's page opened over it is closed: the page is not shown, so nothing is left open that no one sees.
    fn close_on_the_page(&mut self) {
        if self.open.as_ref().is_some_and(Opened::on_the_page) {
            self.open = None;
        }
    }

    /// What is open over the call, while it is joined and not over.
    pub fn open(&self) -> Option<Over> {
        self.open.as_ref().map(Opened::over).filter(|_| self.live() && self.joined())
    }

    /// Whether the call bar stays shown however Voice options set it: a menu that hangs from it is open.
    pub fn bar_held(&self) -> bool {
        matches!(self.open(), Some(Over::Devices | Over::Share))
    }
}

impl Place {
    /// What the call's page opened over the call is closed unless that page is `shown`: the call's page is shown only
    /// where the window shows this place on its voice page.
    pub(crate) fn settle_call_popover(&mut self, shown: bool) {
        let on_its_page = shown && self.panel() == Panel::Call;
        if let Status::Signed(signed) = &mut self.status
            && !on_its_page
            && let Some(call) = signed.call.as_mut()
        {
            call.close_on_the_page();
        }
    }

    pub(super) fn close_over(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            Some(call) => {
                let watching = matches!(call.open.take(), Some(Opened::Sources(_)));
                if watching { vec![Ask::StopSources] } else { Vec::new() }
            }
            None => Vec::new(),
        }
    }

    pub(super) fn toggle_info(&mut self, user_id: MemberId) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live())
            && matches!(call.screens.get(&user_id), Some(Received::Playing(_)))
        {
            let same = call.about() == Some(&user_id);
            call.open = if same { None } else { Some(Opened::Picture(user_id)) };
        }
        Vec::new()
    }

    pub(super) fn toggle_own_info(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let shown = |call: &Call| call.cards().iter().any(|card| matches!(card, Card::OwnScreen(_)));
        if let Some(call) = signed.call.as_mut().filter(|call| call.live() && call.outgoing().is_some() && shown(call))
        {
            call.open = if matches!(call.open, Some(Opened::OwnPicture)) { None } else { Some(Opened::OwnPicture) };
        }
        Vec::new()
    }

    pub(super) fn toggle_own_camera_info(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live() && call.own_camera_pictured()) {
            call.open = if matches!(call.open, Some(Opened::OwnCamera)) { None } else { Some(Opened::OwnCamera) };
        }
        Vec::new()
    }
}

/// What opens over a joined call is shut by a second press on the same control, and swaps for the other one.
pub(super) fn toggled(signed: &mut SignedIn, opened: Opened) -> Vec<Ask> {
    if let Some(call) = signed.call.as_mut().filter(|call| call.live() && call.joined()) {
        let same = call.open.as_ref().map(Opened::over) == Some(opened.over());
        call.open = if same { None } else { Some(opened) };
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::Opening;
    use crate::harness::{ADA, Clock, accepted_as, ada_in_call, flow, id, owls, sound, tell, token};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;

    #[test]
    fn the_devices_menu_opens_only_in_a_joined_call_and_the_call_keeps_what_it_found() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let open = |window: &Window| window.open().and_then(Place::call).and_then(place::Call::open);
        assert_eq!(open(&window), None);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Devices));
        assert_eq!(open(&window), Some(Over::Devices));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CloseOver));
        assert_eq!(open(&window), None);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Sound(sound())));
        assert_eq!(window.open().and_then(Place::call).and_then(place::Call::sound), Some(&sound()));
        let (mut window, place) = {
            let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
            let place = id(&window, 0);
            tell(&mut window, &clock, place, flow(token()));
            tell(&mut window, &clock, place, flow(accepted_as("ada")));
            (window, place)
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Devices));
        assert!(!window.open().is_some_and(|open| open.call().is_some()), "no call, no menu");
    }
}
