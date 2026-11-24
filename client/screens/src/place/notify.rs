//! How much the server and each of its channels alert this account: read at each sign-in, told whole when they change,
//! and set from the server's Notification settings dialog and a channel's menu, shown at once and read again when a
//! change is refused. A mute is until a time the wall clock gives, or until turned off.

use super::*;
use proto::notify::{ChannelSettings, Level, Mute, ServerSettings};

/// The account's settings at the server, whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notifying {
    /// The server's.
    pub server: ServerSettings,
    /// Each channel's that has one; a channel left out follows the server.
    pub channels: std::collections::HashMap<ChannelId, ChannelSettings>,
}

impl Notifying {
    /// The settings the wire states; a channel's out of shape, or with no id, is passed over, and a server's out of
    /// shape reads as the default, a server not keeping the protocol.
    pub fn read(told: &proto::chatkord_v1::NotifySettings) -> Notifying {
        let server = ServerSettings::of_wire(told.server.as_ref()).unwrap_or_default();
        let channels = told
            .channels
            .iter()
            .filter_map(|channel| {
                Some((ChannelId::parse(&channel.channel_id)?, ChannelSettings::of_wire(channel).ok()?))
            })
            .collect();
        Notifying { server, channels }
    }

    /// `channel`'s own settings: none set reads as following the server.
    pub fn channel(&self, channel: &ChannelId) -> ChannelSettings {
        self.channels.get(channel).copied().unwrap_or_default()
    }

    /// Whether `channel` is muted at `now`, by its own mute or the server's.
    pub fn muted(&self, channel: &ChannelId, now: i64) -> bool {
        let holds = |mute: Option<Mute>| mute.is_some_and(|mute| mute.holds_at(now));
        holds(self.server.mute) || holds(self.channel(channel).mute)
    }

    /// The level that holds in `channel`: its own, else the server's.
    pub fn level(&self, channel: &ChannelId) -> Level {
        self.channel(channel).level.unwrap_or(self.server.level)
    }
}

/// How long a mute picked in a menu lasts, as Discord's menus offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MuteFor {
    /// 15 minutes.
    Minutes15,
    /// An hour.
    Hour,
    /// 8 hours.
    Hours8,
    /// 24 hours.
    Hours24,
    /// Until turned off.
    UntilTurnedOn,
}

impl MuteFor {
    /// Every duration, in the menu's order.
    pub const ALL: [MuteFor; 5] =
        [MuteFor::Minutes15, MuteFor::Hour, MuteFor::Hours8, MuteFor::Hours24, MuteFor::UntilTurnedOn];

    /// Its words in the menu.
    pub const fn phrase(self) -> Phrase {
        match self {
            MuteFor::Minutes15 => Phrase::MuteFor15Minutes,
            MuteFor::Hour => Phrase::MuteFor1Hour,
            MuteFor::Hours8 => Phrase::MuteFor8Hours,
            MuteFor::Hours24 => Phrase::MuteFor24Hours,
            MuteFor::UntilTurnedOn => Phrase::MuteUntilTurnedOn,
        }
    }

    /// The mute it makes when picked at `now`, in seconds since the epoch.
    pub const fn from(self, now: i64) -> Mute {
        let minutes = match self {
            MuteFor::Minutes15 => 15,
            MuteFor::Hour => 60,
            MuteFor::Hours8 => 8 * 60,
            MuteFor::Hours24 => 24 * 60,
            MuteFor::UntilTurnedOn => return Mute::Forever,
        };
        Mute::Until(now.saturating_add(minutes * 60))
    }
}

/// A level's words in the dialog and the menus.
pub const fn level_phrase(level: Level) -> Phrase {
    match level {
        Level::All => Phrase::NotifyAll,
        Level::Mentions => Phrase::NotifyMentions,
        Level::Nothing => Phrase::NotifyNothing,
    }
}

/// A channel's level as its menu offers it: the server's, or its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelLevel {
    /// Use the server's level.
    ServerDefault,
    /// Its own.
    Own(Level),
}

/// The server's Notification settings dialog, open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotifyDialog {
    /// Its Mute row's menu of durations is open.
    pub mute_menu: bool,
}

/// What the account's notification settings are, and what a person does with them.
#[derive(Clone, Debug)]
pub enum NotifyMessage {
    /// The settings, read, or why not.
    Read(Result<Notifying, LinkError>),
    /// The settings as the server tells them now, whole.
    Changed(Notifying),
    /// The server's answer to a change.
    Set(Result<(), LinkError>),
    /// "Notification settings" in the server's menu: its dialog opens.
    OpenDialog,
    /// Done, Escape or a press outside: the dialog closes.
    CloseDialog,
    /// The dialog's Mute row pressed: its menu of durations opens, or closes; a mute on is turned off.
    MuteRow,
    /// Escape or a press outside the Mute row's menu of durations: it closes.
    CloseMuteMenu,
    /// The server muted for a while.
    MuteServer(MuteFor),
    /// "Notify me about", chosen.
    ServerLevel(Level),
    /// "Suppress @everyone and @here" switched.
    SuppressEveryone,
    /// "Suppress role mentions" switched.
    SuppressRoles,
    /// A channel's menu: muted for a while.
    MuteChannel(ChannelId, MuteFor),
    /// A channel's menu: its mute turned off.
    UnmuteChannel(ChannelId),
    /// A channel's menu: its level picked.
    ChannelLevel(ChannelId, ChannelLevel),
}

