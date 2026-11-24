//! What the window tells a person, in the five shapes the canvas gives it and no other: a toast for an event, a state
//! line for what holds, a mark on the control that failed, a callout for a note a page keeps, and the sentence under
//! a field. Each is a value of the sentence and its kind; a front end draws it and decides nothing.

use crate::place::{Panel, Place, PlaceId, Roll, WayBack};
use design::Status;
use std::time::{Duration, Instant};
use words::{Phrase, Sentence};

/// What a toast, or a callout, offers to do about what it says: at most one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start sharing the screen again: the desktop's picker opens.
    ShareAgain,
    /// Turn one's camera on again, after it stopped by itself.
    TurnOnCamera,
    /// Take the place in a call that ended back, as its card offers.
    Again(WayBack),
    /// Join the call a dropped connection left, named by its channel.
    Rejoin(String),
    /// Invite people, on a server this account has just made.
    InvitePeople,
    /// Open the server this window lists already, named as it shows.
    OpenServer(String),
}

impl Action {
    /// The words of the pill that offers it.
    pub fn said(&self) -> Sentence {
        match self {
            Action::ShareAgain => Sentence::Phrase(Phrase::ShareAgain),
            Action::TurnOnCamera => Sentence::Phrase(Phrase::TurnOnCamera),
            Action::Again(way) => Sentence::Phrase(way.phrase()),
            Action::Rejoin(channel) => Sentence::Rejoin(channel.clone()),
            Action::InvitePeople => Sentence::Phrase(Phrase::InvitePeople),
            Action::OpenServer(server) => Sentence::OpenServer(server.clone()),
        }
    }
}

/// What is to be told as an event, before it has a time: a sentence of a kind, with at most one action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Telling {
    /// What kind of news it is: a failure is danger, good news success, plain news info.
    pub kind: Status,
    /// What it says.
    pub said: Sentence,
    /// What it offers to do about it.
    pub action: Option<Action>,
}

impl Telling {
    /// Plain news: no dot, no action.
    pub fn plain(said: Sentence) -> Telling {
        Telling { kind: Status::Info, said, action: None }
    }

    /// News of a kind.
    pub fn of(kind: Status, said: Sentence) -> Telling {
        Telling { kind, said, action: None }
    }

    /// The same, offering `action`.
    pub fn offering(mut self, action: Action) -> Telling {
        self.action = Some(action);
        self
    }

    /// Whether it carries a status dot: plain news has none.
    pub fn dotted(&self) -> bool {
        self.kind != Status::Info
    }

    /// How long it stays: a plain one briefly, one with a dot or an action long enough to read and press.
    pub fn lasts(&self) -> Duration {
        if self.dotted() || self.action.is_some() { design::TOAST_NOTED_FOR } else { design::TOAST_PLAIN_FOR }
    }
}

/// A short word over the window that goes by itself, one at a time: a newer one replaces it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// What it says, of what kind, offering what.
    pub telling: Telling,
    /// The server it came from, which its action is about.
    pub place: PlaceId,
    until: Instant,
    held: bool,
}

impl Toast {
    pub(crate) fn new(place: PlaceId, telling: Telling, now: Instant) -> Toast {
        let until = now + telling.lasts();
        Toast { telling, place, until, held: false }
    }

    /// What it says.
    pub fn said(&self) -> &Sentence {
        &self.telling.said
    }

    /// What kind of news it is.
    pub fn kind(&self) -> Status {
        self.telling.kind
    }

    /// What it offers to do.
    pub fn action(&self) -> Option<&Action> {
        self.telling.action.as_ref()
    }

    /// Whether it has a ✕: whenever it has a dot or an action.
    pub fn closable(&self) -> bool {
        self.telling.dotted() || self.telling.action.is_some()
    }

    /// Whether the pointer is on it, which keeps it.
    pub fn held(&self) -> bool {
        self.held
    }

    /// Whether it is still to be shown at `now`: while held, or before its time.
    pub(crate) fn shows(&self, now: Instant) -> bool {
        self.held || self.until > now
    }

    /// The time it goes, once it is not held.
    pub(crate) fn until(&self) -> Instant {
        self.until
    }

    /// The pointer came onto it or left it: leaving starts its time again.
    pub(crate) fn hold(&mut self, held: bool, now: Instant) {
        if self.held && !held {
            self.until = now + self.telling.lasts();
        }
        self.held = held;
    }
}

/// What holds, said on one line at the foot of the area it is about, and gone when it is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLine {
    /// How severe it is, which colours its dot.
    pub kind: Status,
    /// What it says.
    pub said: Sentence,
}

