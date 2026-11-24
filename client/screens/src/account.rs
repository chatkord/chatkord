//! The person's own account page: the username, which cannot change; the name others see, which is optional and saved
//! on Enter or when the page is left; the colour others see them in, saved when its picker closes; and the password,
//! changed in a dialog that asks the one in force first. Sign out and leave are the window's own confirmations.

use crate::Asked;
use crate::chosen::{self, LengthNote, MatchNote};
use crate::colour::{self, Picker};
use crate::name_field::{FieldNote, NameField};
use crate::picture_row::{self, PictureRow};
use crate::tell::FieldSaid;
use proto::chatkord_v1::ErrorCode;
use proto::files::FileId;
use proto::rules::Colour;
use terms::{ErrorKind, LinkError, Secret};
use words::{Phrase, Sentence};

/// One of the three fields of a password change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The password in force.
    Current,
    /// The new password.
    New,
    /// The new password typed again.
    Again,
}

impl Field {
    const fn at(self) -> usize {
        match self {
            Field::Current => 0,
            Field::New => 1,
            Field::Again => 2,
        }
    }
}

/// Why a change was refused, said under the field it is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The current password does not match; it is cleared.
    Wrong,
    /// The server refused: too many tries too soon.
    TooManyTries,
    /// The server will not take the new password although the form did.
    NotAccepted,
    /// The server could not be reached, or did not answer.
    NoAnswer,
}

impl Problem {
    /// The field the refusal is about, which says it under its name; none for one about the server.
    pub fn field(&self) -> Option<Field> {
        match self {
            Problem::Wrong | Problem::TooManyTries => Some(Field::Current),
            Problem::NotAccepted | Problem::NoAnswer => None,
        }
    }

    /// The refusal as a line: `server` is the server's name, for the one that names it.
    pub fn said(&self, server: &str) -> FieldSaid {
        FieldSaid::refusal(match self {
            Problem::Wrong => Sentence::Phrase(Phrase::PasswordDoesntMatch),
            Problem::TooManyTries => Sentence::Phrase(Phrase::TooManyTries),
            Problem::NotAccepted => Sentence::Phrase(Phrase::PasswordNotAccepted),
            Problem::NoAnswer => Sentence::NoAnswer(server.to_owned()),
        })
    }
}

/// The line under the new password when it is the one in force.
pub fn same_as_now() -> FieldSaid {
    FieldSaid::refusal(Sentence::Phrase(Phrase::SameAsNow))
}

/// The form: the password in force and the new one, typed twice.
#[derive(Debug)]
pub struct ChangingPassword {
    current: Secret,
    new: Secret,
    again: Secret,
    revealed: [bool; 3],
    asked: Asked<Problem>,
}

impl Default for ChangingPassword {
    fn default() -> Self {
        let empty = || Secret::new(String::new());
        ChangingPassword { current: empty(), new: empty(), again: empty(), revealed: [false; 3], asked: Asked::Idle }
    }
}

impl ChangingPassword {
    /// What is typed in `field`; empty until something is.
    pub fn field(&self, field: Field) -> &Secret {
        match field {
            Field::Current => &self.current,
            Field::New => &self.new,
            Field::Again => &self.again,
        }
    }

    /// Whether `field` shows what is typed instead of dots.
    pub fn revealed(&self, field: Field) -> bool {
        self.revealed[field.at()]
    }

    /// Why the last change was refused; none while asking, and once the current password is typed again.
    pub fn problem(&self) -> Option<Problem> {
        self.asked.refused()
    }

    /// The server is being asked.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// The note under the new password; none while it is empty.
    pub fn length_note(&self) -> Option<LengthNote> {
        chosen::length_note(&self.new)
    }

    /// The note under the new password typed again; none while that field is empty.
    pub fn match_note(&self) -> Option<MatchNote> {
        chosen::match_note(&self.new, &self.again)
    }

    /// The new password is the one in force: a change that changes nothing is not asked.
    pub fn same_as_now(&self) -> bool {
        !self.new.is_empty() && self.new == self.current
    }

    /// Nothing is typed in any field, so closing the dialog loses nothing.
    pub fn is_untouched(&self) -> bool {
        self.current.is_empty() && self.new.is_empty() && self.again.is_empty()
    }

    /// Whether Save does something now.
    pub fn ready(&self) -> bool {
        !self.asking() && !self.current.is_empty() && !self.same_as_now() && chosen::ready(&self.new, &self.again)
    }

