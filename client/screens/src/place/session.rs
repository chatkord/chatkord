//! The connection and the session: the door, a drop and its retries, signing out.

use super::*;

/// How the reconnect's last try went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Last {
    /// No try has come back yet.
    Untried,
    /// A try came back unanswered.
    Unanswered,
}

/// Where a reconnect stands between its wait, its try and what the person is shown of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Waiting for the next try, which starts at `due`.
    Waiting {
        /// When the try starts.
        due: Instant,
    },
    /// A try began at `began` and says nothing yet: the panel from before stays until it has run a moment.
    Quiet {
        /// When the try started.
        began: Instant,
    },
    /// A try has run long enough to be shown, and is held on screen for at least `CONNECTING_HELD`.
    Connecting {
        /// When it began to show.
        since: Instant,
    },
    /// A shown try failed before it was held long enough: it stays until `since` plus `CONNECTING_HELD`, then waits.
    Closing {
        /// When it began to show.
        since: Instant,
        /// The wait before the next try, from when it ends.
        delay: Duration,
    },
    /// The first open of a connection: shown at once, and its failure at once.
    Opening,
}

impl Phase {
    /// The next time this phase changes by itself, if it does.
    pub(super) fn edge(self) -> Option<Instant> {
        match self {
            Phase::Quiet { began } => Some(began + design::CONNECTING_AFTER),
            Phase::Closing { since, .. } => Some(since + design::CONNECTING_HELD),
            Phase::Waiting { .. } | Phase::Connecting { .. } | Phase::Opening => None,
        }
    }
}

/// A connection being made again by itself: at launch, or after a drop.
#[derive(Debug)]
pub struct Reconnecting {
    /// The channels as they were when the connection dropped; none at launch.
    pub(super) shown: Option<Lobby>,
    pub(super) was_in: Option<Channel>,
    pub(super) attempt: u32,
    pub(super) phase: Phase,
    pub(super) last: Last,
    pub(super) door: Door,
    /// Where the connection before stood in what the server tells, handed back once signed in; none at launch.
    pub(super) cursor: Option<terms::Cursor>,
    /// The text channel that was open, shown as it was and caught up once signed in again.
    pub(crate) discussion: Option<Discussion>,
    /// What was unread when the connection dropped, which the badge keeps showing, marked stale.
    pub(crate) unread: Unread,
}

/// The connection and the session: the door, a drop and its retries, the roles, signing out.
#[derive(Clone, Debug)]
pub enum SessionMessage {
    /// Something done on the sign-in, or what the host reports of it.
    Door(door::Message),
    /// The host found the connection gone, and how it ended.
    Disconnected(Ended),
    /// The host closed the connection because the call on it failed in this program: said once, then made again as
    /// after any connection that ended.
    CallFaulted,
    /// The connection dropped events it could not hold: the state follows whole, and the members list, which never
    /// does, is asked for again where it shows.
    Behind,
    /// The wait before reconnect try `n` is over.
    Reconnect(u32),
    /// "Try now" on a server that is not answering.
    TryNow,
    /// The server says the account's roles changed.
    Standing {
        /// Where it stands in the roles' order now.
        place: proto::rules::Place,
        /// The words it holds now.
        grants: Grants,
    },
    /// "Sign out instead" of choosing a new password.
    SignOutInstead,
    /// The server's answer to leaving it; the window's to read.
    LeaveAnswered(Result<(), LinkError>),
    /// "Trust the new certificate" on a server whose key changed.
    TrustNewKey,
    /// The host keeps the new key for the server now: its connection is made again with the session kept.
    KeyTrusted,
}

/// What an ended account leaves of the server's row on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The row stays, signed out, to sign in again.
    SignedOut,
    /// The row and everything kept for the server go.
    Forgotten,
}

impl Reconnecting {
    /// The remembered account at `server` opening by itself, and what its sign-in asks first.
    pub(super) fn opening(server: &Server, identity: String) -> (Reconnecting, Vec<Ask>) {
        let mut door = Door::opening(Opening::Account(server.clone(), identity));
        let todo = door.update(door::Message::Begin).into_iter().map(Ask::Door).collect();
        let again = Reconnecting {
            shown: None,
            was_in: None,
            attempt: 0,
            phase: Phase::Opening,
            last: Last::Untried,
            door,
            cursor: None,
            discussion: None,
            unread: Unread::default(),
        };
        (again, todo)
    }
}

