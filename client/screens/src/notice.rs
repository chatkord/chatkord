//! What a page of settings says to a person about itself, held by the page: a sentence of a kind about one subject,
//! drawn as a callout. The page shows them most severe first, at most [`MOST`]; a subject said again replaces what was
//! said of it, and a notice is withdrawn or put away by its subject, never by its words. What happens to a person in a
//! call is a toast, a state line or a mark on a control (`tell`), never a notice.

use crate::tell::Callout;
use design::Status;
use words::Sentence;

/// The most notices an area shows at once.
pub const MOST: usize = 3;

/// What a notice is about: an area holds one notice per subject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Subject {
    /// The microphone: it would not start or open again, or it moved to another device.
    Microphone,
    /// The speaker: it would not play, or it moved to another device.
    Speaker,
    /// The sound devices as a whole: none can be looked at.
    Devices,
    /// This account's mute or deafen, as the server was told it.
    VoiceState,
    /// The share's sound.
    ShareSound,
    /// The desktop's screen picker.
    Picker,
    /// A change to a member's roles.
    Roles,
    /// A change to who sees a channel or writes in it.
    Channels,
    /// The test tone of the voice settings page.
    Tone,
}

/// A sentence of a kind, about the page that holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// What it is about: one notice per subject in an area.
    pub subject: Subject,
    /// How severe it is, which orders and colours it.
    pub kind: Status,
    /// What it says, put in the person's words when the page is drawn.
    pub said: Sentence,
    /// An event, which a person may put away, or a state, which goes when its owner says it is over.
    pub lifetime: Lifetime,
}

/// How long a notice lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    /// Said once, and put away by a press.
    Event,
    /// Stays while the area it is about is in it.
    State,
}

impl Notice {
    /// Whether a person may put it away.
    pub fn dismissable(&self) -> bool {
        self.lifetime == Lifetime::Event
    }

    /// An event: said once, and put away by a press.
    pub fn new(subject: Subject, kind: Status, said: Sentence) -> Notice {
        Notice { subject, kind, said, lifetime: Lifetime::Event }
    }

    /// A state: it stays while the page it is about is in it.
    pub fn state(subject: Subject, kind: Status, said: Sentence) -> Notice {
        Notice { subject, kind, said, lifetime: Lifetime::State }
    }

    /// What the page draws: a callout of its kind, with a ✕ while a person may put it away.
    pub fn callout(&self) -> Callout {
        let callout = Callout::new(self.said.clone()).of(self.kind);
        if self.dismissable() { callout.dismissable() } else { callout }
    }
}

const fn severity(kind: Status) -> u8 {
    match kind {
        Status::Danger => 3,
        Status::Warning => 2,
        Status::Info => 1,
        Status::Success => 0,
    }
}

/// `notice` held: it replaces what was said of its subject and moves to the end.
pub(crate) fn raise(held: &mut Vec<Notice>, notice: Notice) {
    withdraw(held, notice.subject);
    held.push(notice);
}

/// What was said of `subject` put away, when it no longer holds.
pub(crate) fn withdraw(held: &mut Vec<Notice>, subject: Subject) {
    held.retain(|one| one.subject != subject);
}

/// What an area shows: the most severe first, the older first among equals, and no more than [`MOST`].
pub(crate) fn shown(held: &[Notice]) -> Vec<Notice> {
    let mut shown = held.to_vec();
    shown.sort_by_key(|one| std::cmp::Reverse(severity(one.kind)));
    shown.truncate(MOST);
    shown
}

/// What Escape puts away among what is shown: the first a person may put away.
pub fn escapes(shown: &[Notice]) -> Option<Subject> {
    shown.iter().find(|one| one.dismissable()).map(|one| one.subject)
}

/// A person put away what was said of `subject`, if it may be put away.
pub(crate) fn put_away(held: &mut Vec<Notice>, subject: Subject) {
    held.retain(|one| one.subject != subject || !one.dismissable());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(subject: Subject, kind: Status, said: &str) -> Notice {
        Notice::new(subject, kind, Sentence::Said(said.into()))
    }

    fn order(held: &[Notice]) -> Vec<String> {
        shown(held).iter().map(|one| one.said.in_english()).collect()
    }

    #[test]
    fn the_most_severe_is_shown_first_and_the_older_first_among_equals() {
        let mut held = Vec::new();
        for notice in [
            said(Subject::Microphone, Status::Info, "a"),
            said(Subject::Tone, Status::Danger, "b"),
            said(Subject::Speaker, Status::Info, "c"),
            said(Subject::Picker, Status::Warning, "d"),
        ] {
            raise(&mut held, notice);
        }
        assert_eq!(order(&held), ["b", "d", "a"], "three at most, danger, warning, then the older info");
    }

    #[test]
    fn a_subject_said_again_is_shown_once_in_its_newest_words_behind_its_equals() {
        let mut held = Vec::new();
        raise(&mut held, said(Subject::Microphone, Status::Warning, "x"));
        raise(&mut held, said(Subject::Speaker, Status::Warning, "y"));
        raise(&mut held, said(Subject::Microphone, Status::Warning, "z"));
        assert_eq!(order(&held), ["y", "z"]);
    }

    #[test]
    fn putting_away_names_the_subject_so_a_notice_raised_meanwhile_stays() {
        let mut held = vec![said(Subject::Microphone, Status::Warning, "shown")];
        // Drawn with "shown" first; a danger arrives before the press lands and is sorted ahead of it.
        raise(&mut held, said(Subject::Tone, Status::Danger, "new"));
        put_away(&mut held, Subject::Microphone);
        assert_eq!(order(&held), ["new"], "the press put away what it was drawn on");
        put_away(&mut held, Subject::Picker);
        assert_eq!(held.len(), 1, "nothing is said of that subject");
    }

    #[test]
    fn a_state_is_not_put_away_by_a_press_but_withdrawn_by_its_owner() {
        let mut held = vec![Notice::state(Subject::Devices, Status::Danger, Sentence::Said("held".into()))];
        put_away(&mut held, Subject::Devices);
        assert_eq!(order(&held), ["held"]);
        withdraw(&mut held, Subject::Devices);
        assert!(held.is_empty());
    }

    #[test]
    fn escape_passes_over_a_state_to_the_first_that_may_be_put_away() {
        let held = [
            Notice::state(Subject::Devices, Status::Danger, Sentence::Said("held".into())),
            said(Subject::Microphone, Status::Warning, "a"),
        ];
        assert_eq!(escapes(&held), Some(Subject::Microphone));
        assert_eq!(escapes(&held[..1]), None, "a state alone is not put away by Escape");
    }

    #[test]
    fn a_fourth_notice_is_held_and_shown_once_one_goes() {
        let mut held = Vec::new();
        for (subject, text) in
            [(Subject::Microphone, "a"), (Subject::Speaker, "b"), (Subject::Picker, "c"), (Subject::Tone, "d")]
        {
            raise(&mut held, said(subject, Status::Warning, text));
        }
        assert_eq!(shown(&held).len(), MOST);
        put_away(&mut held, Subject::Microphone);
        assert_eq!(order(&held), ["b", "c", "d"]);
    }
}
