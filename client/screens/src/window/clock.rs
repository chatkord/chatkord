//! What the window knows of time: when it was last told, the wall clock, the time zone and the day, and when a wake is
//! next due.

use super::*;

/// How many messages may have changed what the window draws; equal before and after a message, nothing drawn did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(u64);

impl Revision {
    pub(super) fn moved(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

/// The times the window was told, and where it is.
#[derive(Debug)]
pub(super) struct Times {
    /// The time the window was last told.
    pub(super) now: Instant,
    /// The wall clock when this window was last told, in seconds since the epoch; none before the first update.
    pub(super) wall: Option<i64>,
    /// The one wake the host keeps, as last asked: a later ask replaces it, so the same time is not asked again.
    pub(super) waking: Option<Instant>,
    /// Where what the window draws stands.
    pub(super) revision: Revision,
    /// The time zone messages' days and times are said in; UTC until the host says.
    pub(super) zone: chat::TimeZone,
    /// The day it is there; none until the host says, when every day line says its date.
    pub(super) today: Option<chat::Date>,
}

impl Times {
    /// The times of a window launched at `now`.
    pub(super) fn new(now: Instant) -> Times {
        Times { now, wall: None, waking: None, revision: Revision::default(), zone: chat::TimeZone::UTC, today: None }
    }
}

impl Window {
    /// The day it is where the person is, once the host has said.
    pub fn today(&self) -> Option<chat::Date> {
        self.times.today
    }

    /// The time zone the person is in, which a list's times are said in.
    pub fn zone(&self) -> &chat::TimeZone {
        &self.times.zone
    }

    /// The time this window was last told, which a countdown is drawn from.
    pub fn now(&self) -> Instant {
        self.times.now
    }

    /// The wall clock when this window was last told, in seconds since the epoch, which a mute is measured by; none
    /// before the first update.
    pub fn wall(&self) -> Option<i64> {
        self.times.wall
    }

    /// Where what the window draws stands: a host lays the window out again only once this moved.
    pub fn revision(&self) -> Revision {
        self.times.revision
    }

    /// Whether what the window draws moved since `seen` (none: never seen), which is brought up to now: a host
    /// derives what it shows of the window outside it (the tray's menu, the launcher's badge, what is kept awake)
    /// again only then.
    pub fn moved_since(&self, seen: &mut Option<Revision>) -> bool {
        seen.replace(self.times.revision) != Some(self.times.revision)
    }

    /// The window is told `at`.
    pub(super) fn on_moment(&mut self, at: Moment) {
        self.times.now = at.now;
        self.times.wall = Some(at.wall);
    }

    /// The time zone the host says, told on to every server.
    pub(super) fn on_zone(&mut self, zone: chat::TimeZone) -> Vec<Ask> {
        self.times.zone = zone;
        self.tell_zone();
        Vec::new()
    }

    /// The day the host says it is.
    pub(super) fn on_today(&mut self, today: chat::Date) -> Vec<Ask> {
        self.times.today = Some(today);
        Vec::new()
    }

    /// A timer fired: a wake that is past is done, and a toast whose time is past goes.
    pub(super) fn on_tick(&mut self, at: Moment) -> Vec<Ask> {
        self.times.waking = self.times.waking.filter(|when| *when > at.now);
        self.desk.toast = self.desk.toast.take().filter(|toast| toast.shows(at.now));
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, ada_in_call, tell};

    #[test]
    fn what_is_derived_from_the_window_is_derived_again_only_once_it_moved() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = ada_in_call(&clock);
        let mut seen = None;
        assert!(window.moved_since(&mut seen), "never seen");
        assert!(!window.moved_since(&mut seen), "nothing told since");
        let silence = crate::RoomEvent::Levels(vec![(crate::harness::member("ada"), 0)]);
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::Room(silence)));
        assert!(!window.moved_since(&mut seen), "a level that changes nothing drawn");
        window.update(Message::Tick, clock.at());
        assert!(window.moved_since(&mut seen), "a message that may change what is drawn");
        assert!(!window.moved_since(&mut seen), "and once only");
    }
}