    fn typed(&mut self, field: Field, typed: Secret) {
        if self.asking() {
            return;
        }
        match field {
            Field::Current => self.current = typed,
            Field::New => self.new = typed,
            Field::Again => self.again = typed,
        }
        if field == Field::Current {
            self.asked.forget();
        }
    }

    /// The server is asked with what was typed, unless it cannot be now.
    fn ask(&mut self) -> Option<(Secret, Secret)> {
        if !self.ready() {
            return None;
        }
        self.asked.ask();
        Some((self.current.clone(), self.new.clone()))
    }

    /// The server's answer; whether the password was saved. A wrong current password is cleared, and the new ones stay.
    fn answered(&mut self, answer: &Result<(), LinkError>) -> bool {
        let Err(refused) = answer else {
            self.asked.answered(None);
            return true;
        };
        let problem = match refused.kind() {
            ErrorKind::Refused(ErrorCode::Unauthenticated) => Problem::Wrong,
            ErrorKind::Refused(ErrorCode::RateLimited) => Problem::TooManyTries,
            ErrorKind::Refused(ErrorCode::InvalidRequest) => Problem::NotAccepted,
            ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached => Problem::NoAnswer,
        };
        self.asked.answered(Some(problem));
        if problem == Problem::Wrong {
            self.current = Secret::new(String::new());
        }
        false
    }
}

/// What a person does with the password dialog.
#[derive(Clone, Debug)]
pub enum Message {
    /// "Change password": the dialog opens.
    Change,
    /// Cancel on the dialog, its ✕ or Escape.
    Cancel,
    /// A press outside the dialog: it closes only while nothing is typed or asked.
    Outside,
    /// A field's whole text after a key in it.
    Typed(Field, Secret),
    /// The eye beside a field: its text shown, or hidden again.
    Reveal(Field),
    /// Save password, or Enter.
    Save,
}

/// The account page, open.
#[derive(Debug)]
pub struct Account {
    /// The name others see as the server last said it; empty while it is the username.
    name: NameField,
    /// The colour others see, at a server that keeps one (`profile-colour`).
    colour: Option<Picker>,
    /// The row of the picture others see.
    picture: PictureRow,
    /// The password dialog, while it is open.
    password: Option<ChangingPassword>,
}

impl Account {
    /// Opened on the name others see now (`shown`) for the account called `username`, and its colour's picker where
    /// the server keeps one.
    pub(crate) fn new(username: &str, shown: Option<&str>, colour: Option<Picker>) -> Account {
        Account {
            name: NameField::new(chosen_name(username, shown)),
            colour,
            picture: PictureRow::profile(None),
            password: None,
        }
    }

    /// Opened before the server has said who this is: no name, no colour.
    pub(crate) fn unknown() -> Account {
        Account {
            name: NameField::new(String::new()),
            colour: None,
            picture: PictureRow::profile(None),
            password: None,
        }
    }

    /// The same page with its picture row holding `held`.
    pub(crate) fn holding(mut self, held: Option<FileId>) -> Account {
        self.picture = PictureRow::profile(held);
        self
    }

    /// The row of the picture others see.
    pub fn picture(&self) -> &PictureRow {
        &self.picture
    }

    /// Something done in the picture's row: what it asks of the host, for this account's own picture.
    pub(crate) fn update_picture(&mut self, message: picture_row::Message) -> Option<picture_row::Ask> {
        self.picture.update(message)
    }

    /// The server said the picture changed, here or on another device: a row that is not busy follows it.
    pub(crate) fn pictured(&mut self, picture: Option<FileId>) {
        self.picture.follow(picture);
    }

    /// The server refused to set what the row held: the row says so and shows `server`'s again.
    pub(crate) fn picture_not_set(&mut self, server: Option<FileId>, failed: LinkError) {
        self.picture.not_set(server, failed);
    }

    /// The password dialog, while it is open.
    pub fn password(&self) -> Option<&ChangingPassword> {
        self.password.as_ref()
    }

    /// The colour's picker; none at a server that keeps no colour.
    pub fn colour(&self) -> Option<&Picker> {
        self.colour.as_ref()
    }

    /// The colour to save, when the picker keeps a change: none for no colour picked.
    pub(crate) fn update_colour(&mut self, message: colour::Message) -> Option<Option<Colour>> {
        self.colour.as_mut().and_then(|picker| picker.update(message))
    }

