//! The account's own status at a server: what the account chose, whether its Do not disturb holds, and what its bar
//! says.

use super::*;
use crate::status::StatusWord;
use proto::status::Chosen;

impl Place {
    /// The account's own status choice in force at `now` (seconds since the epoch), as the server last told it; none
    /// where the account is not signed in.
    pub fn chosen(&self, now: i64) -> Option<Chosen> {
        self.signed_in().then(|| self.chosen.at(now))
    }

    /// The account's Do not disturb holds here at `now`: no popup and no sound.
    pub(crate) fn quiet(&self, now: i64) -> bool {
        self.chosen(now).is_some_and(|chosen| chosen.quiet(now))
    }

    /// What the account bar says under the account's name at `now`, with its dot: its status where it is signed in
    /// (Online before the wall clock is known), else why it is not.
    pub fn account_line(&self, now: Option<i64>) -> StatusWord {
        match self.panel() {
            Panel::Channels
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Call
            | Panel::Settings
            | Panel::Account
            | Panel::Lost => now
                .and_then(|now| self.chosen(now))
                .map_or(StatusWord::Online, |chosen| StatusWord::chosen(chosen.choice)),
            Panel::Connecting | Panel::NotAnswering { .. } | Panel::SignedOut | Panel::Deleted | Panel::NewPassword => {
                StatusWord::NotSignedIn
            }
            Panel::KeyChanged => StatusWord::NotConnected,
        }
    }

    /// A session's greeting: what the account chose is told again as it signs in.
    pub(super) fn greeted(&mut self) {
        self.chosen = Chosen::ONLINE;
    }

    /// The server told the account's own choice.
    pub(super) fn status_chosen(&mut self, chosen: Chosen) {
        self.chosen = chosen;
    }
}
