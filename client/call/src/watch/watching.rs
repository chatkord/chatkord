//! What the call decides of one screen it watches: whether the window shows it, whether its stream is held at the
//! server and why, and when its sender is asked for a key frame. The decoder is the [`crate::watch::viewing::Viewing`]'s.

use crate::watch::viewing::UNSEEN;
use std::time::Duration;
use tokio::time::Instant;

/// The least time between two asks of a sender for a key frame, chosen to give one time to arrive.
const KEY_FRAME_EVERY: Duration = Duration::from_millis(500);

/// How long a viewer whose stream waits for a key frame waits before asking its sender again.
const ASK_AGAIN: Duration = Duration::from_secs(1);

/// The least time from a viewer behind its stream's ask to its next, doubled at each ask up to [`BEHIND_SPACING_MOST`]:
/// its own slow decode breaks the stream again, and each key frame it asks for goes to every viewer.
const BEHIND_SPACING_FIRST: Duration = Duration::from_secs(1);

/// The most time between two asks of a viewer behind its stream.
const BEHIND_SPACING_MOST: Duration = Duration::from_secs(8);

/// Why a stream is held at the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// The window showed it nowhere for [`UNSEEN`]; it starts again once it is shown.
    Unseen,
    /// The person paused it; only they start it again.
    Paused,
}

/// Whether a stream is held at the server, and for which reason.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Hold {
    #[default]
    Off,
    /// The server was asked to hold it and has not answered.
    Asking(Why),
    On(Why),
    /// Held at the server while its decoder opens again, and started there once it is open: the key frame the start
    /// asks for would find no decoder before, and be lost. `paused` when the person paused it meanwhile.
    Opening {
        paused: bool,
    },
}

/// What follows a held stream's decoder opening again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reopened {
    /// The server is asked to start the stream.
    StartAgain,
    /// The person paused it meanwhile: the decoder closes again, and the stream stays held.
    Close,
}

/// Whether the window shows a screen, and whether its stream is held for being shown nowhere or paused by the person.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Sight {
    /// Since when the window has shown it nowhere; none while it shows it.
    unseen_since: Option<Instant>,
    hold: Hold,
}

impl Sight {
    /// The window shows it or not as of `now`; whether its held stream's decoder is to be opened again. One the person
    /// paused stays held whatever the window shows.
    fn seen(&mut self, seen: bool, now: Instant) -> bool {
        if !seen {
            self.unseen_since.get_or_insert(now);
            return false;
        }
        self.unseen_since = None;
        match self.hold {
            Hold::On(Why::Unseen) => {
                self.hold = Hold::Opening { paused: false };
                true
            }
            Hold::Off | Hold::Asking(_) | Hold::On(Why::Paused) | Hold::Opening { .. } => false,
        }
    }

    /// When it is to be held, unseen for [`UNSEEN`]; none while it is seen or held already.
    fn hold_at(self) -> Option<Instant> {
        self.unseen_since.filter(|_| self.hold == Hold::Off).map(|since| since + UNSEEN)
    }

    /// Whether the server is to be asked to hold it now.
    fn hold_now(&mut self, now: Instant) -> bool {
        let due = self.hold_at().is_some_and(|at| at <= now);
        if due {
            self.hold = Hold::Asking(Why::Unseen);
        }
        due
    }

    /// The person pauses it; whether the server is to be asked to hold it.
    fn pause(&mut self) -> bool {
        match self.hold {
            Hold::Opening { .. } => {
                self.hold = Hold::Opening { paused: true };
                false
            }
            Hold::Off => {
                self.hold = Hold::Asking(Why::Paused);
                true
            }
            Hold::Asking(_) => {
                self.hold = Hold::Asking(Why::Paused);
                false
            }
            Hold::On(_) => {
                self.hold = Hold::On(Why::Paused);
                false
            }
        }
    }

