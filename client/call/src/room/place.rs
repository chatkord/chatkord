//! This account's place in the call: what the person wants, what the server last agreed to, an administrator's hold
//! beside them, and the state of the one ask that carries a change. The hold and the person's own mute are two facts:
//! the hold silences the microphone and never writes the own mute, which the person may change while held and finds
//! as they left it once the hold lifts. The ask itself is a future the call's driver holds.

use choices::State;

/// Whether an administrator muted this account for everyone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hold {
    Free,
    Held,
}

/// What the person wants of the place, what the server holds of it, and the state asked of the server and not yet
/// answered.
#[derive(Clone, Debug)]
pub(super) struct Place {
    want: State,
    sent: State,
    asking: Option<State>,
    hold: Hold,
}

impl Place {
    /// A place as the join left it: the server already holds `start` as the person's own, silenced by `hold`.
    pub(super) fn new(start: State, hold: Hold) -> Self {
        Self { want: start, sent: start, asking: None, hold }
    }

    /// The person's own mute, held or not: a hold silences the microphone apart from it.
    pub(super) fn mute(&mut self, muted: bool) {
        self.want.mic_muted = muted;
    }

    pub(super) fn deafen(&mut self, deafened: bool) {
        self.want.deafened = deafened;
    }

    /// The microphone would not open again: the place stays muted, as the server will be told.
    pub(super) fn mic_refused(&mut self) {
        self.want.mic_muted = true;
    }

    /// The hold began or ended. While it holds the microphone is off whatever is in flight; lifted, the person's own
    /// mute is as they set it.
    pub(super) fn hold(&mut self, hold: Hold) {
        self.hold = hold;
    }

    /// What the person wants now, their own mute and deafen, which this device carries out at once and the window
    /// shows as theirs.
    pub(super) fn want(&self) -> State {
        self.want
    }

    /// Whether nothing may be sent from the microphone now: the person's own mute or deafen, or the hold.
    pub(super) fn silenced(&self) -> bool {
        self.want.mic_muted || self.want.deafened || self.hold == Hold::Held
    }

    /// The state to ask the server for what the person wants, unless an ask is in flight or the server holds it
    /// already.
    pub(super) fn ask(&mut self) -> Option<State> {
        if self.asking.is_some() || self.want == self.sent {
            return None;
        }
        self.asking = Some(self.want);
        Some(self.want)
    }

    /// The server agreed to what was asked: it is what the room is told this account's own state is.
    pub(super) fn agreed(&mut self) -> State {
        if let Some(asked) = self.asking.take() {
            self.sent = asked;
        }
        self.sent
    }

    /// The server refused what was asked: the press is undone, unless the person pressed again since.
    pub(super) fn refused(&mut self) {
        if let Some(asked) = self.asking.take()
            && self.want == asked
        {
            self.want = self.sent;
        }
    }

    /// Whether the microphone may be sent: the server has the place heard, and the person still wants it so.
    pub(super) fn may_speak(&self) -> bool {
        let heard = |state: State| !state.mic_muted && !state.deafened;
        self.hold == Hold::Free && heard(self.sent) && heard(self.want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEARD: State = State { mic_muted: false, deafened: false };
    const MUTED: State = State { mic_muted: true, deafened: false };

    fn asked(place: &mut Place) -> Option<State> {
        place.ask()
    }

    #[test]
    fn a_hold_during_an_unmute_in_flight_keeps_the_microphone_off_and_the_unmute_the_persons() {
        let mut place = Place::new(MUTED, Hold::Free);
        place.mute(false);
        assert_eq!(asked(&mut place), Some(HEARD));
        place.hold(Hold::Held);
        assert!(place.silenced(), "off at once, before the server answers");
        assert_eq!(place.agreed(), HEARD, "the person's own state is theirs; the hold is told apart");
        assert_eq!(asked(&mut place), None, "nothing more to ask: the hold is the server's own");
        assert!(!place.may_speak());
        place.hold(Hold::Free);
        assert!(place.may_speak() && !place.silenced(), "lifted, the own unmute stands");
    }

    #[test]
    fn a_set_that_finishes_behind_what_the_person_wants_now_is_sent_again() {
        let mut place = Place::new(HEARD, Hold::Free);
        place.mute(true);
        assert_eq!(asked(&mut place), Some(MUTED));
        place.deafen(true);
        assert_eq!(asked(&mut place), None, "one set at a time");
        assert_eq!(place.agreed(), MUTED);
        assert_eq!(asked(&mut place), Some(State { mic_muted: true, deafened: true }));
    }

    #[test]
    fn a_refused_set_puts_the_place_back_and_a_new_press_asks_again() {
        let mut place = Place::new(MUTED, Hold::Free);
        place.mute(false);
        asked(&mut place);
        place.refused();
        assert_eq!(place.want(), MUTED, "the press is undone where it shows and on the device");
        assert_eq!(asked(&mut place), None, "nothing is asked of what the server holds");
        place.mute(false);
        assert_eq!(asked(&mut place), Some(HEARD), "pressed again, asked again");
    }

    #[test]
    fn a_press_during_an_ask_survives_its_refusal() {
        let mut place = Place::new(HEARD, Hold::Free);
        place.mute(true);
        asked(&mut place);
        place.deafen(true);
        place.refused();
        let pressed = State { mic_muted: true, deafened: true };
        assert_eq!(place.want(), pressed, "the newer press stands");
        assert_eq!(asked(&mut place), Some(pressed));
    }

    #[test]
    fn nothing_is_asked_of_what_the_server_already_holds() {
        let mut place = Place::new(MUTED, Hold::Free);
        place.mute(false);
        place.mute(true);
        assert_eq!(asked(&mut place), None);
    }

    #[test]
    fn the_own_mute_changes_while_held_is_asked_and_the_lift_leaves_it_as_set() {
        let mut place = Place::new(HEARD, Hold::Held);
        assert!(place.silenced(), "a place that joins held is silenced");
        assert_eq!(place.want(), HEARD, "the hold never writes the own mute");
        place.mute(true);
        assert_eq!(asked(&mut place), Some(MUTED), "the own mute is asked while held");
        place.agreed();
        place.mute(false);
        assert_eq!(asked(&mut place), Some(HEARD), "and its undo");
        place.agreed();
        assert!(place.silenced() && !place.may_speak(), "heard by no one while held");
        place.hold(Hold::Free);
        assert!(!place.silenced() && place.may_speak(), "a lift with the own mute off leaves the person heard");
        place.mute(true);
        place.hold(Hold::Held);
        place.hold(Hold::Free);
        assert!(place.silenced(), "with it on, they stay muted by their own");
        assert_eq!(place.want(), MUTED);
    }

    #[test]
    fn the_microphone_may_be_sent_only_once_the_server_agreed() {
        let mut place = Place::new(MUTED, Hold::Free);
        place.mute(false);
        asked(&mut place);
        assert!(!place.may_speak(), "asked, not agreed");
        place.agreed();
        assert!(place.may_speak());
        place.deafen(true);
        assert!(!place.may_speak(), "a deafen pressed since keeps it closed");
    }
}