    /// What leaving the page saves of the colour: what the open picker holds.
    pub(crate) fn left_colour(&mut self) -> Option<Option<Colour>> {
        self.colour.as_mut().filter(|picker| picker.is_open()).and_then(|picker| picker.update(colour::Message::Done))
    }

    /// The server said the colour changed, here or on another device: a closed picker follows it.
    pub(crate) fn recoloured(&mut self, colour: Option<Colour>) {
        if let Some(picker) = self.colour.as_mut() {
            picker.follow(colour);
        }
    }

    /// What the name field holds, untrimmed; empty while it is the username.
    pub fn typed(&self) -> &str {
        self.name.typed()
    }

    /// What the name field says now: a check of what is typed first, then the server's last answer.
    pub fn note(&self) -> FieldNote {
        self.name.note()
    }

    /// What the line under the name field says, if anything: that the field is optional is said beside its name, and
    /// a name nobody is refused for want of a word has no refusal of its own, so one the server would not take says it
    /// did not answer.
    pub fn said(&self, server: &str) -> Option<FieldSaid> {
        match self.note() {
            FieldNote::Unchanged => None,
            FieldNote::NotAllowed => Some(FieldSaid::refusal(Sentence::NoAnswer(server.to_owned()))),
            note => note.said(server),
        }
    }

    /// The name to save when Enter has one ready: none to go back to the username.
    pub(crate) fn update_name(&mut self, message: crate::name_field::Message) -> Option<Option<String>> {
        self.name.update(message)
    }

    /// The name to save, when one is ready and nothing is asked yet: also what leaving the page sends.
    pub(crate) fn ask_name(&mut self) -> Option<Option<String>> {
        self.name.ask()
    }

    /// The server's answer to saving the name.
    pub(crate) fn name_answered(&mut self, answer: &Result<(), LinkError>) {
        self.name.answered(answer);
    }

    /// The server's answer to saving the colour: a refusal puts the picker back on the colour it had.
    pub(crate) fn colour_answered(&mut self, answer: &Result<(), LinkError>) {
        if let Some(picker) = self.colour.as_mut() {
            picker.answered(answer);
        }
    }

    /// The server said the name others see changed, here or on another device: a field nobody is changing follows it.
    pub(crate) fn renamed(&mut self, username: &str, shown: &str) {
        self.name.follow(chosen_name(username, Some(shown)));
    }

    /// The password to change and the new one, when the dialog asks the server now.
    pub(crate) fn update(&mut self, message: Message) -> Option<(Secret, Secret)> {
        match (message, &mut self.password) {
            (Message::Change, None) => self.password = Some(ChangingPassword::default()),
            (Message::Cancel, Some(form)) if !form.asking() => self.password = None,
            (Message::Outside, Some(form)) if form.is_untouched() && !form.asking() => self.password = None,
            (Message::Typed(field, typed), Some(form)) => form.typed(field, typed),
            (Message::Reveal(field), Some(form)) => form.revealed[field.at()] = !form.revealed[field.at()],
            (Message::Save, Some(form)) => return form.ask(),
            (
                Message::Change
                | Message::Cancel
                | Message::Outside
                | Message::Typed(..)
                | Message::Reveal(_)
                | Message::Save,
                _,
            ) => {}
        }
        None
    }

    /// The server's answer to the change: the dialog closes when it was saved. Whether it was.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> bool {
        let Some(form) = self.password.as_mut() else { return false };
        let saved = form.answered(answer);
        if saved {
            self.password = None;
        }
        saved
    }
}

