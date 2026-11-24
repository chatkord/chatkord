//! What alerts on this device: the desktop popups a message that would alert shows and the one place each is kept by,
//! this device's popups switch, whether the idle watch runs, and the badge the unread messages make. Do not disturb is
//! the account's status, each server's to tell.

use super::*;
use crate::place::{Idleness, PlaceUnread, Unread};
use crate::shortened::shortened;
use choices::Popups;

/// The most characters a popup's title holds. The host's notification area takes no more
/// (`platform::notify::TITLE_MAX`, which the host checks it matches).
pub const POPUP_TITLE_MAX: usize = 80;
/// The most characters a popup's body holds (`platform::notify::BODY_MAX`).
pub const POPUP_BODY_MAX: usize = 240;
/// The most characters a popup's id holds (`platform::notify::ID_MAX`).
pub const POPUP_ID_MAX: usize = 64;

/// The most popups the window keeps a place for; the oldest is forgotten past it.
const POPUPS_KEPT: usize = 64;

/// A popup shown, by the id the host names it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PopupId(u64);

impl PopupId {
    /// The id as the host names the popup, and gets it back from a click.
    pub fn wire(self) -> String {
        self.0.to_string()
    }

    /// The popup an id names; none for anything the window did not make.
    pub fn from_wire(id: &str) -> Option<PopupId> {
        id.parse().ok().map(PopupId)
    }
}

/// Whether the host watches for this device going idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleWatch {
    /// Watch: an account is signed in at some server. What this run tells carries its [`IdleRun`].
    Start(IdleRun),
    /// Stop watching: none is.
    Stop,
}

/// One run of the host's idle watch: what a run since stopped still tells is not taken for the running one's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleRun(u64);

/// How long without input makes the person idle, whichever source tells it.
pub const IDLE_AFTER: Duration = Duration::from_secs(60);

/// Where the idle state comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IdleSource {
    /// The host's watch of the system's own idle time.
    System,
    /// The system gives none: the person's input to this window stands in for it.
    Window {
        /// When the last input, or the window gaining focus, was told.
        last_input: Instant,
    },
}

/// What the unread messages of every server make of the rail's badge and the launcher's count on the app's icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Badge {
    /// The number or the dot, or nothing.
    pub shown: BadgeShown,
    /// Some server counted is not connected: its last count is in it.
    pub stale: bool,
}

/// What a badge shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeShown {
    /// Nothing is unread, or what is unread is muted.
    #[default]
    Nothing,
    /// Messages are unread that do not alert.
    Dot,
    /// This many unread messages would alert.
    Count(u32),
}

impl Badge {
    /// The badge of `places`' unread: the sum of those that alert, else a dot when some message that does not is
    /// unread; an unconnected server keeps its last count, marked.
    pub fn of(places: impl IntoIterator<Item = PlaceUnread>) -> Badge {
        let mut counted = 0u32;
        let mut quiet = false;
        let mut stale = false;
        for place in places {
            counted = counted.saturating_add(place.unread.alerting);
            quiet |= place.unread.quiet;
            stale |= place.stale && (place.unread.alerting > 0 || place.unread.quiet);
        }
        let shown = match (counted, quiet) {
            (0, false) => BadgeShown::Nothing,
            (0, true) => BadgeShown::Dot,
            (counted, _) => BadgeShown::Count(counted),
        };
        Badge { shown, stale }
    }

    /// The number the desktop's launcher shows on the app's icon: the same count the rail shows, of what alerts; a dot
    /// or nothing is no number.
    pub fn count(&self) -> Option<u32> {
        match self.shown {
            BadgeShown::Count(count) => Some(count),
            BadgeShown::Dot | BadgeShown::Nothing => None,
        }
    }
}

/// The popups shown, a message's sound, and what watches this device going idle.
#[derive(Debug)]
pub(super) struct Alerts {
    /// The popups shown and not yet read, each with the server and the timeline it is for.
    pub(super) popups: Vec<(PopupId, PlaceId, terms::TimelineId)>,
    /// The popups issued, so the next has a new id.
    pub(super) next_popup: u64,
    /// The popup that offers the debug report after a run that did not end cleanly, while it is offered.
    pub(super) earlier_run: Option<PopupId>,
    /// When a message's sound was last asked for, so a burst of messages sounds once.
    pub(super) message_chimed: Option<Instant>,
    /// Whether the host watches for this device going idle.
    pub(super) idle_watch: IdleWatch,
    /// How many idle watches have been started, which numbers the next.
    pub(super) idle_runs: u64,
    /// Whether the person has used the machine lately, as the host's watch last said.
    pub(super) idleness: place::Idleness,
    /// Where the idle state comes from: the host's watch, or, where the system gives none, input to this window.
    pub(super) idle_source: IdleSource,
}

