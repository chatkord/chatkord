//! A status as the window says it: the word under a name, the dot beside a picture, the account's own status menu in
//! its popover, and how long a status chosen there lasts.

use proto::status::{Choice, Chosen, Shown};
use words::{Ends, Phrase, Sentence};

/// The hour of tomorrow's morning "Until tomorrow" lasts to, where the person is.
pub const TOMORROW_AT: i8 = 8;

/// A status said in words, with the dot beside the picture it goes with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusWord {
    /// Signed in and about.
    Online,
    /// Away, by choice or idle everywhere.
    Away,
    /// Not to be disturbed.
    DoNotDisturb,
    /// Signed in on no device, as others are shown it.
    Offline,
    /// The account's own choice to be shown offline, said only to the account itself.
    Invisible,
    /// The account bar's while the server's key changed: this device is not connected there.
    NotConnected,
    /// The account bar's while the account is not signed in here.
    NotSignedIn,
}

/// The dot beside a picture that says a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusDot {
    /// Online.
    Online,
    /// Away.
    Away,
    /// Do not disturb.
    DoNotDisturb,
    /// Offline, where a popover still marks it.
    Offline,
    /// A hollow ring: the account's own invisible choice.
    Hollow,
}

impl StatusWord {
    /// How every other member is shown `shown`.
    pub const fn of(shown: Shown) -> StatusWord {
        match shown {
            Shown::Offline => StatusWord::Offline,
            Shown::Online => StatusWord::Online,
            Shown::Away => StatusWord::Away,
            Shown::DoNotDisturb => StatusWord::DoNotDisturb,
        }
    }

    /// The account's own `choice`, as its bar and its status menu say it.
    pub const fn chosen(choice: Choice) -> StatusWord {
        match choice {
            Choice::Online => StatusWord::Online,
            Choice::Away => StatusWord::Away,
            Choice::DoNotDisturb => StatusWord::DoNotDisturb,
            Choice::Invisible => StatusWord::Invisible,
        }
    }

    /// The word.
    pub const fn phrase(self) -> Phrase {
        match self {
            StatusWord::Online => Phrase::PresenceOnline,
            StatusWord::Away => Phrase::StatusAway,
            StatusWord::DoNotDisturb => Phrase::DoNotDisturb,
            StatusWord::Offline => Phrase::PresenceOffline,
            StatusWord::Invisible => Phrase::StatusInvisible,
            StatusWord::NotConnected => Phrase::NotConnected,
            StatusWord::NotSignedIn => Phrase::NotSignedIn,
        }
    }

    /// Its dot; none where the account is not in.
    pub const fn dot(self) -> Option<StatusDot> {
        match self {
            StatusWord::Online => Some(StatusDot::Online),
            StatusWord::Away => Some(StatusDot::Away),
            StatusWord::DoNotDisturb => Some(StatusDot::DoNotDisturb),
            StatusWord::Offline => Some(StatusDot::Offline),
            StatusWord::Invisible => Some(StatusDot::Hollow),
            StatusWord::NotConnected | StatusWord::NotSignedIn => None,
        }
    }
}

/// How long a status other than Online lasts, as the status menu offers it under the one being picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusFor {
    /// An hour from now.
    OneHour,
    /// Eight hours from now.
    EightHours,
    /// Until tomorrow at [`TOMORROW_AT`] where the person is.
    Tomorrow,
    /// Until it is changed.
    UntilOff,
}

impl StatusFor {
    /// Every duration, in the order the menu lists them.
    pub const ALL: [StatusFor; 4] =
        [StatusFor::OneHour, StatusFor::EightHours, StatusFor::Tomorrow, StatusFor::UntilOff];

    /// Its label.
    pub const fn phrase(self) -> Phrase {
        match self {
            StatusFor::OneHour => Phrase::StatusForHour,
            StatusFor::EightHours => Phrase::StatusForEightHours,
            StatusFor::Tomorrow => Phrase::StatusUntilTomorrow,
            StatusFor::UntilOff => Phrase::StatusUntilOff,
        }
    }

