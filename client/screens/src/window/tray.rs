//! How the window stands in the desktop: the tray, the keyboard's focus, the session ending, how Chatkord starts and
//! closes on this device, and a close asked for.

use super::*;

/// Whether the desktop session goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Session {
    Running,
    /// It is ending: nothing waits on the person.
    Ending,
}

/// How this window stands in the desktop: its tray icon, whether the session ends, its focus and its own update.
#[derive(Debug)]
pub(super) struct Desktop {
    /// Whether the desktop shows the tray icon.
    pub(super) tray: Tray,
    /// Whether the desktop session goes on.
    pub(super) session: Session,
    /// Whether this build and run can update themselves, and where the update stands.
    pub(super) updates: Updates,
    /// The check by itself the host was last asked to schedule.
    pub(super) update_scheduled: Option<std::time::SystemTime>,
    /// Whether the window has the keyboard's focus, as the host last said (none until it says): what is read is marked
    /// read only then.
    pub(super) focused: bool,
}

impl Default for Desktop {
    fn default() -> Desktop {
        Desktop {
            tray: Tray::Absent,
            session: Session::Running,
            updates: Updates::Unoffered,
            update_scheduled: None,
            focused: false,
        }
    }
}

impl Window {
    /// The host says whether the desktop shows the tray icon.
    pub(super) fn on_tray_shown(&mut self, tray: Tray) -> Vec<Ask> {
        self.desktop.tray = tray;
        Vec::new()
    }

    /// The host read how Chatkord starts and closes, where the system offers a choice.
    pub(super) fn on_startup_read(&mut self, startup: Startup) -> Vec<Ask> {
        self.voice.startup = self.voice.startup.map(|_| startup);
        Vec::new()
    }

    /// The window took the keyboard's focus: what the page shown reads of the system is read again.
    pub(super) fn on_focused(&mut self) -> Vec<Ask> {
        self.desktop.focused = true;
        self.read_startup()
    }

    /// The window lost the keyboard's focus.
    pub(super) fn on_focus_lost(&mut self) -> Vec<Ask> {
        self.desktop.focused = false;
        Vec::new()
    }

    /// The desktop session is ending: nothing waits on the person.
    pub(super) fn on_session_ending(&mut self) -> Vec<Ask> {
        self.desktop.session = Session::Ending;
        Vec::new()
    }

    /// The window is to end now.
    pub(super) fn on_ended(&mut self) -> Vec<Ask> {
        self.desktop.session = Session::Ending;
        vec![Ask::Quit]
    }

    /// Whether the Startup and window page is the page shown.
    pub(super) fn startup_shown(&self) -> bool {
        self.hardware.device == Some(DevicePage::Startup)
    }

    /// The page shown reads the system's start again, where the system offers one: Task Manager's switch, or the
    /// desktop's own entry, may have changed it while the window ran.
    pub(super) fn read_startup(&self) -> Vec<Ask> {
        (self.startup_shown() && self.voice.startup.is_some()).then_some(Ask::ReadStartup).into_iter().collect()
    }

    /// Whether the window shows on the display, as the host last said.
    pub fn shown(&self) -> bool {
        self.layout.shown
    }

    /// How Chatkord starts and closes on this device; none where this system offers no choice.
    pub fn startup(&self) -> Option<Startup> {
        self.voice.startup
    }

    /// Whether the desktop shows the tray icon.
    pub fn tray(&self) -> Tray {
        self.desktop.tray
    }

    /// The tray icon's name and menu: mute and deafen as the call has them, or as the next call starts outside one.
    pub fn tray_menu(&self) -> TrayMenu {
        let state =
            self.places.iter().find_map(|place| place.call().filter(|call| call.running())).map(place::Call::state);
        let restart = self.desktop.updates.restart_offer(self.in_voice());
        startup::tray_menu(state.unwrap_or(self.voice.next_call), restart, &self.speech.words)
    }

    /// A switch of Settings › Startup and window flipped, kept by the host.
    pub(super) fn startup_switched(&mut self, flip: fn(Startup) -> Startup) -> Vec<Ask> {
        let Some(startup) = self.voice.startup.map(flip) else { return Vec::new() };
        self.voice.startup = Some(startup);
        vec![Ask::Keep(Keep::Startup(startup))]
    }