/// The name a person chose: what the server shows unless that is the username.
fn chosen_name(username: &str, shown: Option<&str>) -> String {
    shown.filter(|shown| *shown != username).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{ADA, Clock, account_name, ada_in_call, id, in_call, member, refused, tell, two_signed_in};
    use crate::harness::{sends, wire};
    use crate::name_field::Message as Name;
    use crate::place::{self, Panel, PlaceId};
    use crate::settings::Page;
    use crate::window::{Asking, Dialog, DialogMessage, Pick, Window};
    use crate::{Ask, WindowMessage};
    use proto::chatkord_v1::{ErrorCode, StandingChanged};
    use proto::rules::{Grant, MAX_NAME_CHARS};
    use std::time::Instant;
    use terms::{LinkError, Secret};

    const NOW: &str = "evening games with friends";
    const NEW: &str = "another correct phrase entirely";

    fn secret(text: &str) -> Secret {
        Secret::new(text.to_string())
    }

    fn typed(account: &mut Account, current: &str, new: &str, again: &str) {
        account.update(Message::Change);
        account.update(Message::Typed(Field::Current, secret(current)));
        account.update(Message::Typed(Field::New, secret(new)));
        account.update(Message::Typed(Field::Again, secret(again)));
    }

    #[test]
    fn save_waits_for_the_current_password_a_long_new_one_and_the_same_again() {
        let mut account = Account::unknown();
        assert!(account.password().is_none());
        account.update(Message::Change);
        assert!(!account.password().unwrap().ready());
        account.update(Message::Typed(Field::Current, secret(NOW)));
        account.update(Message::Typed(Field::New, secret("short")));
        account.update(Message::Typed(Field::Again, secret("short")));
        assert!(!account.password().unwrap().ready(), "too short");
        account.update(Message::Typed(Field::New, secret(NEW)));
        assert!(!account.password().unwrap().ready(), "the second differs");
        account.update(Message::Typed(Field::Again, secret(NEW)));
        assert!(account.password().unwrap().ready());
        account.update(Message::Typed(Field::Current, secret("")));
        assert!(!account.password().unwrap().ready(), "the password in force is part of the ask");
    }

    #[test]
    fn a_new_password_that_is_the_one_in_force_is_never_asked() {
        let mut account = Account::unknown();
        typed(&mut account, NEW, NEW, NEW);
        let form = account.password().unwrap();
        assert!(form.same_as_now() && !form.ready());
        assert!(account.update(Message::Save).is_none(), "a change that changes nothing makes no ask");
    }

    #[test]
    fn save_asks_once_and_a_form_being_asked_changes_nothing() {
        let mut account = Account::unknown();
        typed(&mut account, NOW, NEW, NEW);
        let (current, new) = account.update(Message::Save).expect("asked");
        assert_eq!((current.expose(), new.expose()), (NOW, NEW));
        assert!(account.update(Message::Save).is_none(), "asked once");
        account.update(Message::Typed(Field::New, secret("changed under it")));
        account.update(Message::Cancel);
        let form = account.password().expect("not thrown away while asked");
        assert_eq!(form.field(Field::New).expose(), NEW);
        assert!(form.asking());
    }

    #[test]
    fn a_wrong_current_password_is_cleared_and_the_new_ones_stay() {
        let mut account = Account::unknown();
        typed(&mut account, "not it", NEW, NEW);
        account.update(Message::Save);
        assert!(!account.answered(&Err(refused(ErrorCode::Unauthenticated))));
        let form = account.password().unwrap();
        assert_eq!(form.problem(), Some(Problem::Wrong));
        assert!(form.field(Field::Current).is_empty());
        assert_eq!((form.field(Field::New).expose(), form.field(Field::Again).expose()), (NEW, NEW));
        account.update(Message::Typed(Field::Current, secret(NOW)));
        assert_eq!(account.password().unwrap().problem(), None, "typing again takes the note away");
    }

    #[test]
    fn each_refusal_is_said_under_the_form() {
        for (answer, said) in [
            (refused(ErrorCode::RateLimited), Problem::TooManyTries),
            (refused(ErrorCode::InvalidRequest), Problem::NotAccepted),
            (LinkError::Timeout, Problem::NoAnswer),
            (LinkError::Connect, Problem::NoAnswer),
        ] {
            let mut account = Account::unknown();
            typed(&mut account, NOW, NEW, NEW);
            account.update(Message::Save);
            assert!(!account.answered(&Err(answer)));
            let form = account.password().unwrap();
            assert_eq!(form.problem(), Some(said));
            assert_eq!(form.field(Field::Current).expose(), NOW, "only a wrong password is cleared");
            assert!(form.ready(), "and the form can be sent again");
        }
    }

    #[test]
    fn a_saved_password_closes_the_form() {
        let mut account = Account::unknown();
        typed(&mut account, NOW, NEW, NEW);
        account.update(Message::Save);
        assert!(account.answered(&Ok(())));
        assert!(account.password().is_none());
        assert!(!account.answered(&Ok(())), "a late answer finds no form");
    }

    #[test]
    fn each_field_is_shown_or_hidden_on_its_own() {
        let mut account = Account::unknown();
        account.update(Message::Change);
        account.update(Message::Reveal(Field::New));
        let form = account.password().unwrap();
        assert!(form.revealed(Field::New) && !form.revealed(Field::Current) && !form.revealed(Field::Again));
        account.update(Message::Reveal(Field::New));
        assert!(!account.password().unwrap().revealed(Field::New));
    }

    /// The window's first server, signed in, with its account page open.
    fn on_the_account_page(clock: &Clock) -> (Window, PlaceId) {
        let mut window = two_signed_in(clock);
        let owls_id = id(&window, 0);
        tell(&mut window, clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        (window, owls_id)
    }

    fn account(window: &mut Window, clock: &Clock, id: PlaceId, message: crate::account::Message) -> Vec<Ask> {
        tell(window, clock, id, place::Message::Page(place::PageMessage::Account(message)))
    }

    #[test]
    fn the_account_page_and_the_servers_settings_replace_each_other_and_a_call_goes_on_under_both() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        assert_eq!(window.open().unwrap().panel(), Panel::Call);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.open().unwrap().panel(), Panel::Account);
        assert!(window.open().unwrap().call().is_some(), "the call is not left by looking at settings");
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::CloseAccount));
        assert_eq!(window.open().unwrap().panel(), Panel::Call);
        let owner = StandingChanged {
            place: proto::rules::Place::OWNER.wire(),
            grants: Grant::ALL.map(|word| word.word().to_string()).into(),
        };
        tell(&mut window, &clock, owls_id, place::Message::standing(&owner));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Settings(Page::Invites)));
        assert_eq!(window.open().unwrap().panel(), Panel::Settings, "the server's pages open over it");
        assert!(window.open().unwrap().account().is_none());
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.open().unwrap().panel(), Panel::Account);
        assert!(window.open().unwrap().settings().is_none());
    }

    #[test]
    fn a_password_is_changed_with_the_one_in_force_and_said_in_a_toast() {
        use crate::account::{Field, Message as Change};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_the_account_page(&clock);
        let typed = |field, text: &str| Change::Typed(field, Secret::new(text.into()));
        account(&mut window, &clock, owls_id, Change::Change);
        account(&mut window, &clock, owls_id, typed(Field::Current, "evening games with friends"));
        account(&mut window, &clock, owls_id, typed(Field::New, "another correct phrase entirely"));
        assert!(account(&mut window, &clock, owls_id, Change::Save).is_empty(), "not typed twice yet: no ask");
        account(&mut window, &clock, owls_id, typed(Field::Again, "another correct phrase entirely"));
        let todo = account(&mut window, &clock, owls_id, Change::Save);
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)]
            if *at == owls_id
                && sends(
                    ask,
                    wire::ChangePassword {
                        password: "another correct phrase entirely".into(),
                        current: "evening games with friends".into()
                    }
                )),
            "{todo:?}"
        );
        let wrong = LinkError::Refused(ErrorCode::Unauthenticated);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::PasswordChanged(Err(wrong))));
        assert!(
            window.toast().map(|toast| toast.said().in_english()).as_deref().is_none(),
            "a refusal is said under the form"
        );
        assert!(window.open().unwrap().account().unwrap().password().is_some());
        account(&mut window, &clock, owls_id, typed(Field::Current, "evening games with friends"));
        account(&mut window, &clock, owls_id, Change::Save);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::PasswordChanged(Ok(()))));
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("Password changed. Your other devices are signed out.")
        );
        let page = window.open().unwrap().account().unwrap();
        assert!(page.password().is_none(), "the form closes, the page stays");
    }

    #[test]
    fn a_press_outside_the_password_dialog_closes_it_only_while_nothing_is_typed() {
        let mut account = Account::unknown();
        account.update(Message::Change);
        account.update(Message::Outside);
        assert!(account.password().is_none(), "nothing typed: closed");
        account.update(Message::Change);
        account.update(Message::Typed(Field::New, secret("a")));
        account.update(Message::Outside);
        assert!(account.password().is_some(), "something typed: kept");
        account.update(Message::Cancel);
        assert!(account.password().is_none(), "Cancel always closes");
    }

    #[test]
    fn the_accounts_picture_row_is_the_accounts_own_and_follows_the_server_while_idle() {
        use crate::picture_row::{Ask as Row, Message as Pick, Picked};
        const OWL: FileId = FileId::from_bytes([7; FileId::LEN]);
        let mut page = Account::new(ADA, None, None).holding(None);
        assert_eq!(page.picture().kind(), proto::files::FileKind::ProfilePicture);
        assert_eq!(page.update_picture(Pick::Choose), Some(Row::Pick));
        assert_eq!(page.update_picture(Pick::Picked(Picked::File(vec![1]))), Some(Row::Upload(vec![1])));
        assert_eq!(page.update_picture(Pick::Uploaded(Ok(OWL))), Some(Row::Hold(Some(OWL))));
        page.picture_not_set(None, LinkError::Timeout);
        assert_eq!(page.picture().held(), None, "a refused picture shows the server's again");
        page.pictured(Some(OWL));
        assert_eq!(page.picture().held(), Some(OWL), "another device's change");
    }

    #[test]
    fn a_name_is_typed_saved_and_said_saved() {
        let mut page = Account::new(ADA, Some(ADA), None);
        assert_eq!((page.typed(), page.note()), ("", FieldNote::Unchanged), "the username is no chosen name");
        page.update_name(Name::Typed("Ada Lovelace ".into()));
        assert_eq!(page.note(), FieldNote::Unsaved);
        assert_eq!(page.update_name(Name::Save), Some(Some("Ada Lovelace".into())), "sent trimmed");
        assert_eq!(page.note(), FieldNote::Saving);
        assert_eq!(page.update_name(Name::Save), None, "asked once");
        page.name_answered(&Ok(()));
        assert_eq!(page.note(), FieldNote::Saved);
        assert_eq!(page.ask_name(), None, "nothing left to save");
    }

    #[test]
    fn an_emptied_name_goes_back_to_the_username() {
        let mut page = Account::new(ADA, Some("Countess"), None);
        assert_eq!(page.typed(), "Countess");
        page.update_name(Name::Typed("  ".into()));
        assert_eq!(page.update_name(Name::Save), Some(None));
    }

    #[test]
    fn a_name_out_of_bounds_is_said_and_not_sent() {
        let mut page = Account::new(ADA, None, None);
        page.update_name(Name::Typed("n".repeat(MAX_NAME_CHARS + 1)));
        assert_eq!((page.note(), page.update_name(Name::Save)), (FieldNote::TooLong, None));
        page.update_name(Name::Typed("a\u{202E}b".into()));
        assert_eq!((page.note(), page.update_name(Name::Save)), (FieldNote::Characters, None));
    }

    #[test]
    fn a_refused_name_is_said_until_it_is_typed_again() {
        let mut page = Account::new(ADA, None, None);
        page.update_name(Name::Typed("Ada L".into()));
        page.update_name(Name::Save);
        page.name_answered(&Err(refused(ErrorCode::InvalidRequest)));
        assert_eq!(page.note(), FieldNote::NotAccepted);
        page.update_name(Name::Typed("Ada Lo".into()));
        assert_eq!(page.note(), FieldNote::Unsaved);
        page.update_name(Name::Save);
        page.name_answered(&Err(refused(ErrorCode::RateLimited)));
        assert_eq!(page.note(), FieldNote::TooManyTries);
    }

    #[test]
    fn a_rename_from_elsewhere_moves_an_untouched_field_and_spares_a_typed_one() {
        let mut page = Account::new(ADA, None, None);
        page.renamed(ADA, "Countess");
        assert_eq!(page.typed(), "Countess");
        page.update_name(Name::Typed("Ada L".into()));
        page.renamed(ADA, "Enchantress");
        assert_eq!((page.typed(), page.note()), ("Ada L", FieldNote::Unsaved));
    }

    #[test]
    fn a_colour_saves_when_its_picker_closes_or_the_page_is_left_and_follows_elsewhere() {
        let blue = Colour::parse("#3D7FD6").expect("blue");
        let swatch = Colour::parse("#8A44A8").expect("a swatch");
        let mut page = Account::new(ADA, None, Some(Picker::new(None, swatch)));
        assert_eq!(page.colour().map(Picker::picked), Some(swatch), "one who picked none starts on their swatch");
        page.update_colour(colour::Message::Toggle);
        page.update_colour(colour::Message::Typed("#3D7FD6".into()));
        assert_eq!(page.update_colour(colour::Message::Done), Some(Some(blue)));
        page.update_colour(colour::Message::Toggle);
        page.update_colour(colour::Message::Reset);
        assert_eq!(page.left_colour(), Some(None), "leaving keeps the swatch again as none picked");
        page.recoloured(Some(blue));
        assert_eq!(page.colour().map(Picker::picked), Some(blue), "a closed picker follows another device");
        assert_eq!(Account::new(ADA, None, None).left_colour(), None, "a server with no colour asks none");
    }

    #[test]
    fn the_name_others_see_is_saved_on_enter_and_shown_once_the_server_says_so() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = ada_in_call(&clock);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        assert_eq!(window.open().unwrap().panel(), Panel::Account);
        account_name(&mut window, &clock, owls_id, Name::Typed("Countess".into()));
        let todo = account_name(&mut window, &clock, owls_id, Name::Save);
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)]
                if *at == owls_id && sends(ask, wire::SetDisplayName { display_name: Some("Countess".into()) })),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::NameSet(Ok(()))));
        assert_eq!(window.open().unwrap().account().unwrap().note(), FieldNote::Saved);
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the page says it");
        assert_eq!(window.open().unwrap().shown(), Some(ADA), "shown once the server tells everyone");
        let renamed =
            place::Message::Server(place::ServerMessage::Renamed { user_id: member("ada"), name: "Countess".into() });
        tell(&mut window, &clock, owls_id, renamed);
        let place = window.open().unwrap();
        assert_eq!(place.shown(), Some("Countess"));
        assert_eq!(place.call().unwrap().tiles()[0].name, "Countess", "the call's own tile follows");
    }

    #[test]
    fn a_colour_picked_on_the_account_page_is_saved_and_shown_once_the_server_says_so() {
        use crate::colour::Message as Pick;
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = ada_in_call(&clock);
        let swatch = design::PersonColour::Swatch(crate::people::swatch_of(&member("ada")));
        assert_eq!(window.open().unwrap().own_colour(), Some(swatch), "none picked is the id's swatch");
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::ProfileColour(Pick::Toggle)));
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Page(place::PageMessage::ProfileColour(Pick::Typed("#3D7FD6".into()))),
        );
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::ProfileColour(Pick::Done)));
        let blue = proto::rules::Colour::parse("#3D7FD6").ok();
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)] if *at == owls_id && sends(ask, wire::SetProfileColour { colour: blue.map(|blue| blue.to_string()) })),
            "{todo:?}"
        );
        assert_eq!(window.open().unwrap().own_colour(), Some(swatch), "shown once the server tells everyone");
        let told = place::Message::Server(place::ServerMessage::Recoloured { user_id: member("ada"), colour: blue });
        tell(&mut window, &clock, owls_id, told);
        let place = window.open().unwrap();
        let picked = design::PersonColour::Picked(design::Rgb::hex(0x3D7FD6));
        assert_eq!(place.own_colour(), Some(picked));
        assert_eq!(place.call().unwrap().tiles()[0].colour, picked, "the call's own tile follows");
        let grace = place::Message::Server(place::ServerMessage::Recoloured { user_id: member("grace"), colour: blue });
        tell(&mut window, &clock, owls_id, grace);
        assert_eq!(window.open().unwrap().own_colour(), Some(picked), "another's colour is not this account's");
    }

    #[test]
    fn a_name_typed_and_not_saved_is_saved_when_the_page_is_left() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = ada_in_call(&clock);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        account_name(&mut window, &clock, owls_id, Name::Typed("Countess".into()));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::CloseAccount));
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if crate::harness::is(ask, crate::harness::Op::SetDisplayName)),
            "{todo:?}"
        );
        assert_ne!(window.open().unwrap().panel(), Panel::Account);
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::OpenAccount));
        assert!(
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::CloseAccount)).is_empty(),
            "nothing typed"
        );
    }

    #[test]
    fn sign_out_and_leave_on_the_account_page_are_the_menus_confirmations() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_the_account_page(&clock);
        window.update(WindowMessage::Pick(owls_id, Pick::SignOut), clock.at());
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::SignOut)));
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        window.update(WindowMessage::Pick(owls_id, Pick::Leave), clock.at());
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::Leave(_))));
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        window.update(WindowMessage::Pick(owls_id, Pick::SignOut), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert_eq!(window.open().unwrap().panel(), Panel::SignedOut, "the page goes with the session");
        assert!(window.open().unwrap().account().is_none());
    }
}