impl Row {
    /// The row once the token is forgotten or not: one the keyring kept keeps the row, signed out, so the window shows
    /// it again and nothing is left behind unseen.
    pub fn after(self, token_forgotten: bool) -> Row {
        if token_forgotten { self } else { Row::SignedOut }
    }
}

impl Place {
    /// Whether a session of this account may live at the server: one open here, whatever the place shows over it, or
    /// one its kept token stands for while the connection is made again.
    fn holds_session(&self) -> bool {
        match &self.status {
            Status::Signed(_) | Status::Reconnecting(_) => true,
            Status::Door(_) => self.panel() == Panel::NewPassword,
        }
    }

    /// Signs this device out: the panel asks for the password, saying why, and the host ends the session.
    pub(crate) fn sign_out(&mut self) -> Vec<Ask> {
        // A temporary password's account is known by the sign-in that used it, not yet by the place.
        let typed = self.door().and_then(Door::target).map(|target| target.identity.clone());
        let Some(identity) = self.identity.clone().or(typed) else { return Vec::new() };
        let why = Sentence::SignedOutHere(self.server.shown().to_string());
        self.status = Status::Door(Box::new(Door::signed_out(self.server.clone(), identity.clone(), why)));
        let end = Ask::EndAccount {
            server: self.server.clone(),
            identity: Some(identity),
            sign_out: true,
            row: Row::SignedOut,
        };
        // The host lets go of this server's pictures as it ends the account, so its own is asked again after.
        std::iter::once(end).chain(self.forget_pictures()).collect()
    }

    /// What the host must do to take this server off the device.
    pub(crate) fn forget(&self) -> Ask {
        let (server, identity) = (self.server.clone(), self.identity.clone());
        Ask::EndAccount { server, identity, sign_out: self.holds_session(), row: Row::Forgotten }
    }

    /// The keyring kept the token a sign-out left: the door says so.
    pub(crate) fn token_stayed(&mut self) {
        if let Status::Door(door) = &mut self.status {
            door.say_why(Sentence::Text(words::Text::TokenLeft));
        }
    }

    /// The sign-in a person types into here, when there is one to show.
    pub fn door(&self) -> Option<&Door> {
        match &self.status {
            Status::Door(door) => Some(door.as_ref()),
            Status::Reconnecting(_) | Status::Signed(_) => None,
        }
    }

