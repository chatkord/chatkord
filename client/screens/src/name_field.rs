//! A name others read that a page saves on Enter or when the page is left: the name others see on the account page, a server's
//! name in Overview, a role's in Roles. It is shaped as `proto::rules` says such a name is before anything is sent; an
//! empty field is "none" (the username, or the address) where a name may be none, and never sent where one may not.

use crate::tell::FieldSaid;
use proto::chatkord_v1::ErrorCode;
use proto::rules::{MAX_NAME_CHARS, TextRefused, text_shape};
use terms::{ErrorKind, LinkError};
use words::{Phrase, Sentence};

/// What the field says beside its label; each page says it in its own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldNote {
    /// Nothing typed differs from what is saved.
    Unchanged,
    /// A change Enter would save.
    Unsaved,
    /// Sent to the server, which has not answered yet.
    Saving,
    /// The server took the last change, and nothing has been typed since.
    Saved,
    /// Longer than a name others read may be.
    TooLong,
    /// A control or text-direction character.
    Characters,
    /// The server would not take the name although the field did.
    NotAccepted,
    /// The account may no longer change it.
    NotAllowed,
    /// The server refused: too many changes too soon.
    TooManyTries,
    /// The server could not be reached, or refused for a reason the field has no words for.
    NoAnswer,
}

impl FieldNote {
    /// The line under the field it is the note of, `server` being the server's name for a refusal that names it; none
    /// while the field holds what is saved.
    pub fn said(self, server: &str) -> Option<FieldSaid> {
        let refusal = |phrase| Some(FieldSaid::refusal(Sentence::Phrase(phrase)));
        let help = |phrase| Some(FieldSaid::help(Sentence::Phrase(phrase)));
        match self {
            FieldNote::Unchanged => None,
            FieldNote::Unsaved => help(Phrase::PressEnterToSave),
            FieldNote::Saving => help(Phrase::Saving),
            FieldNote::Saved => help(Phrase::Saved),
            FieldNote::TooLong => Some(FieldSaid::refusal(Sentence::TextTooLong)),
            FieldNote::Characters => refusal(Phrase::PlainTextOnly),
            FieldNote::NotAccepted => refusal(Phrase::NameNotAccepted),
            FieldNote::NotAllowed => refusal(Phrase::NameNotAllowed),
            FieldNote::TooManyTries => refusal(Phrase::TooManyTries),
            FieldNote::NoAnswer => Some(FieldSaid::refusal(Sentence::NoAnswer(server.to_owned()))),
        }
    }
}

/// What a person does in the field.
#[derive(Clone, Debug)]
pub enum Message {
    /// The field's whole text after an edit; ignored while a save is asked.
    Typed(String),
    /// Enter in the field.
    Save,
}

/// Where the field's last save stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Save {
    /// Nothing asked since the field was last typed in.
    Idle,
    /// Asked; no answer yet.
    Asking,
    /// The server took it, and nothing has been typed since.
    Saved,
    /// The server would not take it, for the reason the note says.
    Refused(FieldNote),
}

/// What an emptied field means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Emptied {
    /// None: the username, or the address.
    Unnamed,
    /// Nothing: a role has a name always, so an empty field is never sent.
    Unsent,
}

/// The field, over the name as the server last said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameField {
    /// Empty for none.
    saved: String,
    typed: String,
    save: Save,
    emptied: Emptied,
}

impl NameField {
    pub(crate) fn new(saved: String) -> NameField {
        NameField { typed: saved.clone(), saved, save: Save::Idle, emptied: Emptied::Unnamed }
    }

    /// A field over a name that is never none: emptied, it says a change is to be saved and sends nothing.
    pub(crate) fn required(saved: String) -> NameField {
        NameField { emptied: Emptied::Unsent, ..NameField::new(saved) }
    }

    /// What the field holds, untrimmed.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// What the field says now: a check of what is typed first, then the server's last answer.
    pub fn note(&self) -> FieldNote {
        match (self.changed(), self.save) {
            (Some(Err(note)), _) => note,
            (Some(Ok(_)), Save::Asking) => FieldNote::Saving,
            (Some(Ok(_)), Save::Refused(note)) => note,
            (Some(Ok(_)), Save::Idle | Save::Saved) => FieldNote::Unsaved,
            (None, Save::Saved) => FieldNote::Saved,
            (None, Save::Idle | Save::Asking | Save::Refused(_)) => FieldNote::Unchanged,
        }
    }