impl StateLine {
    /// What holds of `place` now: the connection lost while the channels stay, an administrator's mute on the call and
    /// what the account's own microphone is once it lifts.
    pub fn of(place: &Place) -> Option<StateLine> {
        match place.panel() {
            Panel::Lost => {
                Some(StateLine { kind: Status::Warning, said: Sentence::Lost(place.server().shown().to_string()) })
            }
            Panel::Call => place.call().filter(|call| call.live() && call.held()).map(|call| {
                let after = if call.state().mic_muted { words::Text::HeldOwnMuted } else { words::Text::HeldOwnOn };
                StateLine { kind: Status::Danger, said: Sentence::Text(after) }
            }),
            Panel::Connecting
            | Panel::NotAnswering { .. }
            | Panel::SignedOut
            | Panel::Deleted
            | Panel::KeyChanged
            | Panel::NewPassword
            | Panel::Channels
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Settings
            | Panel::Account => None,
        }
    }
}

/// A device of the call that can fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// This account's microphone.
    Microphone,
    /// This account's camera.
    Camera,
    /// This account's screen share.
    Share,
}

/// What failed of the call's devices, each with its sentence, until it is used again or works again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    microphone: Option<Sentence>,
    camera: Option<Sentence>,
    share: Option<Sentence>,
}

impl Faults {
    /// What failed of `device`, which is the tooltip of its control; none while it works.
    pub fn of(&self, device: Device) -> Option<&Sentence> {
        match device {
            Device::Microphone => self.microphone.as_ref(),
            Device::Camera => self.camera.as_ref(),
            Device::Share => self.share.as_ref(),
        }
    }

    fn slot(&mut self, device: Device) -> &mut Option<Sentence> {
        match device {
            Device::Microphone => &mut self.microphone,
            Device::Camera => &mut self.camera,
            Device::Share => &mut self.share,
        }
    }

    /// `device` failed, for the reason `said`.
    pub(crate) fn set(&mut self, device: Device, said: Sentence) {
        *self.slot(device) = Some(said);
    }

    /// `device` was pressed or works again.
    pub(crate) fn clear(&mut self, device: Device) {
        *self.slot(device) = None;
    }
}

/// What a line under a field is: a refusal of what was asked, or help.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldTone {
    /// What was asked was refused: the danger colour, with an alert circle before it.
    Refusal,
    /// A hint, or where the field stands: muted, with no icon.
    Help,
}

/// One line under a field in a form, a dialog or a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSaid {
    /// Refusal or help.
    pub tone: FieldTone,
    /// What it says.
    pub said: Sentence,
}

impl FieldSaid {
    /// A refusal.
    pub fn refusal(said: Sentence) -> FieldSaid {
        FieldSaid { tone: FieldTone::Refusal, said }
    }

    /// A help line.
    pub fn help(said: Sentence) -> FieldSaid {
        FieldSaid { tone: FieldTone::Help, said }
    }

    /// Whether the field it is under is shown invalid.
    pub fn refuses(&self) -> bool {
        self.tone == FieldTone::Refusal
    }
}

/// One box wherever a note or a refusal is boxed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callout {
    /// Its status, which colours its edge and gives it a dot; none for a note with none.
    pub kind: Option<Status>,
    /// A title over the sentence.
    pub title: Option<Sentence>,
    /// What it says.
    pub said: Sentence,
    /// What it offers to do, at most one.
    pub action: Option<Action>,
    /// Whether a person may put it away with a ✕.
    pub dismissable: bool,
}

impl Callout {
    /// A callout saying `said` with no status, title, action or ✕.
    pub fn new(said: Sentence) -> Callout {
        Callout { kind: None, title: None, said, action: None, dismissable: false }
    }

    /// The same of a status.
    pub fn of(mut self, kind: Status) -> Callout {
        self.kind = Some(kind);
        self
    }

    /// The same under a title.
    pub fn titled(mut self, title: Sentence) -> Callout {
        self.title = Some(title);
        self
    }

    /// The same, offering `action`.
    pub fn offering(mut self, action: Action) -> Callout {
        self.action = Some(action);
        self
    }

    /// The same, put away with a ✕.
    pub fn dismissable(mut self) -> Callout {
        self.dismissable = true;
        self
    }
}

/// What a centre panel's round mark is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Something is being waited for: a turning circle.
    Waiting,
    /// The server does not answer.
    Offline,
    /// A server with no channel open.
    Channels,
    /// A server with no text channel to open, only voice ones to join.
    Voice,
    /// Threads.
    Threads,
    /// Direct messages.
    Conversations,
    /// A server with no category.
    Categories,
    /// Something failed.
    Failed,
}

/// What a centre panel offers, as a pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Try the server now, in place of waiting for the next try.
    TryNow,
    /// Show the channels, which a narrow window folds away.
    ShowChannels,
    /// Ask the list again.
    TryAgain(Roll),
}