    /// The connection and the session, each in the state it belongs to.
    pub(super) fn on_session(&mut self, message: SessionMessage, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        match message {
            SessionMessage::Standing { place, grants } => {
                if let Some(standing) = self.standing.as_mut() {
                    if place.outranks(standing.place) {
                        *told = Some(Told::Raised);
                    }
                    (standing.place, standing.grants) = (place, grants);
                }
                // A page whose word was taken closes.
                if let Status::Signed(signed) = &mut self.status {
                    let offered = Page::offered(grants);
                    if signed
                        .page
                        .as_ref()
                        .and_then(OpenPage::settings)
                        .is_some_and(|open| !offered.contains(&open.page))
                    {
                        signed.page = None;
                    }
                    // What the account may do and see in each channel follows its roles: the list carries it again.
                    return vec![ops::list_channels()];
                }
                Vec::new()
            }
            SessionMessage::SignOutInstead => {
                let Status::Door(door) = &mut self.status else { return Vec::new() };
                if door.step() != Step::NewPassword {
                    return Vec::new();
                }
                self.sign_out()
            }
            SessionMessage::Door(message) => match &mut self.status {
                Status::Door(door) => {
                    let todo: Vec<Ask> = door.update(message).into_iter().map(Ask::Door).collect();
                    self.after_door(todo)
                }
                Status::Reconnecting(again) => {
                    let todo = again.door.update(message).into_iter().map(Ask::Door).collect();
                    self.settle(todo, at)
                }
                Status::Signed(_) => Vec::new(),
            },
            SessionMessage::Disconnected(ended) => {
                // A server that closed on purpose says why in words; only a connection lost sounds, as Sharkord's.
                let sounded = retryable(&ended).then_some(Told::Chime(Chime::Disconnected));
                self.signed_dropped(&ended, sounded, told, at)
            }
            SessionMessage::CallFaulted => {
                let said = Telling::of(design::Status::Danger, Sentence::Phrase(Phrase::CallFaulted));
                self.signed_dropped(&Ended::default(), Some(Told::Toast(said)), told, at)
            }
            SessionMessage::Reconnect(attempt) => {
                let Status::Reconnecting(again) = &mut self.status else { return Vec::new() };
                if !matches!(again.phase, Phase::Waiting { .. }) || attempt != again.attempt {
                    return Vec::new();
                }
                again.phase = Phase::Quiet { began: at.now };
                let next = match again.door.showing() {
                    Showing::Retrying => door::Message::Retry,
                    Showing::Form | Showing::Working | Showing::Done => door::Message::Begin,
                };
                let todo = again.door.update(next).into_iter().map(Ask::Door).collect();
                self.settle(todo, at)
            }
            SessionMessage::TryNow => {
                let Status::Reconnecting(again) = &mut self.status else { return Vec::new() };
                if !matches!(again.phase, Phase::Waiting { .. }) {
                    return Vec::new();
                }
                // A timer still out for the old try finds another attempt and is dropped.
                again.attempt += 1;
                again.phase = Phase::Quiet { began: at.now };
                let todo = again.door.update(door::Message::Retry).into_iter().map(Ask::Door).collect();
                self.settle(todo, at)
            }
            SessionMessage::Behind => {
                let Status::Signed(signed) = &self.status else { return Vec::new() };
                // Events were dropped: the list is read again, as it is told only by change, and the open channel asks
                // what changed since what it read.
                let mut todo = vec![ops::list_channels()];
                todo.extend(signed.content.discussions().filter_map(catch_up));
                todo.extend(self.talk_reads());
                if self.showing(Page::Members) {
                    todo.push(Ask::ListMembers);
                }
                todo.push(self.list_people());
                todo.push(ops::list_roles());
                todo.push(self.emoji.read_again());
                todo.push(ops::get_notify());
                todo
            }
            SessionMessage::TrustNewKey => match self.door().and_then(Door::problem) {
                Some(door::Problem::KeyChanged { presented, .. }) => {
                    vec![Ask::TrustKey { server: self.server.url.clone(), pin: *presented }]
                }
                Some(_) | None => Vec::new(),
            },
            SessionMessage::KeyTrusted => {
                if self.panel() != Panel::KeyChanged {
                    return Vec::new();
                }
                match self.identity.clone() {
                    Some(identity) => {
                        let (again, todo) = Reconnecting::opening(&self.server, identity);
                        self.status = Status::Reconnecting(Box::new(again));
                        self.settle(todo, at)
                    }
                    None => {
                        self.status = Status::Door(Box::new(Door::opening(Opening::Server(self.server.clone()))));
                        Vec::new()
                    }
                }
            }
            // The answers the window reads before handing them on.
            SessionMessage::LeaveAnswered(_) => Vec::new(),
        }
    }

    /// A door that signed in goes to the channels.
    pub(super) fn after_door(&mut self, mut todo: Vec<Ask>) -> Vec<Ask> {
        let Status::Door(door) = &mut self.status else { return todo };
        if !door.signed_in() {
            return todo;
        }
        let door = std::mem::take(door);
        self.signed_by(&door);
        todo.extend(self.first_reads());
        todo
    }

    /// The account `door` signed in to is this place's now, with what the server said of it.
    pub(crate) fn signed_by(&mut self, door: &Door) {
        self.identity = door.target().map(|target| target.identity.clone());
        self.standing = door.standing();
        self.status = Status::Signed(Box::new(SignedIn::new(door.arrival(), None, door.token())));
        self.search = Search::default();
    }

