//! An ask proved by the current password (leaving a server, handing it on), and why the server refused an ask about an
//! account: one reading of its answer for every dialog that asks one.

use crate::Asked;
use crate::place::ops;
use crate::window::{Asking, Confirming, DialogMessage, Next};
use proto::chatkord_v1::ErrorCode;
use terms::{ErrorKind, LinkError, Secret};

/// Why an ask was refused, said in the dialog that asked it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// This account may not do that to them (any more).
    NotAllowed,
    /// The current password typed is not this account's.
    WrongPassword,
    /// The server refused: too many tries too soon.
    TooManyTries,
    /// The server could not be reached, or refused for a reason the dialog has no words for.
    NoAnswer,
}

/// What the server's refusal means: none when the account asked about is no longer there, else why.
pub(crate) fn refusal(refused: &LinkError) -> Option<Refused> {
    match refused.kind() {
        ErrorKind::Refused(ErrorCode::NotFound) => None,
        ErrorKind::Refused(ErrorCode::NotAuthorized) => Some(Refused::NotAllowed),
        ErrorKind::Refused(ErrorCode::Unauthenticated) => Some(Refused::WrongPassword),
        ErrorKind::Refused(ErrorCode::RateLimited) => Some(Refused::TooManyTries),
        ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached => Some(Refused::NoAnswer),
    }
}

/// The current password, typed in a dialog.
#[derive(Debug)]
pub struct Proof {
    pub(crate) password: Secret,
    pub(crate) revealed: bool,
    asked: Asked<Refused>,
}

impl Default for Proof {
    fn default() -> Self {
        Proof { password: Secret::new(String::new()), revealed: false, asked: Asked::Idle }
    }
}

impl Proof {
    /// What is typed in the password field; emptied after a wrong one.
    pub fn password(&self) -> &Secret {
        &self.password
    }

    /// Whether the field shows the password as typed rather than masked.
    pub fn revealed(&self) -> bool {
        self.revealed
    }

    /// Why the server refused the last ask; none before an ask, while one is asked and once something is typed.
    pub fn refused(&self) -> Option<Refused> {
        self.asked.refused()
    }

    /// The server is being asked.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Whether the dialog's main button does something now.
    pub fn ready(&self) -> bool {
        !self.password.is_empty() && !self.asking()
    }

    /// Something typed or asked, which a press outside the dialog must not throw away.
    pub(crate) fn kept(&self) -> bool {
        !self.password.is_empty() || self.asking()
    }

    pub(crate) fn typed(&mut self, password: Secret) {
        if !self.asking() {
            self.password = password;
            self.asked.forget();
        }
    }

    /// The server is asked with the password typed, unless it cannot be now.
    pub(crate) fn ask(&mut self) -> Option<Secret> {
        if !self.ready() {
            return None;
        }
        self.asked.ask();
        Some(self.password.clone())
    }

    /// The server refused: a wrong password is cleared.
    pub(crate) fn answered(&mut self, refused: Refused) {
        if refused == Refused::WrongPassword {
            self.password = Secret::new(String::new());
        }
        self.asked.answered(Some(refused));
    }
}

impl Proof {
    /// The password typed or shown; on Confirm, the password to send, once one may be sent.
    pub(crate) fn heard(&mut self, said: DialogMessage) -> Option<Secret> {
        match said {
            DialogMessage::Password(typed) => self.typed(typed),
            DialogMessage::Reveal => self.revealed = !self.revealed,
            DialogMessage::Confirm => return self.ask(),
            DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::Named(_)
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => {}
        }
        None
    }

    /// The server refused to let the account leave: an ownership this window has not heard of asks for a new owner
    /// first, and anything else is said in the dialog.
    pub(crate) fn leave_refused(&mut self, refused: &LinkError) -> Next {
        match refusal(refused) {
            Some(Refused::NotAllowed) => return Next::Becomes(Asking::OwnerLeaves),
            Some(refused) => self.answered(refused),
            None => self.answered(Refused::NoAnswer),
        }
        Next::Stays
    }
}

