//! How a call ends: its end card, the way back, and what the place is told when its loop returns.

use super::*;
use crate::place::ops;

/// What a call that ended says: a title, why, and whether the seat can be taken back from where it went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndCard {
    /// What the end card says first.
    pub title: Sentence,
    /// Why the call ended, in a sentence.
    pub body: Sentence,
    /// The same channel again, when the end allows it; none when only the way out is offered.
    pub way_back: Option<WayBack>,
}

/// What a call that ended offers beside the way out to the channels: the same channel again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WayBack {
    /// The account joined from another device: take the seat back.
    SwitchHere,
    /// An administrator took the account out: nothing bars it, so it may join again.
    JoinAgain,
}

impl WayBack {
    /// The words of the button that offers it.
    pub fn phrase(self) -> Phrase {
        match self {
            WayBack::SwitchHere => Phrase::SwitchHere,
            WayBack::JoinAgain => Phrase::JoinAgain,
        }
    }
}

impl Call {
    /// Why the call is over, once it is.
    pub fn ended(&self) -> Option<&End> {
        self.room.ended()
    }

    /// Why the call is over, to offer what fits beside the sentence.
    pub fn ended_why(&self) -> Option<Why> {
        self.room.ended_why()
    }

    /// What a call that ended says to a person holding `grants`, to draw beside the way back.
    pub fn end_card(&self, grants: Grants) -> Option<EndCard> {
        let (title, body, way_back) = self.end_words(grants)?;
        Some(EndCard { title, body, way_back })
    }

    /// The end card's title and body to a person holding `grants`, and its way back.
    fn end_words(&self, grants: Grants) -> Option<(Sentence, Sentence, Option<WayBack>)> {
        let end = self.ended()?;
        let why = end.why();
        let name = self.channel.name.clone();
        let (title, body) = match why {
            Why::Deleted => (Sentence::ChannelWasDeleted(name), Sentence::Phrase(Phrase::DeletedBody)),
            Why::Failed => (Sentence::CouldNotJoin(name), end.said_to(grants)),
            Why::TakenOut => (Sentence::OutOf(name.clone()), Sentence::TakenOutOf(name)),
            Why::Moved | Why::RolesChanged | Why::Closed | Why::FellBehind => {
                (Sentence::YouLeft(name), end.said_to(grants))
            }
        };
        let way_back = match why {
            Why::Moved => Some(WayBack::SwitchHere),
            Why::TakenOut => Some(WayBack::JoinAgain),
            Why::Deleted | Why::RolesChanged | Why::Closed | Why::Failed | Why::FellBehind => None,
        };
        Some((title, body, way_back))
    }
}

impl Place {
    /// What a call that ended here tells where its own view is not on screen, offering the card's way back: when the
    /// server is `elsewhere`, a take-out names it too. Nothing while the call is being put away.
    pub(crate) fn ended_telling(&self, elsewhere: bool) -> Option<Telling> {
        let call = self.call().filter(|call| !call.leaving())?;
        let (_, body, way_back) = call.end_words(self.grants())?;
        let kind = if call.ended_why() == Some(Why::Failed) { design::Status::Warning } else { design::Status::Info };
        let body = match call.ended_why() {
            Some(Why::TakenOut) if elsewhere => {
                Sentence::TakenOutOn { channel: call.channel.name.clone(), server: self.server.shown().to_string() }
            }
            _ => body,
        };
        let telling = Telling::of(kind, body);
        Some(match way_back {
            Some(way) => telling.offering(Action::Again(way)),
            None => telling,
        })
    }