    /// The place as the add dialog checks a server being added against it: an invite to one where this device holds no
    /// account goes on to make one, here.
    pub(crate) fn listed(&self) -> door::Listed {
        let server = self.server.clone();
        match self.panel() {
            Panel::SignedOut | Panel::Deleted => door::Listed::Unheld(server),
            Panel::Channels
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Call
            | Panel::Settings
            | Panel::Account
            | Panel::Lost
            | Panel::Connecting
            | Panel::NotAnswering { .. }
            | Panel::KeyChanged
            | Panel::NewPassword => door::Listed::Held(server),
        }
    }

    /// What a place signed in through a door reads first: the channel list, the people the window names authors by,
    /// the roles their popovers name, the server's emoji, and the account's notification settings.
    pub(crate) fn first_reads(&mut self) -> Vec<Ask> {
        let mut reads = vec![
            ops::list_channels(),
            self.list_people(),
            ops::list_roles(),
            self.emoji.read_again(),
            ops::get_notify(),
        ];
        reads.extend(self.talk_reads());
        reads
    }

    /// The signed-in connection gone as `ended` says, telling `said` of it; nothing where none was signed in.
    fn signed_dropped(&mut self, ended: &Ended, said: Option<Told>, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let (shown, was_in) = (signed.lobby.clone(), signed.call.as_ref().map(|call| call.channel.clone()));
        let reading = std::mem::take(&mut signed.content).into_discussions().next();
        let held = signed.token.take();
        self.device = None;
        self.activity_told = None;
        *told = said;
        self.dropped(Some(shown), was_in, reading, held, ended, at)
    }

    /// A connection that may come back is tried again by itself after a wait, offering `held`, the token the session
    /// signed in with, first; one whose session is over asks for the password, saying why, and lets `held` go.
    pub(super) fn dropped(
        &mut self,
        shown: Option<Lobby>,
        was_in: Option<Channel>,
        discussion: Option<Discussion>,
        held: Option<Secret>,
        ended: &Ended,
        at: Moment,
    ) -> Vec<Ask> {
        let unread = self.unread(at.wall).map(|held| held.unread).unwrap_or_default();
        let over = (!retryable(ended))
            .then(|| Sentence::Ended { server: self.server.shown().to_string(), reason: ended.reason });
        let door = match (self.identity.clone(), over) {
            (Some(identity), None) => Door::resuming(self.server.clone(), identity, held),
            (Some(identity), Some(why)) => {
                self.status = Status::Door(Box::new(Door::signed_out(self.server.clone(), identity, why)));
                return Vec::new();
            }
            // No account is known here to sign in as again: who signs in is the person's to say, never an empty name.
            (None, over) => {
                let mut door = Door::opening(Opening::Server(self.server.clone()));
                if let Some(why) = over {
                    door.say_why(why);
                }
                self.status = Status::Door(Box::new(door));
                return Vec::new();
            }
        };
        let delay = BACKOFF.delay(1, at.jitter);
        let phase = Phase::Waiting { due: at.now + delay };
        let cursor = ended.cursor.clone();
        let again =
            Reconnecting { shown, was_in, attempt: 1, phase, last: Last::Untried, door, cursor, discussion, unread };
        self.status = Status::Reconnecting(Box::new(again));
        vec![Ask::Wait { attempt: 1, delay }]
    }

    /// What time alone changes in a reconnect: a try that has run a moment shows, and a shown one that failed in the
    /// meantime lets go once it has been held, starting its wait.
    pub(crate) fn passed(&mut self, now: Instant) -> Vec<Ask> {
        let Status::Reconnecting(again) = &mut self.status else { return Vec::new() };
        match again.phase {
            Phase::Quiet { began } if now >= began + design::CONNECTING_AFTER => {
                again.phase = Phase::Connecting { since: now };
                Vec::new()
            }
            Phase::Closing { since, delay } if now >= since + design::CONNECTING_HELD => {
                again.phase = Phase::Waiting { due: now + delay };
                vec![Ask::Wait { attempt: again.attempt, delay }]
            }
            Phase::Waiting { .. }
            | Phase::Quiet { .. }
            | Phase::Connecting { .. }
            | Phase::Closing { .. }
            | Phase::Opening => Vec::new(),
        }
    }