    /// What an entry of the tray icon's menu does: mute and deafen are the account bar's, or the call's in one.
    pub(super) fn tray_picked(&mut self, pick: TrayPick, at: Moment) -> Vec<Ask> {
        let calling = self.places.iter().find(|place| place.in_call()).map(Place::id);
        match (pick, calling) {
            (TrayPick::Open, _) => vec![Ask::Show],
            (TrayPick::Quit, _) => vec![Ask::Quit],
            (TrayPick::Restart, _) => self.updated(UpdateMessage::Restart),
            (TrayPick::Mute, Some(id)) => self.at(id, place::Message::Call(place::CallMessage::ToggleMute), at),
            (TrayPick::Deafen, Some(id)) => self.at(id, place::Message::Call(place::CallMessage::ToggleDeafen), at),
            (TrayPick::Mute, None) => self.update(Message::NextMute, at),
            (TrayPick::Deafen, None) => self.update(Message::NextDeafen, at),
        }
    }

    /// A close asked during a call asks first, on the call's server; outside a call, or asked again while asking, the
    /// window closes, so it always can. Where the window closes to the tray it goes there instead, asking nothing, and
    /// once the desktop session is ending every close quits at once.
    pub(super) fn close_asked(&mut self) -> Vec<Ask> {
        if self.desktop.session == Session::Ending
            || self.desk.dialog.as_ref().is_some_and(|dialog| matches!(dialog.asking, Asking::Quit))
        {
            return vec![Ask::Quit];
        }
        if self.voice.startup.is_some_and(|startup| startup.closes_to_tray(self.desktop.tray)) {
            return vec![Ask::Hide];
        }
        let Some(place) = self.places.iter().find(|place| place.calling()) else { return vec![Ask::Quit] };
        let call = place.live_call().map(|channel| channel.name.clone());
        self.desk.menu = None;
        self.desk.dialog = Some(Dialog { place: place.id(), asking: Asking::Quit, call });
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, ada_in_call};

    fn quits(todo: &[Ask]) -> bool {
        matches!(todo, [Ask::Quit])
    }