    /// When a status picked at `now` (seconds since the epoch) for this long ends; `tomorrow` is the moment of
    /// tomorrow's morning where the person is, which only their calendar knows: none where it cannot be found, when
    /// that pick lasts a day.
    pub fn until(self, now: i64, tomorrow: Option<i64>) -> Option<i64> {
        const HOUR: i64 = 60 * 60;
        match self {
            StatusFor::OneHour => Some(now.saturating_add(HOUR)),
            StatusFor::EightHours => Some(now.saturating_add(8 * HOUR)),
            StatusFor::Tomorrow => Some(tomorrow.unwrap_or_else(|| now.saturating_add(24 * HOUR))),
            StatusFor::UntilOff => None,
        }
    }
}

/// The choices the status menu offers, in its order.
const CHOICES: [Choice; 4] = [Choice::Online, Choice::Away, Choice::DoNotDisturb, Choice::Invisible];

/// One row of the account's status menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusRow {
    /// What it picks.
    pub choice: Choice,
    /// Its word and dot.
    pub word: StatusWord,
    /// It is the account's choice now.
    pub chosen: bool,
    /// Under the chosen row while its choice has an end: when it ends.
    pub until: Option<Sentence>,
    /// Its durations are open under it.
    pub picking: bool,
}

/// The status menu of the account's own popover, opened from the account bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusMenu {
    /// Online, Away, Do not disturb, Invisible.
    pub rows: Vec<StatusRow>,
    /// The durations offered under the row being picked, none while no row is: [`StatusFor::ALL`].
    pub durations: Vec<StatusFor>,
}

impl StatusMenu {
    /// The menu for an account whose choice in force is `chosen`, its row `picking` open, its end said by `ends`.
    pub(crate) fn of(chosen: Chosen, picking: Option<Choice>, ends: Option<Ends>) -> StatusMenu {
        let rows = CHOICES
            .into_iter()
            .map(|choice| {
                let is_chosen = chosen.choice == choice;
                StatusRow {
                    choice,
                    word: StatusWord::chosen(choice),
                    chosen: is_chosen,
                    until: ends.filter(|_| is_chosen).map(Sentence::StatusUntil),
                    picking: picking == Some(choice),
                }
            })
            .collect();
        let durations = if picking.is_some() { StatusFor::ALL.to_vec() } else { Vec::new() };
        StatusMenu { rows, durations }
    }
}

/// What is done in the account's status menu, and the pointer to it on Settings › Notifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMessage {
    /// A row pressed: Online is asked of the server at once; another opens its durations, or closes them while open.
    Pick(Choice),
    /// A duration pressed under the row being picked: that status is asked of the server, ending then.
    For(StatusFor),
    /// Settings › Notifications' "Choose a status": settings close and the account's popover opens on the open server.
    Choose,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_763_035_200;

    #[test]
    fn each_duration_ends_when_it_says() {
        assert_eq!(StatusFor::OneHour.until(NOW, None), Some(NOW + 3_600));
        assert_eq!(StatusFor::EightHours.until(NOW, None), Some(NOW + 8 * 3_600));
        assert_eq!(StatusFor::Tomorrow.until(NOW, Some(NOW + 5)), Some(NOW + 5), "the calendar's morning");
        assert_eq!(StatusFor::Tomorrow.until(NOW, None), Some(NOW + 86_400), "else a day");
        assert_eq!(StatusFor::UntilOff.until(NOW, None), None);
        for duration in StatusFor::ALL {
            let until = duration.until(NOW, chat::tomorrow_at(NOW, &chat::TimeZone::UTC, TOMORROW_AT));
            assert!(
                until.is_none_or(|until| until > NOW && until <= NOW + proto::rules::MAX_STATUS_FOR),
                "{duration:?}: a server takes it"
            );
        }
    }

    #[test]
    fn every_status_has_its_word_and_only_the_signed_in_have_a_dot() {
        for word in [StatusWord::Online, StatusWord::Away, StatusWord::DoNotDisturb, StatusWord::Offline] {
            assert!(word.dot().is_some(), "{word:?}");
        }
        assert_eq!(StatusWord::Invisible.dot(), Some(StatusDot::Hollow));
        assert_eq!((StatusWord::NotConnected.dot(), StatusWord::NotSignedIn.dot()), (None, None));
        assert_eq!(StatusWord::chosen(Choice::Invisible).phrase().in_english(), "Invisible");
        assert_eq!(StatusWord::of(Shown::Offline).phrase().in_english(), "Offline", "others see an invisible one so");
    }
}