    /// Where a try left the sign-in: signed in goes to the channels (never back into a call, which is offered
    /// instead) and asks what it missed when the server offers that, unanswered waits for the next try, and a refusal
    /// is the person's to answer.
    pub(super) fn settle(&mut self, mut todo: Vec<Ask>, at: Moment) -> Vec<Ask> {
        let Status::Reconnecting(again) = &mut self.status else { return todo };
        match again.door.showing() {
            Showing::Working => {}
            Showing::Done => {
                self.standing = again.door.standing();
                let (back, cursor, discussion) = (again.was_in.take(), again.cursor.take(), again.discussion.take());
                let mut signed = SignedIn::new(None, back, again.door.token());
                todo.push(ops::list_channels());
                if let Some(cursor) = cursor {
                    todo.push(Ask::Resume(cursor));
                }
                todo.extend(discussion.as_ref().and_then(catch_up));
                todo.extend(self.talk_reads());
                // Who arrived or left while away is read again whole: what the ring tells whole leaves them out; and
                // the roles, which a role changed meanwhile may have changed.
                todo.push(self.list_people());
                todo.push(ops::list_roles());
                todo.push(self.emoji.read_again());
                todo.push(ops::get_notify());
                signed.content = discussion.map_or(Content::Call, Content::seated);
                self.status = Status::Signed(Box::new(signed));
                self.search = Search::default();
            }
            Showing::Retrying => {
                again.attempt += 1;
                let delay = BACKOFF.delay(again.attempt, at.jitter);
                let held = match (again.phase, again.last, &again.shown) {
                    // Shown as the lost notice, never as connecting: there is nothing to hold.
                    (_, Last::Untried, Some(_)) => None,
                    (Phase::Connecting { since }, _, _) if at.now < since + design::CONNECTING_HELD => Some(since),
                    (
                        Phase::Waiting { .. }
                        | Phase::Quiet { .. }
                        | Phase::Connecting { .. }
                        | Phase::Closing { .. }
                        | Phase::Opening,
                        _,
                        _,
                    ) => None,
                };
                again.last = Last::Unanswered;
                match held {
                    Some(since) => again.phase = Phase::Closing { since, delay },
                    None => {
                        again.phase = Phase::Waiting { due: at.now + delay };
                        todo.push(Ask::Wait { attempt: again.attempt, delay });
                    }
                }
            }
            Showing::Form => {
                let door = std::mem::take(&mut again.door);
                self.status = Status::Door(Box::new(door));
            }
        }
        todo
    }
}

/// What an open channel asks for after events were missed: the changes since what it read, or its first page again.
fn catch_up(open: &Discussion) -> Option<Ask> {
    let timeline = open.timeline_id()?.clone();
    Some(match open.timeline() {
        Some(read) => ops::catch_up(timeline, read.revision()),
        None => ops::history(timeline, terms::Anchor::Newest, None),
    })
}