    pub(super) fn leave_asked(&mut self, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_mut() {
            // The loop already returned: nothing is left to leave.
            Some(call) if !call.running() => {
                let mut todo = signed.end_call(&mut self.held, self.zone.clone());
                todo.push(ops::list_channels());
                todo
            }
            // It ended by itself and is winding down: it is over in a moment, and the call goes then.
            Some(call) if !call.live() => {
                call.ending = Ending::Leaving;
                Vec::new()
            }
            Some(call) if !call.leaving() => {
                call.ending = Ending::Leaving;
                *told = Some(Told::Chime(Chime::Left));
                vec![Ask::Leave]
            }
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn call_over(&mut self, end: Option<End>, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let mut todo = Vec::new();
        if let Some(call) = signed.call.as_mut() {
            if let Some(end) = end
                && call.ended().is_none()
            {
                call.room.on(RoomEvent::Ended(end), at.now);
            }
            let leaving = call.leaving();
            call.ending = Ending::Over;
            // A call that ended by itself stays, to say so, until a person puts it away.
            if call.live() || leaving {
                todo = signed.end_call(&mut self.held, self.zone.clone());
            }
        }
        todo.push(ops::list_channels());
        todo
    }
}

/// How a call's loop came back, as its host saw it; [`CallReturn::told`] is what the place is told.
#[derive(Debug)]
pub enum CallReturn {
    /// The connection ended during the call, as it ended: the place makes it again.
    Disconnected(Ended),
    /// The loop returned by itself or because it was left.
    Returned,
    /// The host left it because the window fell too far behind the room to say what it became.
    FellBehind,
    /// The loop stopped with the end it reported last, or could not begin, saying why.
    Stopped(Option<End>),
    /// The loop or a step of it panicked, so its connection was closed: the place says so and makes it again.
    Faulted,
}

impl CallReturn {
    /// The message that tells the place how its call came back.
    pub fn told(self) -> Message {
        match self {
            Self::Disconnected(ended) => Message::Session(SessionMessage::Disconnected(ended)),
            Self::Returned => Message::Call(CallMessage::CallOver(None)),
            Self::FellBehind => Message::Call(CallMessage::CallOver(Some(End::FellBehind))),
            Self::Stopped(end) => Message::Call(CallMessage::CallOver(end)),
            Self::Faulted => Message::Session(SessionMessage::CallFaulted),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{End, Event as RoomEvent, Why};
    use crate::chime::Chime;
    use crate::harness::{Clock, ada_in_call, id, in_call, moved, tell, two_signed_in};
    use crate::place;
    use proto::chatkord_v1::CloseReason;
    use std::time::Instant;

    #[test]
    fn leaving_asks_once_and_a_call_that_ended_goes_back_without_asking() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        assert_eq!(window.places()[0].call().unwrap().tiles().len(), 1, "the room's events make the tiles");
        assert!(matches!(
            &tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Leave))[..],
            [crate::Ask::At(_, place::Ask::Leave), crate::Ask::Chime(Chime::Left)]
        ));
        assert!(
            tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Leave)).is_empty(),
            "a second press asks nothing"
        );
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        in_call(&mut window, &clock, owls_id);
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(End::Closed(None)))),
        );
        assert_eq!(window.places()[0].call().unwrap().ended(), Some(&End::Closed(None)));
        tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::CallOver(None)));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::Leave));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, ask1), crate::Ask::At(_, ask2)]
             if crate::harness::is(ask1, crate::harness::Op::History) && crate::harness::is(ask2, crate::harness::Op::ChannelsList)),
            "nothing is left to leave, and the first text channel opens: {todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::Discussion);
    }

    /// A quick "Switch to this device": the old loop's late end and seat are not the new call's.
    #[test]
    fn switching_back_waits_for_the_old_loop_and_the_late_news_is_not_the_new_calls() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        tell(&mut window, &clock, place, moved());
        assert_eq!(window.places()[0].call().unwrap().ended_why(), Some(Why::Moved));

        let todo = tell(
            &mut window,
            &clock,
            place,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert!(todo.is_empty(), "the old loop has not returned, so nothing starts yet: {todo:?}");

        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        let joins = todo.iter().filter(
            |todo| matches!(todo, crate::Ask::At(_, place::Ask::Join { channel_id, .. }) if channel_id.as_str() == "lounge"),
        );
        assert_eq!(joins.count(), 1, "the switch starts once the old call is over: {todo:?}");
        let call = window.places()[0].call().unwrap();
        assert!(call.live() && call.running() && call.ended().is_none(), "a new, clean call");
        assert_eq!(call.tiles().len(), 0, "the new call waits for its own room");
    }

    #[test]
    fn a_call_that_ended_says_so_in_a_card_and_only_a_move_offers_the_seat_back() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        assert_eq!(window.places()[0].call().unwrap().end_card(Grants::NONE), None, "a live call has none");
        tell(&mut window, &clock, place, moved());
        let card = window.places()[0].call().unwrap().end_card(Grants::NONE).unwrap();
        assert_eq!(card.title.in_english(), "You left Lounge");
        assert_eq!(card.body.in_english(), "This account joined voice from another device.");
        assert_eq!(card.way_back, Some(place::WayBack::SwitchHere));

        let (mut window, place) = ada_in_call(&clock);
        let deleted = End::Closed(Some(CloseReason::ChannelDeleted));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(deleted))));
        let card = window.places()[0].call().unwrap().end_card(Grants::NONE).unwrap();
        assert_eq!(card.title.in_english(), "Lounge was deleted");
        assert_eq!(
            card.body.in_english(),
            "You left the call when the channel was deleted.",
            "the window's words, not the link's"
        );
        assert_eq!(card.way_back, None);

        let (mut window, place) = ada_in_call(&clock);
        let roles = End::Closed(Some(CloseReason::NotAllowed));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(roles))));
        let card = window.places()[0].call().unwrap().end_card(Grants::NONE).unwrap();
        assert_eq!((card.title.in_english().as_str(), card.way_back), ("You left Lounge", None));
    }

    #[test]
    fn a_call_that_ended_goes_when_it_is_put_away_and_not_before_its_loop_returns() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        tell(&mut window, &clock, place, moved());
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Leave));
        assert!(todo.is_empty(), "the loop returns by itself: {todo:?}");
        assert!(window.places()[0].call().is_some(), "still held until it has");
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, ask1), crate::Ask::At(_, ask2)]
             if crate::harness::is(ask1, crate::harness::Op::History) && crate::harness::is(ask2, crate::harness::Op::ChannelsList)),
            "{todo:?}"
        );
        assert!(window.places()[0].call().is_none());
    }

    #[test]
    fn being_taken_out_says_who_did_it_and_offers_to_join_again() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        let card = window.places()[0].call().unwrap().end_card(Grants::NONE).unwrap();
        assert_eq!(card.title.in_english(), "You’re out of Lounge");
        assert_eq!(card.body.in_english(), "An admin took you out of Lounge.");
        assert_eq!(card.way_back, Some(place::WayBack::JoinAgain), "nothing bars it");
    }

    #[test]
    fn a_call_that_ends_under_a_settings_page_is_told_in_a_toast_that_offers_the_way_back() {
        use crate::Action;
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        window.update(crate::WindowMessage::OpenDevice(crate::DevicePage::Appearance), clock.at());
        assert!(window.toast().is_none(), "a live call says nothing on the page");
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        let toast = window.toast().expect("told over the page");
        assert_eq!(toast.said().in_english(), "An admin took you out of Lounge.");
        assert_eq!((toast.kind(), toast.closable()), (design::Status::Info, true));
        assert_eq!(toast.action(), Some(&Action::Again(place::WayBack::JoinAgain)), "the card's own way back");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        let todo = window.update(crate::WindowMessage::ToastAct, clock.at());
        assert!(todo.iter().any(|todo| matches!(todo, crate::Ask::At(_, place::Ask::Join { .. }))), "{todo:?}");

        let (mut window, place) = ada_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Page(place::PageMessage::OpenAccount));
        let gone = End::Closed(Some(CloseReason::ChannelDeleted));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(gone))));
        assert_eq!(window.toast().expect("told").action(), None, "a deleted channel offers no way back");
        assert_eq!(window.places()[0].panel(), Panel::Account, "the page stays");

        let (mut window, place) = ada_in_call(&clock);
        let out = End::Closed(Some(CloseReason::TakenOut));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(RoomEvent::Ended(out))));
        assert!(window.toast().is_none(), "the call's own view shows its card: nothing is told twice");
    }

    #[test]
    fn how_a_call_came_back_is_what_the_place_is_told() {
        let over = |returned: place::CallReturn| match returned.told() {
            place::Message::Call(place::CallMessage::CallOver(end)) => Some(end),
            _ => None,
        };
        assert_eq!(over(place::CallReturn::Returned), Some(None));
        assert_eq!(over(place::CallReturn::FellBehind), Some(Some(End::FellBehind)));
        assert_eq!(over(place::CallReturn::Stopped(Some(End::Closed(None)))), Some(Some(End::Closed(None))));
        let gone = terms::Ended { reason: None, refusal: None, ..Default::default() };
        assert!(matches!(
            place::CallReturn::Disconnected(gone).told(),
            place::Message::Session(place::SessionMessage::Disconnected(_))
        ));
        assert!(matches!(
            place::CallReturn::Faulted.told(),
            place::Message::Session(place::SessionMessage::CallFaulted)
        ));
    }
}