impl Place {
    /// The account's notification settings here, once read.
    pub fn notifying(&self) -> Option<&Notifying> {
        self.notify.as_ref()
    }

    /// The server's Notification settings dialog, when it is open.
    pub fn notify_dialog(&self) -> Option<NotifyDialog> {
        match &self.status {
            Status::Signed(signed) => signed.notify_dialog,
            Status::Reconnecting(_) | Status::Door(_) => None,
        }
    }

    /// Whether the whole server is muted at `now` (seconds since the epoch): its dialog's Mute row turns it off.
    pub fn server_muted(&self, now: i64) -> bool {
        self.notify.as_ref().and_then(|notify| notify.server.mute).is_some_and(|mute| mute.holds_at(now))
    }

    /// Whether `channel` is muted at `now` (seconds since the epoch): the list greys it and its unread mark goes.
    pub fn channel_muted(&self, channel: &ChannelId, now: i64) -> bool {
        self.notify.as_ref().is_some_and(|notify| notify.muted(channel, now))
    }

    /// The account's notification settings: read, told, or changed by the person.
    pub(super) fn on_notify(&mut self, message: NotifyMessage, at: Moment) -> Vec<Ask> {
        if let NotifyMessage::Read(read) = message {
            if let Ok(read) = read {
                self.notify = Some(read);
            }
            return Vec::new();
        }
        if let NotifyMessage::Changed(told) = message {
            self.notify = Some(told);
            return Vec::new();
        }
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match message {
            NotifyMessage::Read(_) | NotifyMessage::Changed(_) => Vec::new(),
            // A change refused is put back by reading what the server keeps.
            NotifyMessage::Set(answer) => match answer {
                Ok(()) => Vec::new(),
                Err(_) => vec![ops::get_notify()],
            },
            NotifyMessage::OpenDialog => {
                signed.notify_dialog = Some(NotifyDialog::default());
                Vec::new()
            }
            NotifyMessage::CloseDialog => {
                signed.notify_dialog = None;
                Vec::new()
            }
            NotifyMessage::MuteRow => {
                let Some(notify) = self.notify.as_mut() else { return Vec::new() };
                if notify.server.mute.is_some_and(|mute| mute.holds_at(at.wall)) {
                    notify.server.mute = None;
                    return vec![ops::set_server_notify(notify.server)];
                }
                if let Some(dialog) = signed.notify_dialog.as_mut() {
                    dialog.mute_menu = !dialog.mute_menu;
                }
                Vec::new()
            }
            NotifyMessage::CloseMuteMenu => {
                if let Some(dialog) = signed.notify_dialog.as_mut() {
                    dialog.mute_menu = false;
                }
                Vec::new()
            }
            NotifyMessage::MuteServer(span) => {
                if let Some(dialog) = signed.notify_dialog.as_mut() {
                    dialog.mute_menu = false;
                }
                self.change_server(|server| server.mute = Some(span.from(at.wall)))
            }
            NotifyMessage::ServerLevel(level) => self.change_server(|server| server.level = level),
            NotifyMessage::SuppressEveryone => {
                self.change_server(|server| server.suppress_everyone = !server.suppress_everyone)
            }
            NotifyMessage::SuppressRoles => self.change_server(|server| server.suppress_roles = !server.suppress_roles),
            NotifyMessage::MuteChannel(channel, span) => {
                self.change_channel(&channel, |settings| settings.mute = Some(span.from(at.wall)))
            }
            NotifyMessage::UnmuteChannel(channel) => self.change_channel(&channel, |settings| settings.mute = None),
            NotifyMessage::ChannelLevel(channel, level) => self.change_channel(&channel, |settings| {
                settings.level = match level {
                    ChannelLevel::ServerDefault => None,
                    ChannelLevel::Own(level) => Some(level),
                };
            }),
        }
    }

    fn change_server(&mut self, change: impl FnOnce(&mut ServerSettings)) -> Vec<Ask> {
        let Some(notify) = self.notify.as_mut() else { return Vec::new() };
        let before = notify.server;
        change(&mut notify.server);
        if notify.server == before {
            return Vec::new();
        }
        vec![ops::set_server_notify(notify.server)]
    }

    fn change_channel(&mut self, channel: &ChannelId, change: impl FnOnce(&mut ChannelSettings)) -> Vec<Ask> {
        let Some(notify) = self.notify.as_mut() else { return Vec::new() };
        let before = notify.channel(channel);
        let mut settings = before;
        change(&mut settings);
        if settings == before {
            return Vec::new();
        }
        if settings.follows() {
            notify.channels.remove(channel);
        } else {
            notify.channels.insert(channel.clone(), settings);
        }
        vec![ops::set_channel_notify(channel, settings)]
    }
}