pub(super) fn deleted(door: &Door) -> bool {
    matches!(door.why(), Some(Sentence::Ended { reason: Some(reason), .. })
        if Then::of(*reason) == Then::AccountGone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::{self, Errand, Heard, Opening};
    use crate::harness::{ADA, Clock, accepted, books, flow, id, owls, standing_as, tell, token, two_signed_in};
    use crate::place;
    use crate::sign_in::{Report, Standing};
    use crate::window::{Window, whole_seconds};
    use proto::chatkord_v1::CloseReason;
    use proto::rules::Grants;
    use std::time::{Duration, Instant};
    use terms::{Ended, Secret};
    use words::Sentence;

    #[test]
    fn a_lost_connection_whose_long_try_fails_is_not_held_as_connecting() {
        let mut clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let lost = Ended { silent: true, ..Ended::default() };
        tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::Disconnected(lost)));
        tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::Reconnect(1)));
        clock.pass(design::CONNECTING_AFTER);
        window.update(crate::WindowMessage::Tick, clock.at());
        assert_eq!(window.places()[0].panel(), Panel::Lost, "the lost notice says it is reconnecting");
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Timeout)));
        assert!(todo.iter().any(|done| matches!(done, crate::Ask::At(_, place::Ask::Wait { .. }))), "{todo:?}");
        assert!(matches!(window.places()[0].panel(), Panel::NotAnswering { .. }), "{:?}", window.places()[0].panel());
    }

    #[test]
    fn a_call_that_faulted_says_so_once_and_its_connection_is_made_again() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::CallFaulted));
        assert!(
            todo.iter().any(|done| matches!(done, crate::Ask::At(_, place::Ask::Wait { attempt: 1, .. }))),
            "{todo:?}"
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("This call stopped after a fault in Chatkord; reconnecting to the server.")
        );
        let again = tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::CallFaulted));
        assert!(again.is_empty(), "a place already reconnecting is not dropped again: {again:?}");
    }

    #[test]
    fn a_dropped_session_with_no_account_known_asks_the_person_and_signs_in_as_no_one() {
        let (mut place, _) = Place::remembered(PlaceId(1), owls(), None);
        place.status = Status::Signed(Box::new(SignedIn::new(None, None, None)));
        let at = Clock(Instant::now()).at();
        let lost = Ended { silent: true, ..Ended::default() };
        let (todo, _) = place.update(Message::Session(SessionMessage::Disconnected(lost)), at);
        assert!(todo.is_empty(), "nothing is tried in an empty name: {todo:?}");
        let door = place.door().expect("the sign-in, for the person to say who");
        assert_eq!((door.target(), door.username()), (None, ""));
        assert_eq!(place.panel(), Panel::SignedOut);
    }

    #[test]
    fn a_connection_signed_in_again_resumes_where_it_stood() {
        {
            let clock = Clock(Instant::now());
            let mut window = two_signed_in(&clock);
            let owls_id = id(&window, 0);
            let told = proto::chatkord_v1::Event { cursor: vec![7; 36], kind: None };
            let cursor = terms::Cursor::of(&told).expect("a place");
            let ended = Ended { silent: true, cursor: Some(cursor.clone()), ..Ended::default() };
            tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Disconnected(ended)));
            tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1)));
            let todo = tell(&mut window, &clock, owls_id, flow(accepted(Grants::default())));
            let resumed =
                todo.iter().any(|done| matches!(done, crate::Ask::At(_, place::Ask::Resume(from)) if *from == cursor));
            assert!(resumed, "{todo:?}");
            assert!(todo.iter().any(|done| matches!(done, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ChannelsList))), "{todo:?}");
        }
    }

    #[test]
    fn an_ending_that_is_over_asks_for_the_password_and_a_deleted_account_says_so() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        let ended = |reason| {
            place::Message::Session(place::SessionMessage::Disconnected(Ended {
                reason: Some(reason),
                ..Ended::default()
            }))
        };
        assert!(tell(&mut window, &clock, owls_id, ended(CloseReason::PasswordReset)).is_empty());
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.rail()), (Panel::SignedOut, RailState::SignedOut));
        let door = place.door().unwrap();
        assert_eq!(door.username(), ADA, "the username is kept");
        assert_eq!(
            door.why().unwrap().in_english(),
            "An admin reset your password. Sign in with the temporary password they gave you."
        );
        tell(&mut window, &clock, books_id, ended(CloseReason::AccountDeleted));
        assert_eq!(window.places()[1].panel(), Panel::Deleted);
    }

    #[test]
    fn a_session_ended_banned_is_not_tried_again_and_says_so() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let banned = Ended { reason: Some(CloseReason::Banned), ..Ended::default() };
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Disconnected(banned)));
        assert!(todo.is_empty(), "no try waits: {todo:?}");
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.rail()), (Panel::SignedOut, RailState::SignedOut));
        let door = place.door().expect("the sign-in");
        assert_eq!(door.username(), ADA, "the username is kept for when they are unbanned");
        assert_eq!(
            door.why().map(Sentence::in_english).as_deref(),
            Some("An admin banned this account from this server.")
        );
    }

    #[test]
    fn signing_in_on_a_signed_out_panel_opens_its_channels_with_no_toast() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Server(books())], None, clock.0);
        let books_id = id(&window, 0);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        tell(&mut window, &clock, books_id, door(door::Message::Username(ADA.into())));
        tell(
            &mut window,
            &clock,
            books_id,
            door(door::Message::Password(Secret::new("evening games with friends".into()))),
        );
        let todo = tell(&mut window, &clock, books_id, door(door::Message::Submit));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::SubmitPassword(..))))]),
            "{todo:?}"
        );
        let todo = tell(&mut window, &clock, books_id, flow(accepted(Grants::default())));
        assert!(
            matches!(
                &todo[..],
                [
                    crate::Ask::At(_, ask1),
                    crate::Ask::At(_, ask2),
                    crate::Ask::At(_, ask3),
                    crate::Ask::At(_, ask4),
                    crate::Ask::At(_, ask5),
                    crate::Ask::Idle(crate::window::IdleWatch::Start(_))
                ]
             if crate::harness::is(ask1, crate::harness::Op::ChannelsList) && crate::harness::is(ask2, crate::harness::Op::ListPeople) && crate::harness::is(ask3, crate::harness::Op::ListRoles) && crate::harness::is(ask4, crate::harness::Op::ListEmoji) && crate::harness::is(ask5, crate::harness::Op::GetNotify)),
            "{todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::Channels);
        assert_eq!(window.places()[0].identity(), Some(ADA));
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the channels open: that is the answer"
        );
        assert_eq!(whole_seconds(Duration::from_millis(1)), 1);
    }

    #[test]
    fn a_temporary_password_replaced_opens_the_channels_with_no_toast() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Server(books())], None, clock.0);
        let books_id = id(&window, 0);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        let typed = |text: &str| Secret::new(text.into());
        tell(&mut window, &clock, books_id, door(door::Message::Username(ADA.into())));
        tell(&mut window, &clock, books_id, door(door::Message::Password(typed("river-copper-lamp-42"))));
        tell(&mut window, &clock, books_id, door(door::Message::Submit));
        let temporary = Standing { must_change: true, ..standing_as("ada") };
        tell(&mut window, &clock, books_id, flow(Report::Accepted { token: None, named: None, standing: temporary }));
        assert_eq!(window.places()[0].panel(), Panel::NewPassword);
        tell(&mut window, &clock, books_id, door(door::Message::Password(typed("riverside lanterns at dusk"))));
        tell(&mut window, &clock, books_id, door(door::Message::Again(typed("riverside lanterns at dusk"))));
        tell(&mut window, &clock, books_id, door(door::Message::Submit));
        tell(&mut window, &clock, books_id, door(door::Message::Reported(Heard::PasswordSaved(Ok(())))));
        // The change ended every session, so the new password signs in again before the channels.
        tell(&mut window, &clock, books_id, flow(accepted(Grants::default())));
        assert_eq!(window.places()[0].panel(), Panel::Channels);
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the channels open: that is the answer"
        );
    }

    #[test]
    fn a_token_the_keyring_kept_leaves_the_row_signed_out_whatever_was_asked() {
        assert_eq!(place::Row::Forgotten.after(true), place::Row::Forgotten);
        assert_eq!(place::Row::SignedOut.after(true), place::Row::SignedOut);
        assert_eq!(place::Row::Forgotten.after(false), place::Row::SignedOut);
        assert_eq!(place::Row::SignedOut.after(false), place::Row::SignedOut);
    }
    #[test]
    fn a_key_that_changed_on_reconnecting_keeps_the_session_until_the_new_key_is_trusted() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let pin = |byte| proto::pin::KeyPin::from_digest([byte; proto::pin::KeyPin::LEN]);
        let session = |message| place::Message::Session(message);
        tell(&mut window, &clock, owls_id, session(SessionMessage::Disconnected(Ended::default())));
        tell(&mut window, &clock, owls_id, session(SessionMessage::Reconnect(1)));
        let changed = LinkError::KeyChanged { trusted: pin(1), presented: pin(2) };
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Unreached(changed)));
        assert!(todo.is_empty(), "no token forgotten and no try again: {todo:?}");
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.rail(), place.shows_pane()), (Panel::KeyChanged, RailState::KeyChanged, true));
        let todo = tell(&mut window, &clock, owls_id, session(SessionMessage::TrustNewKey));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::TrustKey { server, pin: trusted })]
                if *server == owls().url && *trusted == pin(2)),
            "the key it presented: {todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::KeyChanged, "until the host keeps it");
        let todo = tell(&mut window, &clock, owls_id, session(SessionMessage::KeyTrusted));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::LoadToken(target))))]
                if target.identity == ADA),
            "the kept session signs in again: {todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::Connecting);
        assert!(tell(&mut window, &clock, owls_id, session(SessionMessage::KeyTrusted)).is_empty(), "said once");
    }

    #[test]
    fn a_server_whose_key_changed_is_removed_from_its_panel_without_ending_its_session_there() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let pin = |byte| proto::pin::KeyPin::from_digest([byte; proto::pin::KeyPin::LEN]);
        tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::Disconnected(Ended::default())));
        tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::Reconnect(1)));
        let changed = LinkError::KeyChanged { trusted: pin(1), presented: pin(2) };
        tell(&mut window, &clock, owls_id, flow(Report::Unreached(changed)));
        let todo = window.update(crate::WindowMessage::Remove(owls_id), clock.at());
        assert!(
            matches!(&todo[0], crate::Ask::At(_, place::Ask::EndAccount { sign_out: false, row: Row::Forgotten, .. })),
            "the token never goes to a key not trusted: {todo:?}"
        );
    }

    fn drop_and_reconnect(window: &mut Window, clock: &Clock, owls_id: PlaceId) -> Vec<crate::Ask> {
        let lost = Ended { silent: true, ..Ended::default() };
        tell(window, clock, owls_id, place::Message::Session(SessionMessage::Disconnected(lost)));
        tell(window, clock, owls_id, place::Message::Session(SessionMessage::Reconnect(1)))
    }

    /// Whether `todo` tries `token` as Ada, once, beside nothing but the try's own deadline.
    fn offers_token(todo: &[crate::Ask], token: &str) -> bool {
        let tries = |ask: &crate::Ask| {
            matches!(ask, crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::TryToken(target, held))))
                if held.expose() == token && target.identity == ADA)
        };
        todo.iter().filter(|ask| tries(ask)).count() == 1
            && todo.iter().all(|ask| tries(ask) || matches!(ask, crate::Ask::WakeAt(_)))
    }

    #[test]
    fn a_reconnect_after_a_drop_offers_the_token_the_session_signed_in_with_before_the_keyring() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        let todo = drop_and_reconnect(&mut window, &clock, owls_id);
        assert!(offers_token(&todo, "t"), "the keyring is not read again: {todo:?}");
    }

    #[test]
    fn a_reconnect_offers_the_token_the_server_issued_in_place_of_the_one_it_was_given() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        tell(&mut window, &clock, owls_id, flow(token()));
        let issued =
            Report::Accepted { token: Some(Secret::new("issued".into())), named: None, standing: standing_as("ada") };
        tell(&mut window, &clock, owls_id, flow(issued));
        tell(&mut window, &clock, owls_id, flow(Report::Stored(false)));
        let todo = drop_and_reconnect(&mut window, &clock, owls_id);
        assert!(offers_token(&todo, "issued"), "held whether or not the keyring kept it: {todo:?}");
    }

    #[test]
    fn a_held_token_the_server_turns_away_falls_back_to_the_keyring_then_the_password() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        drop_and_reconnect(&mut window, &clock, owls_id);
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Expired(None)));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::LoadToken(_))))]),
            "the keyring is read next: {todo:?}"
        );
        let newer = Report::TokenLoaded(Some(Secret::new("newer".into())));
        let todo = tell(&mut window, &clock, owls_id, flow(newer));
        assert!(offers_token(&todo, "newer"), "{todo:?}");
        let todo = tell(&mut window, &clock, owls_id, flow(Report::Expired(None)));
        assert!(
            todo.iter()
                .any(|done| matches!(done, crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::ForgetToken(_)))))),
            "{todo:?}"
        );
        assert!(window.places()[0].door().is_some(), "the password is asked for last");
    }

    #[test]
    fn a_reconnect_the_server_did_not_answer_offers_the_same_token_again() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        drop_and_reconnect(&mut window, &clock, owls_id);
        tell(&mut window, &clock, owls_id, flow(Report::Unreached(LinkError::Connect)));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(SessionMessage::TryNow));
        assert!(offers_token(&todo, "t"), "{todo:?}");
    }
}