    /// What typed would save, when it differs from the saved name: none for an emptied field, or why it may not be.
    fn changed(&self) -> Option<Result<Option<&str>, FieldNote>> {
        let typed = self.typed.trim();
        if typed == self.saved {
            return None;
        }
        Some(match text_shape(typed, MAX_NAME_CHARS) {
            Ok(name) => Ok(Some(name)),
            Err(TextRefused::Empty) => match self.emptied {
                Emptied::Unnamed => Ok(None),
                Emptied::Unsent => Err(FieldNote::Unsaved),
            },
            Err(TextRefused::TooLong) => Err(FieldNote::TooLong),
            Err(TextRefused::ControlCharacter) => Err(FieldNote::Characters),
        })
    }

    pub(crate) fn update(&mut self, message: Message) -> Option<Option<String>> {
        match message {
            Message::Typed(typed) if self.save != Save::Asking => {
                self.typed = typed;
                self.save = Save::Idle;
                None
            }
            Message::Typed(_) => None,
            Message::Save => self.ask(),
        }
    }

    /// The name to save, when one is ready and nothing is asked yet: also what leaving the page sends.
    pub(crate) fn ask(&mut self) -> Option<Option<String>> {
        if self.save == Save::Asking {
            return None;
        }
        let name = self.changed()?.ok()?.map(str::to_string);
        self.save = Save::Asking;
        Some(name)
    }

    /// The server's answer to a save.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) {
        self.save = match answer.as_ref().map_err(LinkError::kind) {
            Ok(()) => {
                self.saved = self.typed.trim().to_string();
                Save::Saved
            }
            Err(ErrorKind::Refused(ErrorCode::InvalidRequest)) => Save::Refused(FieldNote::NotAccepted),
            Err(ErrorKind::Refused(ErrorCode::NotAuthorized)) => Save::Refused(FieldNote::NotAllowed),
            Err(ErrorKind::Refused(ErrorCode::RateLimited)) => Save::Refused(FieldNote::TooManyTries),
            Err(ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached) => {
                Save::Refused(FieldNote::NoAnswer)
            }
        };
    }

    /// The server said the name changed, here or on another device: a field nobody is changing follows it.
    pub(crate) fn follow(&mut self, saved: String) {
        if self.changed().is_none() && self.save != Save::Asking {
            self.typed = saved.clone();
        }
        self.saved = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(code: ErrorCode) -> Result<(), LinkError> {
        Err(LinkError::Refused(code))
    }

    #[test]
    fn a_name_is_typed_saved_and_said_saved_and_an_emptied_field_is_none() {
        let mut field = NameField::new("Night Owls".into());
        assert_eq!(field.note(), FieldNote::Unchanged);
        field.update(Message::Typed("Owls ".into()));
        assert_eq!(field.update(Message::Save), Some(Some("Owls".into())), "sent trimmed");
        assert_eq!((field.note(), field.update(Message::Save)), (FieldNote::Saving, None), "asked once");
        field.answered(&Ok(()));
        assert_eq!(field.note(), FieldNote::Saved);
        field.update(Message::Typed(" ".into()));
        assert_eq!(field.update(Message::Save), Some(None));
    }

    #[test]
    fn a_name_that_is_never_none_is_not_sent_emptied() {
        let mut field = NameField::required("Moderators".into());
        field.update(Message::Typed("  ".into()));
        assert_eq!((field.note(), field.update(Message::Save)), (FieldNote::Unsaved, None));
        field.update(Message::Typed("Mods".into()));
        assert_eq!(field.update(Message::Save), Some(Some("Mods".into())));
    }

    #[test]
    fn an_account_that_may_no_longer_change_it_is_told_so() {
        let mut field = NameField::new(String::new());
        field.update(Message::Typed("Server S".into()));
        field.ask();
        field.answered(&refused(ErrorCode::NotAuthorized));
        assert_eq!(field.note(), FieldNote::NotAllowed);
    }
}