    /// The person starts it again; whether its decoder is to be opened again.
    fn resume(&mut self) -> bool {
        match self.hold {
            Hold::On(Why::Paused) => {
                self.hold = Hold::Opening { paused: false };
                true
            }
            Hold::Opening { .. } => {
                self.hold = Hold::Opening { paused: false };
                false
            }
            // The answer decides: it holds it for being unseen, or starts it at once if it is shown.
            Hold::Asking(Why::Paused) => {
                self.hold = Hold::Asking(Why::Unseen);
                false
            }
            Hold::Off | Hold::On(Why::Unseen) | Hold::Asking(Why::Unseen) => false,
        }
    }

    /// The server answered the ask to hold it, `held` or refused, as of `now`; whether its decoder is to be opened
    /// again because it is shown now.
    fn answered(&mut self, held: bool, now: Instant) -> bool {
        let Hold::Asking(why) = self.hold else { return false };
        if !held {
            self.hold = Hold::Off;
            // A hold refused is asked again after its time, not at once.
            if why == Why::Unseen && self.unseen_since.is_some() {
                self.unseen_since = Some(now);
            }
            return false;
        }
        if why == Why::Unseen && self.unseen_since.is_none() {
            self.hold = Hold::Opening { paused: false };
            return true;
        }
        self.hold = Hold::On(why);
        false
    }

    /// Whether it is held or being held, so no key frame is asked for.
    fn held(self) -> bool {
        self.hold != Hold::Off
    }

    /// Whether the person paused it.
    fn paused(self) -> bool {
        matches!(self.hold, Hold::Asking(Why::Paused) | Hold::On(Why::Paused) | Hold::Opening { paused: true })
    }

    /// Its decoder opened again, or did not: what follows, none unless it was opening.
    fn opened(&mut self) -> Option<Reopened> {
        let Hold::Opening { paused } = self.hold else { return None };
        if paused {
            self.hold = Hold::On(Why::Paused);
            return Some(Reopened::Close);
        }
        self.hold = Hold::Off;
        Some(Reopened::StartAgain)
    }
}

/// One screen watched: its sight, and the key frame its decoder wants.
#[derive(Debug)]
pub(crate) struct Watching {
    sight: Sight,
    /// When the sender is next asked for a key frame, and when it last was.
    ask_at: Option<Instant>,
    asked: Option<Instant>,
    /// The least time from the last ask to the next ([`spacing`]).
    spacing: Duration,
    /// Its card was reported at its first picture, before the next look.
    first_reported: bool,
}

impl Watching {
    /// A screen taken at `now`, held from the start when the person had paused it. The first thing a stream needs is
    /// a key frame, unless it is held.
    pub(crate) fn new(paused: bool, now: Instant) -> Self {
        let mut sight = Sight::default();
        if paused {
            sight.hold = Hold::On(Why::Paused);
        }
        Self {
            ask_at: (!sight.held()).then_some(now),
            sight,
            asked: None,
            spacing: KEY_FRAME_EVERY,
            first_reported: false,
        }
    }

    /// The window shows it (`seen`) or not, as of `now`. Whether its held stream's decoder is to be opened again,
    /// the stream started at the server once it is ([`Watching::opened`]).
    pub(crate) fn seen(&mut self, seen: bool, now: Instant) -> bool {
        self.sight.seen(seen, now)
    }

    /// Whether the server holds its stream, the person having paused it or the window shown it nowhere.
    pub(crate) fn on_hold(&self) -> bool {
        matches!(self.sight.hold, Hold::On(_))
    }

    /// Whether the person has it paused.
    pub(crate) fn is_paused(&self) -> bool {
        self.sight.paused()
    }

    /// The person pauses it. Whether the server is to be asked to hold it; it asks for no key frame meanwhile.
    pub(crate) fn pause(&mut self) -> bool {
        let ask = self.sight.pause();
        if ask {
            self.ask_at = None;
        }
        ask
    }

    /// The person starts it again. Whether its decoder is to be opened again ([`Watching::opened`]).
    pub(crate) fn resume(&mut self) -> bool {
        self.sight.resume()
    }