impl Offer {
    /// The words of the pill.
    pub fn phrase(self) -> Phrase {
        match self {
            Offer::TryNow => Phrase::TryNow,
            Offer::ShowChannels => Phrase::ShowChannels,
            Offer::TryAgain(_) => Phrase::TryAgain,
        }
    }
}

/// What an area says when it has nothing else to show: a mark, a title, a line, and up to two pills, the first accented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CentrePanel {
    /// The round mark.
    pub mark: Mark,
    /// What it says; none while a list is only awaited.
    pub title: Option<Sentence>,
    /// A line under it.
    pub body: Option<Sentence>,
    /// What it offers.
    pub offers: Vec<Offer>,
}

impl CentrePanel {
    /// The panel of `place` while its connection is made, or unanswered: none once there is more to show.
    pub fn of(place: &Place, now: Instant, folded: bool) -> Option<CentrePanel> {
        let server = place.server().shown().to_string();
        Some(match place.panel() {
            Panel::Connecting => CentrePanel {
                mark: Mark::Waiting,
                title: Some(Sentence::Connecting(server)),
                body: Some(Sentence::Phrase(Phrase::PickAnother)),
                offers: Vec::new(),
            },
            Panel::NotAnswering { due } => CentrePanel {
                mark: Mark::Offline,
                title: Some(Sentence::NotAnswering(server)),
                body: Some(Sentence::TryingIn(due.checked_duration_since(now).map_or(0, crate::whole_seconds))),
                // A try under way is not interrupted; the panel is held with no wait left.
                offers: if due > now { vec![Offer::TryNow] } else { Vec::new() },
            },
            Panel::Lost | Panel::Channels => {
                let hint = place.start_hint();
                // A server with a text channel opens it by itself; one with voice channels only says to join one.
                let voice_only = hint == Phrase::PickVoiceChannel;
                CentrePanel {
                    mark: if voice_only { Mark::Voice } else { Mark::Channels },
                    title: Some(Sentence::Said(server)),
                    body: Some(Sentence::Phrase(hint)),
                    offers: if folded && !voice_only { vec![Offer::ShowChannels] } else { Vec::new() },
                }
            }
            Panel::SignedOut
            | Panel::Deleted
            | Panel::KeyChanged
            | Panel::NewPassword
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Call
            | Panel::Settings
            | Panel::Account => return None,
        })
    }

    /// The panel of an empty list of `roll`.
    pub fn without(roll: Roll) -> CentrePanel {
        let (mark, title, body) = match roll {
            Roll::Threads => (Mark::Threads, Phrase::NoThreadsYet, Phrase::NoThreadsBody),
            Roll::Conversations => (Mark::Conversations, Phrase::NoDirectMessagesYet, Phrase::NoDirectMessagesBody),
        };
        CentrePanel {
            mark,
            title: Some(Sentence::Phrase(title)),
            body: Some(Sentence::Phrase(body)),
            offers: Vec::new(),
        }
    }

    /// The panel of a server with no category, and so no channel.
    pub fn without_categories() -> CentrePanel {
        CentrePanel {
            mark: Mark::Categories,
            title: Some(Sentence::Phrase(Phrase::NoCategoriesYet)),
            body: Some(Sentence::Phrase(Phrase::NoCategoriesBody)),
            offers: Vec::new(),
        }
    }

    /// The panel of a list that is being fetched.
    pub fn waiting() -> CentrePanel {
        CentrePanel { mark: Mark::Waiting, title: None, body: None, offers: Vec::new() }
    }

    /// The panel of a list the server could not give.
    pub fn failed(roll: Roll) -> CentrePanel {
        CentrePanel {
            mark: Mark::Failed,
            title: Some(Sentence::Phrase(Phrase::ServerCouldntDoIt)),
            body: None,
            offers: vec![Offer::TryAgain(roll)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_news_is_brief_and_news_with_a_dot_or_an_action_stays_longer() {
        let said = || Sentence::Said("x".into());
        assert_eq!(Telling::plain(said()).lasts(), design::TOAST_PLAIN_FOR);
        assert_eq!(Telling::of(Status::Danger, said()).lasts(), design::TOAST_NOTED_FOR);
        assert_eq!(Telling::plain(said()).offering(Action::ShareAgain).lasts(), design::TOAST_NOTED_FOR);
        assert!(!Telling::plain(said()).dotted() && Telling::of(Status::Warning, said()).dotted());
    }

    #[test]
    fn a_fault_is_set_and_cleared_per_device() {
        let mut faults = Faults::default();
        faults.set(Device::Share, Sentence::Said("x".into()));
        assert!(faults.of(Device::Share).is_some() && faults.of(Device::Camera).is_none());
        faults.clear(Device::Share);
        assert_eq!(faults.of(Device::Share), None);
    }
}