/// Leaving a server: the proof alone, sent on Confirm.
impl Confirming for Proof {
    fn holds_outside(&self) -> bool {
        self.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        self.heard(said).map_or(Next::Stays, |password| Next::Asks(ops::leave_server(&password)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, id, tell, two_signed_in};
    use crate::harness::{sends, wire};
    use crate::window::{Asking, Dialog, DialogMessage, Pick, Window};
    use crate::{Ask, WindowMessage, place};
    use proto::chatkord_v1::ErrorCode;
    use proto::rules::Grants;
    use std::time::Instant;
    use terms::{LinkError, Secret};

    #[test]
    fn an_account_gone_is_no_refusal_and_each_other_answer_says_why() {
        let refused = |code| LinkError::Refused(code);
        assert_eq!(refusal(&refused(ErrorCode::NotFound)), None);
        assert_eq!(refusal(&refused(ErrorCode::NotAuthorized)), Some(Refused::NotAllowed));
        assert_eq!(refusal(&refused(ErrorCode::Unauthenticated)), Some(Refused::WrongPassword));
        assert_eq!(refusal(&refused(ErrorCode::RateLimited)), Some(Refused::TooManyTries));
        assert_eq!(refusal(&LinkError::Timeout), Some(Refused::NoAnswer));
    }

    #[test]
    fn a_proof_is_asked_once_and_a_wrong_password_is_cleared() {
        let mut proof = Proof::default();
        assert!(proof.ask().is_none(), "nothing typed");
        proof.typed(Secret::new("evening games".into()));
        assert_eq!(proof.ask().map(|said| said.expose().to_string()).as_deref(), Some("evening games"));
        assert!(proof.ask().is_none(), "asked once");
        proof.typed(Secret::new("changed".into()));
        assert_eq!(proof.password().expose(), "evening games", "nothing changes while asked");
        proof.answered(Refused::WrongPassword);
        assert!(proof.password().is_empty() && !proof.asking());
        assert_eq!(proof.refused(), Some(Refused::WrongPassword));
    }

    #[test]
    fn leaving_asks_the_password_and_a_wrong_one_keeps_the_server() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        window.update(WindowMessage::Pick(owls_id, Pick::Leave), clock.at());
        let leaving = |window: &Window| match window.dialog().map(Dialog::asking) {
            Some(Asking::Leave(leaving)) => {
                (leaving.password().expose().to_string(), leaving.refused(), leaving.ready())
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(leaving(&window), (String::new(), None, false), "Leave waits for the password");
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty());
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("wrong one".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_some(), "a press outside keeps a typed password");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)]
                if *at == owls_id && sends(ask, wire::LeaveServer { current: "wrong one".into(), delete_messages: false })),
            "{todo:?}"
        );
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "asked once");
        let wrong = LinkError::Refused(ErrorCode::Unauthenticated);
        assert!(
            tell(
                &mut window,
                &clock,
                owls_id,
                place::Message::Session(place::SessionMessage::LeaveAnswered(Err(wrong)))
            )
            .is_empty()
        );
        assert_eq!(leaving(&window), (String::new(), Some(Refused::WrongPassword), false), "cleared, said");
        assert_eq!(window.places().len(), 2);
        let busy = LinkError::Refused(ErrorCode::RateLimited);
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("again".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::LeaveAnswered(Err(busy))));
        assert_eq!(leaving(&window).1, Some(Refused::TooManyTries));
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        assert!(window.dialog().is_none());
    }

    #[test]
    fn a_server_left_is_gone_and_the_owner_is_told_to_hand_it_on_first() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        window.update(WindowMessage::Pick(owls_id, Pick::Leave), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("evening games".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        // The server knew of an ownership the window had not heard of: the dialog for an owner of a server instead.
        let owner = LinkError::Refused(ErrorCode::NotAuthorized);
        tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::LeaveAnswered(Err(owner))));
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::OwnerLeaves)));
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "only Cancel");
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::OWNER,
                grants: Grants::default(),
            }),
        );
        window.update(WindowMessage::Pick(books_id, Pick::Leave), clock.at());
        assert!(
            matches!(window.dialog().map(Dialog::asking), Some(Asking::OwnerLeaves)),
            "an owner of a server is not asked for a password"
        );
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        window.update(WindowMessage::Pick(owls_id, Pick::Leave), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("evening games".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::LeaveAnswered(Ok(()))));
        assert!(
            matches!(&todo[0], Ask::At(at, place::Ask::EndAccount { sign_out: false, .. }) if *at == owls_id),
            "the server ended the sessions itself: {todo:?}"
        );
        assert!(window.dialog().is_none());
        assert_eq!(window.places().len(), 1);
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the rail shows it gone");
    }
}