    /// The server answered an ask to hold it, `held` or refused, as of `now`. Whether its decoder is to be opened
    /// again because the window shows it now ([`Watching::opened`]).
    pub(crate) fn hold_answered(&mut self, held: bool, now: Instant) -> bool {
        self.sight.answered(held, now)
    }

    /// Its held stream's decoder opened again, or did not, at `now`: what follows. Started again, the server's resume
    /// asks the sender for the key frame the stream waits for, which is its ask: one of its own too would reach the
    /// server inside the second it holds a stream's asks and be sent on at the second's end, a key frame more. It asks
    /// only if none came by [`ASK_AGAIN`].
    pub(crate) fn opened(&mut self, now: Instant) -> Option<Reopened> {
        let reopened = self.sight.opened();
        if reopened == Some(Reopened::StartAgain) {
            self.asked = Some(now);
            self.ask_at = Some(now + ASK_AGAIN);
        }
        reopened
    }

    /// When it is to be held, unseen for [`UNSEEN`]; none while it is seen or held already.
    pub(crate) fn hold_at(&self) -> Option<Instant> {
        self.sight.hold_at()
    }

    /// Whether its stream is to be held at `now`, unseen for [`UNSEEN`]; it asks for no key frame while held.
    pub(crate) fn hold_now(&mut self, now: Instant) -> bool {
        let due = self.sight.hold_now(now);
        if due {
            self.ask_at = None;
        }
        due
    }

    /// The decoder wants a key frame at `now`: it is asked for at once, or when the last ask has had time to be
    /// answered, a viewer behind its stream waiting longer at each ask.
    pub(crate) fn want_key_frame(&mut self, now: Instant) {
        if self.sight.held() {
            return;
        }
        let due = self.asked.map_or(now, |asked| asked + self.spacing);
        self.ask_at = Some(due.max(now));
    }

    /// When a wanted key frame may be asked for; none while none is wanted.
    pub(crate) fn key_frame_at(&self) -> Option<Instant> {
        self.ask_at
    }

    /// Whether a wanted key frame may be asked for at `now`.
    pub(crate) fn key_frame_due(&self, now: Instant) -> bool {
        self.ask_at.is_some_and(|at| at <= now)
    }

    /// The key frame that was due at `now`: whether the sender is to be asked, while the stream still waits for one,
    /// and again until it comes, as an ask can be lost or leave before the receive transport is up. A decoder behind
    /// the stream asks once a break, each break's ask further from the last ([`spacing`]): its slow decode of the key
    /// frame is no lost ask.
    pub(crate) fn asked_key_frame(&mut self, awaiting_key: bool, behind: bool, now: Instant) -> bool {
        let ask = ask(awaiting_key, behind, now);
        self.spacing = spacing(self.spacing, &ask, behind);
        self.ask_at = match ask {
            Ask::Repeating(at) => Some(at),
            Ask::Not | Ask::Once => None,
        };
        if ask != Ask::Not {
            self.asked = Some(now);
        }
        ask != Ask::Not
    }

    /// Whether its card was reported at its first picture.
    pub(crate) fn first_reported(&self) -> bool {
        self.first_reported
    }

    /// Its card was reported at its first picture.
    pub(crate) fn report_first(&mut self) {
        self.first_reported = true;
    }
}

/// Whether a viewer asks for a key frame when one is due.
#[derive(Debug, PartialEq, Eq)]
enum Ask {
    /// A key frame came: nothing to ask.
    Not,
    /// Asked once: a decoder behind its stream would take its own slow decode of the key frame for a lost ask.
    Once,
    /// Asked, and again at this time while the stream still waits.
    Repeating(Instant),
}

/// What a viewer asks when a key frame is due: while its stream waits for one, it asks, and again after [`ASK_AGAIN`]
/// unless its decoder is `behind` the stream.
fn ask(awaiting_key: bool, behind: bool, now: Instant) -> Ask {
    match (awaiting_key, behind) {
        (false, _) => Ask::Not,
        (true, true) => Ask::Once,
        (true, false) => Ask::Repeating(now + ASK_AGAIN),
    }
}