impl Default for Alerts {
    fn default() -> Alerts {
        Alerts {
            popups: Vec::new(),
            next_popup: 0,
            earlier_run: None,
            message_chimed: None,
            idle_watch: IdleWatch::Stop,
            idle_runs: 0,
            idleness: place::Idleness::Active,
            idle_source: IdleSource::System,
        }
    }
}

impl Window {
    /// The badge of every server's unread messages: for the rail's picture of each server see [`Place::unread`].
    pub fn badge(&self) -> Badge {
        let Some(wall) = self.times.wall else { return Badge::default() };
        Badge::of(self.places.iter().filter_map(|place| place.unread(wall)))
    }

    /// One server's unread as the rail marks it.
    pub fn place_badge(&self, id: PlaceId) -> Badge {
        let Some(wall) = self.times.wall else { return Badge::default() };
        Badge::of(self.place(id).and_then(|place| place.unread(wall)))
    }

    /// Whether this device shows a popup for what alerts.
    pub fn popups(&self) -> Popups {
        self.voice.popups
    }

    /// The popups switch pressed: kept, and told to every server.
    pub(super) fn toggle_popups(&mut self) -> Vec<Ask> {
        self.voice.popups = self.voice.popups.toggled();
        self.tell_alerting();
        vec![Ask::Keep(Keep::Popups(self.voice.popups))]
    }

    /// The id the popup for `timeline` of `place` is known by: the one it has, else a new one.
    fn popup_id(&mut self, place: PlaceId, timeline: &terms::TimelineId) -> PopupId {
        if let Some((id, ..)) = self.alerts.popups.iter().find(|(_, held, there)| *held == place && there == timeline) {
            return *id;
        }
        self.alerts.next_popup += 1;
        let id = PopupId(self.alerts.next_popup);
        if self.alerts.popups.len() == POPUPS_KEPT {
            self.alerts.popups.remove(0);
        }
        self.alerts.popups.push((id, place, timeline.clone()));
        id
    }

    /// What a server asks of the host about popups, as asks of the host by the popup's id: its text said in this
    /// window's language and cut to what the desktop takes. Anything else the server asks passes through.
    pub(super) fn popup_asks(&mut self, place: PlaceId, asked: place::Ask) -> Option<Ask> {
        match asked {
            place::Ask::Popup { timeline, title, body } => {
                let id = self.popup_id(place, &timeline);
                // Another program draws a popup, and its shaping may show the marks that isolate a placeable.
                let title = shortened(&self.speech.words.said(&title), POPUP_TITLE_MAX);
                let body = shortened(&self.speech.words.said(&body), POPUP_BODY_MAX);
                Some(Ask::Popup { id, title, body })
            }
            // One already taken away, or never shown, is nothing to take away.
            place::Ask::Withdraw { timeline } => {
                let at = self.alerts.popups.iter().position(|(_, held, there)| *held == place && *there == timeline)?;
                let (id, ..) = self.alerts.popups.remove(at);
                Some(Ask::Withdraw { id })
            }
            other => Some(Ask::At(place, other)),
        }
    }

    /// The popup that offers the debug report, once the host says the run before this one did not end cleanly; its
    /// click shows the window and opens the report. With this device's popups off, none shows.
    pub(super) fn earlier_run_ended(&mut self) -> Vec<Ask> {
        if self.voice.popups == Popups::Off {
            return Vec::new();
        }
        self.alerts.next_popup += 1;
        let id = PopupId(self.alerts.next_popup);
        self.alerts.earlier_run = Some(id);
        let title = self.speech.words.said(&Sentence::Phrase(Phrase::EarlierRunEndedTitle));
        let body = self.speech.words.said(&Sentence::Phrase(Phrase::EarlierRunEndedBody));
        vec![Ask::Popup { id, title: shortened(&title, POPUP_TITLE_MAX), body: shortened(&body, POPUP_BODY_MAX) }]
    }

    /// The popup `id` clicked: the window shows, and the server and the channel, conversation or thread it is for
    /// open. A popup the window does not hold, or one for a server since taken away, does nothing.
    pub(super) fn popup_clicked(&mut self, id: PopupId, at: Moment) -> Vec<Ask> {
        if self.alerts.earlier_run.take_if(|offered| *offered == id).is_some() {
            let mut todo = vec![Ask::Show];
            todo.extend(self.report_asked(crate::client_report::ReportMessage::Open));
            return todo;
        }
        let Some(at_place) = self.alerts.popups.iter().position(|(held, ..)| *held == id) else { return Vec::new() };
        let (_, place, timeline) = self.alerts.popups.remove(at_place);
        let Some(zone) = self.place(place).map(|_| self.times.zone.clone()) else { return Vec::new() };
        let mut todo = vec![Ask::Show];
        todo.extend(self.rail(place, at));
        self.desk.menu = None;
        self.layout.drawer = Drawer::Shut;
        if let Some(found) = self.places.iter_mut().find(|held| held.id() == place) {
            todo.extend(found.open_timeline(&timeline, zone).into_iter().map(|asked| Ask::At(place, asked)));
        }
        todo
    }