    #[test]
    fn closing_outside_a_call_quits_at_once() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        assert!(quits(&window.update(Message::CloseAsked, clock.at())));
        assert!(window.dialog().is_none());
    }

    #[test]
    fn closing_in_a_call_asks_on_the_call_s_server_and_quit_quits() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        assert!(window.update(Message::CloseAsked, clock.at()).is_empty(), "nothing closes yet");
        let dialog = window.dialog().expect("asked");
        assert!(matches!(dialog.asking(), Asking::Quit));
        assert_eq!((dialog.place(), dialog.call()), (place, Some("Lounge")), "about the call it ends");
        assert!(quits(&window.update(Message::Dialog(DialogMessage::Confirm), clock.at())));
    }

    #[test]
    fn closing_again_while_asked_quits_so_the_window_always_closes() {
        let clock = Clock(Instant::now());
        let (mut window, _) = ada_in_call(&clock);
        window.update(Message::CloseAsked, clock.at());
        assert!(quits(&window.update(Message::CloseAsked, clock.at())));
    }

    #[test]
    fn cancelling_the_quit_keeps_the_window_and_the_call() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        window.update(Message::CloseAsked, clock.at());
        assert!(window.update(Message::Dialog(DialogMessage::Cancel), clock.at()).is_empty());
        assert!(window.dialog().is_none());
        assert!(window.place(place).is_some_and(Place::calling), "still in the call");
        window.update(Message::CloseAsked, clock.at());
        assert!(window.update(Message::Dialog(DialogMessage::Outside), clock.at()).is_empty(), "a press outside too");
        assert!(window.dialog().is_none());
    }

    /// Ada in a call on a system that offers Startup and window, its switches as they come.
    fn ada_in_call_with_startup(clock: &Clock) -> (Window, PlaceId) {
        let (window, place) = ada_in_call(clock);
        let remembered = Remembered { startup: Some(choices::Startup::default()), ..Remembered::default() };
        (window.remembering(remembered, BTreeMap::new()), place)
    }

    #[test]
    fn closing_goes_to_the_tray_in_a_call_and_asks_nothing_where_a_tray_is_shown() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call_with_startup(&clock);
        assert!(window.update(Message::CloseAsked, clock.at()).is_empty(), "no tray yet: it asks");
        window.update(Message::Dialog(DialogMessage::Cancel), clock.at());
        window.update(Message::TrayShown(Tray::Shown), clock.at());
        assert!(matches!(&window.update(Message::CloseAsked, clock.at())[..], [Ask::Hide]));
        assert!(window.dialog().is_none() && window.place(place).is_some_and(Place::calling), "the call goes on");
        let todo = window.update(Message::ToggleCloseTo, clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Startup(kept))] if kept.close_to == choices::CloseTo::Quit));
        assert!(window.update(Message::CloseAsked, clock.at()).is_empty(), "switched off, it asks again");
    }

    #[test]
    fn once_the_session_is_ending_a_close_quits_at_once_even_in_a_call() {
        let clock = Clock(Instant::now());
        let (mut window, _) = ada_in_call_with_startup(&clock);
        window.update(Message::TrayShown(Tray::Shown), clock.at());
        window.update(Message::SessionEnding, clock.at());
        assert!(quits(&window.update(Message::CloseAsked, clock.at())), "neither hidden nor asked");
    }

    #[test]
    fn the_system_asking_chatkord_to_end_quits_at_once_even_in_a_call() {
        let clock = Clock(Instant::now());
        let (mut window, _) = ada_in_call_with_startup(&clock);
        window.update(Message::TrayShown(Tray::Shown), clock.at());
        assert!(quits(&window.update(Message::Ended, clock.at())), "neither hidden nor asked");
        assert!(window.dialog().is_none());
    }

    #[test]
    fn the_tray_mutes_and_deafens_the_call_in_one_and_the_next_call_outside_and_quits_at_once() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call_with_startup(&clock);
        let todo = window.update(Message::FromTray(TrayPick::Mute), clock.at());
        assert!(
            todo.iter()
                .any(|todo| matches!(todo, Ask::At(at, place::Ask::Call(place::Control::Mute(true))) if *at == place)),
            "the call's mute: {todo:?}"
        );
        assert!(matches!(&window.update(Message::FromTray(TrayPick::Open), clock.at())[..], [Ask::Show]));
        assert!(quits(&window.update(Message::FromTray(TrayPick::Quit), clock.at())), "never asked");

        let (mut outside, _) = Window::launch(Vec::new(), None, clock.0);
        let todo = outside.update(Message::FromTray(TrayPick::Deafen), clock.at());
        assert!(todo.iter().any(|todo| matches!(todo, Ask::NextCall(next) if next.deafened)), "{todo:?}");
        let ticked: Vec<Option<bool>> = outside.tray_menu().items.iter().map(|item| item.ticked).collect();
        assert_eq!(ticked, [None, Some(false), Some(true), None], "the menu ticks the next call's deafen");
    }

    fn reads_startup(todo: &[Ask]) -> bool {
        todo.iter().any(|done| matches!(done, Ask::ReadStartup))
    }

    #[test]
    fn the_startup_page_reads_the_system_again_when_it_opens_and_when_the_window_takes_the_focus() {
        let clock = Clock(Instant::now());
        let (mut window, _) = ada_in_call_with_startup(&clock);
        let open = Message::OpenDevice;
        assert!(!reads_startup(&window.update(Message::Focused, clock.at())), "not on the page");
        assert!(!reads_startup(&window.update(open(DevicePage::Appearance), clock.at())));
        assert!(reads_startup(&window.update(open(DevicePage::Startup), clock.at())), "the page opened");
        assert!(!reads_startup(&window.update(Message::ToggleCloseTo, clock.at())), "only as it opens");
        assert!(reads_startup(&window.update(Message::Focused, clock.at())), "back from another application");
        let turned_off = choices::Startup { at_sign_in: choices::AtSignIn::Not, ..choices::Startup::default() };
        let opens = choices::Startup { at_sign_in: choices::AtSignIn::Opens, ..turned_off };
        window.update(Message::StartupRead(opens), clock.at());
        assert_eq!(window.startup(), Some(opens), "the page shows what the system says now");
        window.update(Message::StartupRead(turned_off), clock.at());
        assert_eq!(window.startup(), Some(turned_off), "Task Manager's switch turned it off");
    }

    #[test]
    fn a_system_with_no_startup_choice_keeps_nothing_of_it() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        assert_eq!(window.startup(), None);
        assert!(window.update(Message::ToggleAtSignIn, clock.at()).is_empty());
        window.update(Message::StartupRead(choices::Startup::default()), clock.at());
        assert_eq!(window.startup(), None, "a read never makes a choice of a system that offers none");
    }
}