/// The least time from a viewer's ask to its next, `before` the last: [`KEY_FRAME_EVERY`] once a key frame came or
/// while its decoder keeps up; while it is `behind`, each ask [`BEHIND_SPACING_FIRST`] or twice the last, up to
/// [`BEHIND_SPACING_MOST`].
fn spacing(before: Duration, ask: &Ask, behind: bool) -> Duration {
    match (ask, behind) {
        (Ask::Not, _) | (_, false) => KEY_FRAME_EVERY,
        (Ask::Once | Ask::Repeating(_), true) => (before * 2).clamp(BEHIND_SPACING_FIRST, BEHIND_SPACING_MOST),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewer_asks_again_while_its_stream_waits_for_a_key_frame_and_not_once_one_came() {
        let now = Instant::now();
        assert_eq!(
            ask(true, false, now),
            Ask::Repeating(now + ASK_AGAIN),
            "a stream that broke after its first picture asks, and again"
        );
        assert_eq!(ask(false, false, now), Ask::Not, "a key frame came: nothing more to ask");
    }

    #[test]
    fn a_viewer_behind_its_stream_asks_once_a_break_not_again_each_second() {
        let now = Instant::now();
        assert_eq!(ask(true, true, now), Ask::Once, "a break asks, or nothing ever mends it");
        assert_eq!(ask(false, true, now), Ask::Not, "a key frame came: nothing to ask");
    }

    #[test]
    fn a_viewer_behind_its_stream_spaces_its_asks_from_a_second_doubling_to_eight_until_a_key_frame_keeps() {
        let start = Instant::now();
        let mut watching = Watching::new(false, start);
        let mut now = start;
        assert!(watching.asked_key_frame(true, true, now), "a break asks");
        let mut gaps = Vec::new();
        for _ in 0..5 {
            // The key frame came and its slow decode filled the queue again at once.
            watching.want_key_frame(now);
            let at = watching.key_frame_at().expect("a key frame is wanted");
            gaps.push((at - now).as_secs());
            now = at;
            assert!(watching.key_frame_due(now) && watching.asked_key_frame(true, true, now));
        }
        assert_eq!(gaps, [1, 2, 4, 8, 8]);
        watching.want_key_frame(now + BEHIND_SPACING_MOST);
        assert!(!watching.asked_key_frame(false, true, now + BEHIND_SPACING_MOST), "a key frame came and kept");
        watching.want_key_frame(now + BEHIND_SPACING_MOST);
        let after = now + BEHIND_SPACING_MOST;
        assert_eq!(watching.key_frame_at(), Some(after), "the spacing starts over, the last ask long past");
        assert!(watching.asked_key_frame(true, true, after));
        watching.want_key_frame(after);
        assert_eq!(watching.key_frame_at(), Some(after + BEHIND_SPACING_FIRST), "from a second again");
        assert!(watching.asked_key_frame(true, false, after + BEHIND_SPACING_FIRST), "caught up");
        watching.want_key_frame(after + BEHIND_SPACING_FIRST);
        assert_eq!(watching.key_frame_at(), Some(after + BEHIND_SPACING_FIRST + KEY_FRAME_EVERY), "one that keeps up");
    }

    #[test]
    fn a_screen_taken_held_asks_for_no_key_frame_and_one_shown_asks_at_once() {
        let now = Instant::now();
        assert_eq!(Watching::new(false, now).key_frame_at(), Some(now));
        let held = Watching::new(true, now);
        assert!(held.is_paused() && held.key_frame_at().is_none());
    }

    /// The ways a held screen is taken again.
    #[derive(Debug, Clone, Copy)]
    enum Way {
        /// Shown again after its hold for being unseen.
        ShownAgain,
        /// Started again by the person who paused it.
        Started,
        /// Shown again while its hold was asked, and then held.
        ShownWhileHeld,
    }

    const WAYS: [Way; 3] = [Way::ShownAgain, Way::Started, Way::ShownWhileHeld];

    /// Takes `watching`, watched since `start`, again `way` at `at`: its decoder is to open.
    fn take_again(watching: &mut Watching, way: Way, start: Instant, at: Instant) {
        match way {
            Way::ShownAgain => {
                watching.seen(false, start);
                assert!(watching.hold_now(start + UNSEEN));
                assert!(!watching.hold_answered(true, start + UNSEEN));
                assert!(watching.seen(true, at), "shown again: its decoder opens");
            }
            Way::Started => {
                assert!(watching.pause());
                assert!(!watching.hold_answered(true, start));
                assert!(watching.resume(), "the person starts it again: its decoder opens");
            }
            Way::ShownWhileHeld => {
                watching.seen(false, start);
                assert!(watching.hold_now(start + UNSEEN));
                assert!(!watching.seen(true, at), "the hold is not answered yet");
                assert!(watching.hold_answered(true, at), "held and shown: its decoder opens");
            }
        }
    }

    #[test]
    fn a_held_screen_is_started_once_its_decoder_opened_and_asks_for_a_key_frame_only_if_the_start_s_did_not_come() {
        let start = Instant::now();
        let (again, open) = (start + UNSEEN * 2, start + UNSEEN * 2 + Duration::from_millis(1_700));
        for way in WAYS {
            for came in [true, false] {
                let mut watching = Watching::new(false, start);
                take_again(&mut watching, way, start, again);
                watching.want_key_frame(again);
                assert_eq!(watching.key_frame_at(), None, "{way:?}: nothing is asked while its decoder opens");
                assert_eq!(watching.opened(open), Some(Reopened::StartAgain), "{way:?}: started once it opened");
                assert_eq!(watching.opened(open), None, "{way:?}: once");
                assert_eq!(watching.key_frame_at(), Some(open + ASK_AGAIN), "{way:?}: the start asked, not this");
                let asked = watching.asked_key_frame(!came, false, open + ASK_AGAIN);
                assert_eq!(asked, !came, "{way:?}: asked only when the start's key frame did not come");
            }
        }
    }

    #[test]
    fn a_held_screen_paused_while_its_decoder_opens_closes_it_and_stays_held() {
        let start = Instant::now();
        let again = start + UNSEEN * 2;
        for way in WAYS {
            let mut watching = Watching::new(false, start);
            take_again(&mut watching, way, start, again);
            assert!(!watching.pause(), "{way:?}: never started at the server, nothing to hold there");
            assert!(watching.is_paused());
            assert_eq!(watching.opened(again), Some(Reopened::Close), "{way:?}");
            assert!(watching.on_hold() && watching.is_paused() && watching.key_frame_at().is_none(), "{way:?}");
            assert!(watching.resume(), "{way:?}: started again, it opens again");
            assert!(!watching.pause() && !watching.resume(), "{way:?}: paused and started while it opens");
            assert_eq!(watching.opened(again), Some(Reopened::StartAgain), "{way:?}");
        }
    }

    #[test]
    fn a_key_frame_wanted_is_asked_at_once_then_not_again_for_its_time() {
        let now = Instant::now();
        let mut watching = Watching::new(false, now);
        assert!(watching.asked_key_frame(true, false, now), "asked");
        assert_eq!(watching.key_frame_at(), Some(now + ASK_AGAIN), "and again while it waits");
        watching.want_key_frame(now + Duration::from_millis(100));
        assert_eq!(watching.key_frame_at(), Some(now + KEY_FRAME_EVERY), "not before the last ask had its time");
        assert!(!watching.asked_key_frame(false, false, now + KEY_FRAME_EVERY), "one came: nothing to ask");
        assert_eq!(watching.key_frame_at(), None);
    }

    #[test]
    fn a_screen_shown_nowhere_for_its_time_is_held_and_started_again_once_shown() {
        let start = Instant::now();
        let mut sight = Sight::default();
        assert!(!sight.seen(true, start), "shown, nothing to start");
        assert_eq!(sight.hold_at(), None, "shown, nothing to hold");
        assert!(!sight.seen(false, start), "unseen from now");
        assert!(!sight.seen(false, start + UNSEEN / 2), "unseen again, timed from the first");
        assert_eq!(sight.hold_at(), Some(start + UNSEEN));
        assert!(!sight.hold_now(start + UNSEEN / 2), "not yet");
        assert!(sight.hold_now(start + UNSEEN), "unseen its whole time: held");
        assert!(!sight.answered(true, start + UNSEEN), "the server held it");
        assert!(!sight.hold_now(start + UNSEEN * 2), "held once");
        assert_eq!(sight.hold_at(), None, "a held stream waits for nothing");
        assert!(sight.seen(true, start + UNSEEN * 2), "shown again: its decoder opens");
        assert!(!sight.seen(true, start + UNSEEN * 2), "once");
        assert_eq!(sight.opened(), Some(Reopened::StartAgain), "and once it opened, it is started");
        assert!(!sight.seen(false, start + UNSEEN * 3), "unseen anew");
        assert!(!sight.seen(true, start + UNSEEN * 3), "shown before its time: never held, nothing to start");
    }

    /// A sight held for being unseen, as the server answered the ask to hold it.
    fn held_unseen(start: Instant) -> Sight {
        let mut sight = Sight::default();
        sight.seen(false, start);
        assert!(sight.hold_now(start + UNSEEN));
        assert!(!sight.answered(true, start + UNSEEN));
        sight
    }

    #[test]
    fn a_screen_the_person_paused_is_not_started_by_being_shown_again() {
        let start = Instant::now();
        let mut sight = Sight::default();
        assert!(sight.pause(), "the server is asked to hold it");
        assert!(!sight.answered(true, start));
        assert!(sight.paused());
        assert!(!sight.seen(false, start), "unseen while paused");
        assert_eq!(sight.hold_at(), None, "held already, nothing to hold");
        assert!(!sight.seen(true, start + UNSEEN * 2), "shown again: still paused");
        assert!(sight.paused() && sight.held());
        assert!(sight.resume(), "only the person starts it");
        assert_eq!(sight.opened(), Some(Reopened::StartAgain));
        assert!(!sight.held());
    }

    #[test]
    fn a_screen_held_for_being_unseen_and_then_paused_waits_for_the_person() {
        let start = Instant::now();
        let mut sight = held_unseen(start);
        assert!(!sight.pause(), "held at the server already");
        assert!(sight.paused());
        assert!(!sight.seen(true, start + UNSEEN * 2));
        assert!(sight.resume());
    }

    #[test]
    fn a_pause_the_server_refused_leaves_the_screen_not_held() {
        let start = Instant::now();
        let mut sight = Sight::default();
        assert!(sight.pause());
        assert!(!sight.answered(false, start));
        assert!(!sight.held() && !sight.paused(), "refused: it plays");
        let mut unseen = Sight::default();
        unseen.seen(false, start);
        assert!(unseen.hold_now(start + UNSEEN));
        assert!(!unseen.answered(false, start + UNSEEN));
        assert!(!unseen.held());
        assert_eq!(unseen.hold_at(), Some(start + UNSEEN * 2), "asked again after its time, not at once");
    }

    #[test]
    fn a_screen_shown_while_its_hold_was_asked_is_started_when_the_hold_is_answered() {
        let start = Instant::now();
        let mut sight = Sight::default();
        sight.seen(false, start);
        assert!(sight.hold_now(start + UNSEEN));
        assert!(!sight.seen(true, start + UNSEEN), "the hold is not answered yet");
        assert!(sight.answered(true, start + UNSEEN), "held now and shown: started once its decoder opened");
        assert!(sight.held());
        assert_eq!(sight.opened(), Some(Reopened::StartAgain));
        assert!(!sight.held());
    }

    #[test]
    fn a_pause_undone_before_the_server_answered_holds_by_the_window_alone() {
        let start = Instant::now();
        let mut sight = Sight::default();
        assert!(sight.pause());
        assert!(!sight.resume(), "nothing held yet to start");
        assert!(!sight.paused());
        assert!(sight.answered(true, start), "shown, so it is started when held");
    }
}