    /// The idle state the host's watch reported.
    pub(super) fn idleness_changed(&mut self, run: IdleRun, idleness: Idleness) -> Vec<Ask> {
        if self.alerts.idle_watch == IdleWatch::Start(run) {
            self.alerts.idleness = idleness;
        }
        Vec::new()
    }

    /// The host's watch found no source: from now on the person's input to this window tells whether they are idle.
    pub(super) fn idle_source_lost(&mut self, run: IdleRun) -> Vec<Ask> {
        if self.alerts.idle_watch == IdleWatch::Start(run) {
            self.alerts.idle_source = IdleSource::Window { last_input: self.times.now };
        }
        Vec::new()
    }

    /// Input to this window, or its gaining focus, counts as the person being there where no source tells it.
    pub(super) fn input_seen(&mut self) {
        if let IdleSource::Window { last_input } = &mut self.alerts.idle_source {
            *last_input = self.times.now;
            self.alerts.idleness = Idleness::Active;
        }
    }

    /// When the person turns idle for want of input to this window, while that is how it is told.
    pub(super) fn idle_due(&self) -> Option<Instant> {
        match self.alerts.idle_source {
            IdleSource::Window { last_input }
                if matches!(self.alerts.idle_watch, IdleWatch::Start(_))
                    && self.alerts.idleness == Idleness::Active =>
            {
                Some(last_input + IDLE_AFTER)
            }
            IdleSource::Window { .. } | IdleSource::System => None,
        }
    }

    /// What the idle watch is owed after a change: it runs while an account is signed in at some server, and each such
    /// server is told when this device goes idle or comes back.
    pub(super) fn follow_idleness(&mut self) -> Vec<Ask> {
        let wanted = self.places.iter().any(Place::wants_idleness);
        let mut todo = Vec::new();
        if wanted != matches!(self.alerts.idle_watch, IdleWatch::Start(_)) {
            self.alerts.idle_source = IdleSource::System;
            self.alerts.idle_watch = if wanted {
                self.alerts.idle_runs += 1;
                // A watch begun takes the person as active.
                self.alerts.idleness = Idleness::Active;
                IdleWatch::Start(IdleRun(self.alerts.idle_runs))
            } else {
                IdleWatch::Stop
            };
            todo.push(Ask::Idle(self.alerts.idle_watch));
        }
        if self.idle_due().is_some_and(|due| self.times.now >= due) {
            self.alerts.idleness = Idleness::Idle;
        }
        let idleness = self.alerts.idleness;
        for place in &mut self.places {
            let id = place.id();
            todo.extend(place.tell_activity(idleness).map(|asked| Ask::At(id, asked)));
        }
        todo
    }

    /// Every server told what alerts on this device.
    pub(super) fn tell_alerting(&mut self) {
        let popups = self.voice.popups;
        for place in &mut self.places {
            place.set_alerting(popups);
        }
    }

    /// Every server told the time zone.
    pub(super) fn tell_zone(&mut self) {
        for place in &mut self.places {
            place.set_zone(self.times.zone.clone());
        }
    }

    /// The unread messages of every server are `Unread`; the badge of one is [`Window::place_badge`].
    pub fn unread_of(&self, id: PlaceId) -> Option<Unread> {
        let wall = self.times.wall?;
        self.place(id).and_then(|place| place.unread(wall)).map(|held| held.unread)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Clock;

    #[test]
    fn a_run_that_did_not_end_cleanly_is_offered_the_debug_report_by_a_popup_clicked_once() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        let todo = window.update(Message::EarlierRunEnded, clock.at());
        let [Ask::Popup { id, title, body }] = &todo[..] else { panic!("one popup: {todo:?}") };
        assert!(!title.is_empty() && !body.is_empty());

        let clicked = window.update(Message::PopupClicked(*id), clock.at());
        assert!(
            matches!(&clicked[..], [Ask::Show, Ask::ReadLog, ..]),
            "the window shows and reads the log: {clicked:?}"
        );
        assert!(window.update(Message::PopupClicked(*id), clock.at()).is_empty(), "a popup is clicked once");
    }

    #[test]
    fn a_device_whose_popups_are_off_is_offered_no_report_by_a_popup() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(Vec::new(), None, clock.0);
        window.toggle_popups();
        assert_eq!(window.popups(), Popups::Off);
        let todo = window.update(Message::EarlierRunEnded, clock.at());
        assert!(todo.is_empty(), "{todo:?}");
    }
}
