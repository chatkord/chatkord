//! What a chosen password is held to, said as the two notes under its fields: the door's "Set a new password" and the
//! account's change of password read the same rule.

use crate::tell::FieldSaid;
use proto::rules::{MIN_CHOSEN_CHARS, is_common};
use terms::Secret;
use words::{Phrase, Sentence};

/// What the note under a new password says, once something is typed: its length, then whether it is a common one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthNote {
    /// Too short: this many more characters would make it long enough.
    ToGo(usize),
    /// At least the shortest a chosen password may be.
    LongEnough,
    /// Long enough, but one of the common passwords a guesser tries first, which the server refuses.
    Common,
}

/// What the second password's note says, once something is typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchNote {
    /// It is the same as the first.
    Matches,
    /// It is not the same as the first.
    Differs,
}

impl LengthNote {
    /// The line under the password: help, or a refusal of a common one.
    pub fn said(self) -> FieldSaid {
        match self {
            LengthNote::ToGo(left) => FieldSaid::help(Sentence::ToGo(left)),
            LengthNote::LongEnough => FieldSaid::help(Sentence::Phrase(Phrase::LongEnough)),
            LengthNote::Common => FieldSaid::refusal(Sentence::Phrase(Phrase::TooCommon)),
        }
    }
}

impl MatchNote {
    /// The line under the repeated password.
    pub fn said(self) -> FieldSaid {
        match self {
            MatchNote::Matches => FieldSaid::help(Sentence::Phrase(Phrase::Matches)),
            MatchNote::Differs => FieldSaid::refusal(Sentence::Phrase(Phrase::Differs)),
        }
    }
}

pub(crate) fn length_note(password: &Secret) -> Option<LengthNote> {
    match password.expose().chars().count() {
        0 => None,
        n if n >= MIN_CHOSEN_CHARS && is_common(password.expose()) => Some(LengthNote::Common),
        n if n >= MIN_CHOSEN_CHARS => Some(LengthNote::LongEnough),
        n => Some(LengthNote::ToGo(MIN_CHOSEN_CHARS - n)),
    }
}

pub(crate) fn match_note(password: &Secret, again: &Secret) -> Option<MatchNote> {
    match (again.is_empty(), again == password) {
        (true, _) => None,
        (false, true) => Some(MatchNote::Matches),
        (false, false) => Some(MatchNote::Differs),
    }
}

/// Long enough, and typed twice the same.
pub(crate) fn ready(password: &Secret, again: &Secret) -> bool {
    length_note(password) == Some(LengthNote::LongEnough) && match_note(password, again) == Some(MatchNote::Matches)
}
