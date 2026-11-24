//! The menu open over the window, one at a time: a server's, where it opens, the items its server can answer now and
//! what each does; a channel's, with its Mute and Notification settings submenus, or a category's; a person's popover;
//! or the Language field's.

use super::*;

/// Where a server's menu was opened from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuFrom {
    /// The pane's header, under the server's name.
    Header,
    /// A right press on the server in the rail.
    Rail,
    /// A right press on the pane's channel list where no row is: what is made there, for whoever manages the
    /// channels.
    Pane,
}

/// A server's menu, open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The server whose menu it is.
    pub place: PlaceId,
    /// Where it was opened, which is where it is drawn.
    pub from: MenuFrom,
}

/// The menu open over the window: opening one closes another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// A server's.
    Server(Menu),
    /// The Language field's, on Settings › Appearance.
    Language,
    /// A channel's or a category's, in the side pane or on the channels page.
    List(ListMenu),
    /// A person's popover.
    Person(PersonOpen),
    /// An app's About.
    App(super::app_about::AppOpen),
}

/// A channel's or a category's menu, open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListMenu {
    /// The server whose list it is.
    pub place: PlaceId,
    /// The row it was opened on.
    pub key: place::ListKey,
    /// The submenu open beside it, if one is.
    pub sub: Option<ListSub>,
}

/// A submenu of a channel's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListSub {
    /// How long to mute the channel.
    Mute,
    /// The channel's level of notifications.
    Notify,
}

/// What an item of a channel's or a category's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListPick {
    /// Edit channel: the channels page opens on it.
    EditChannel,
    /// Rename channel: its name is asked.
    RenameChannel,
    /// Delete channel: asked first.
    DeleteChannel,
    /// Create channel: one is made in this category.
    CreateChannel,
    /// Rename category: its name is asked.
    RenameCategory,
    /// Delete category: asked first; its channels and their messages go with it.
    DeleteCategory,
    /// Mute channel ›, or Notification settings ›: its submenu opens beside it, or closes.
    Open(ListSub),
    /// Unmute channel: its mute is turned off.
    Unmute,
    /// The channel is muted for a while.
    MuteFor(crate::MuteFor),
    /// The channel's level of notifications picked.
    Level(crate::ChannelLevel),
}

/// An item of a channel's or a category's menu, as it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// What choosing it does.
    pub pick: ListPick,
    /// Its words.
    pub said: Phrase,
    /// It deletes: drawn in the danger colour.
    pub danger: bool,
    /// A rule sets it off from the items above.
    pub rule_before: bool,
    /// It opens a submenu: drawn with a chevron at its end.
    pub opens: bool,
}

/// An item of a channel's submenu, as it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubItem {
    /// What choosing it does.
    pub pick: ListPick,
    /// Its words.
    pub said: Phrase,
    /// What is written beside it: the server's level beside "Use server default".
    pub note: Option<Phrase>,
    /// The one taken, of a submenu of choices; none of a submenu of actions.
    pub chosen: Option<bool>,
}

/// A channel's submenu as drawn: the item of the menu it hangs from, by its place, and its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubMenuView {
    /// The place in the menu's items of the item it hangs from.
    pub from: usize,
    /// Its items, in order.
    pub items: Vec<SubItem>,
}

/// A channel's or a category's menu as drawn: what it is open on, its title and its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListMenuView {
    /// Where it is open.
    pub menu: ListMenu,
    /// Its title: a text channel's name after `#`, a voice channel's or a category's name.
    pub title: String,
    /// Its items, in order.
    pub items: Vec<ListItem>,
    /// The submenu open beside it, if one is.
    pub sub: Option<SubMenuView>,
}

/// What an item of a server's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Invite people: the invite dialog opens.
    Invite,
    /// Server settings: its first page opens.
    Settings,
    /// Sign in, on a server signed out here: it opens on its sign-in.
    SignIn,
    /// Copy the server's address to the clipboard.
    Copy,
    /// Sign out of the server on this device, asked first.
    SignOut,
    /// Remove the server from this device, asked first.
    Remove,
    /// Leave the server, which deletes the account there: asked first, by the password.
    Leave,
    /// Create category: its name is asked.
    CreateCategory,
    /// Notification settings: the server's dialog opens.
    Notify,
}

/// An item of a server's menu, as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What choosing it does.
    pub pick: Pick,
    /// The item's words, naming the server where they do.
    pub said: Sentence,
    /// It removes or leaves something.
    pub danger: bool,
    /// A rule sets it off from the items above.
    pub rule_before: bool,
}

impl Window {
    /// The rail is scrolled to `by`, where a menu opened from it is drawn.
    pub(super) fn on_rail_scrolled(&mut self, by: f32) -> Vec<Ask> {
        self.desk.rail_scrolled = by;
        Vec::new()
    }

    /// The menu put away.
    pub(super) fn on_close_menu(&mut self) -> Vec<Ask> {
        self.desk.menu = None;
        Vec::new()
    }

    /// How far the rail is scrolled from its start.
    pub fn rail_scrolled(&self) -> f32 {
        self.desk.rail_scrolled
    }

    /// The server's menu open, and its items.
    pub fn menu(&self) -> Option<(Menu, Vec<Entry>)> {
        let Some(Opened::Server(menu)) = self.desk.menu.as_ref() else { return None };
        Some((*menu, entries(self.place(menu.place)?, menu.from)))
    }

    /// The channel's or category's menu open, as drawn; none where its row is no longer listed. Every member mutes a
    /// channel and sets its notifications; editing items, and a category's menu, are for whoever manages the channels.
    pub fn list_menu(&self) -> Option<ListMenuView> {
        let Some(Opened::List(menu)) = self.desk.menu.as_ref() else { return None };
        let place = self.place(menu.place)?;
        let Some(Lobby::Ready(listed)) = place.lobby() else { return None };
        let manages = place.manages_channels();
        let item = |pick, said, danger, rule_before, opens| ListItem { pick, said, danger, rule_before, opens };
        let (title, items, sub) = match &menu.key {
            place::ListKey::Channel(id) => {
                let title = match listed.find(id)? {
                    place::ListEntry::Text(channel) => format!("#{}", channel.name),
                    place::ListEntry::Voice(channel) => channel.name.clone(),
                    place::ListEntry::Category(_) => return None,
                };
                let muted = self.times.wall.is_some_and(|now| place.channel_muted(id, now));
                let mute = if muted {
                    item(ListPick::Unmute, Phrase::UnmuteChannel, false, false, false)
                } else {
                    item(ListPick::Open(ListSub::Mute), Phrase::MuteChannel, false, false, true)
                };
                let mut items =
                    vec![mute, item(ListPick::Open(ListSub::Notify), Phrase::NotificationSettings, false, false, true)];
                if manages {
                    items.extend([
                        item(ListPick::EditChannel, Phrase::EditChannel, false, true, false),
                        item(ListPick::RenameChannel, Phrase::RenameChannel, false, false, false),
                        item(ListPick::DeleteChannel, Phrase::DeleteChannel, true, true, false),
                    ]);
                }
                let sub = menu.sub.and_then(|open| {
                    let from = items.iter().position(|item| item.pick == ListPick::Open(open))?;
                    Some(SubMenuView { from, items: sub_items(place, id, open) })
                });
                (title, items, sub)
            }
            place::ListKey::Category(id) if manages => {
                let category = listed.categories.iter().find(|category| category.id == *id)?;
                let items = vec![
                    item(ListPick::CreateChannel, Phrase::CreateChannel, false, false, false),
                    item(ListPick::RenameCategory, Phrase::RenameCategory, false, false, false),
                    item(ListPick::DeleteCategory, Phrase::DeleteCategory, true, true, false),
                ];
                (category.name.clone(), items, None)
            }
            place::ListKey::Category(_) => return None,
        };
        Some(ListMenuView { menu: menu.clone(), title, items, sub })
    }

    /// A channel's or a category's menu opened, by a right press, its gear or the menu key: a channel's for every
    /// member of a listed server, a category's for whoever manages the channels.
    pub(super) fn open_list_menu(&mut self, id: PlaceId, key: place::ListKey) -> Vec<Ask> {
        let offered = self.place(id).is_some_and(|place| match &key {
            place::ListKey::Channel(_) => matches!(place.lobby(), Some(Lobby::Ready(_))),
            place::ListKey::Category(_) => place.manages_channels(),
        });
        if offered {
            self.desk.menu = Some(Opened::List(ListMenu { place: id, key, sub: None }));
            self.desk.toast = None;
        }
        Vec::new()
    }

    /// An item of a channel's or a category's menu, or of a channel's submenu: what its menu no longer offers does
    /// nothing; one that opens a submenu leaves the menu open.
    pub(super) fn list_pick(&mut self, id: PlaceId, key: place::ListKey, pick: ListPick, at: Moment) -> Vec<Ask> {
        let offered = self.list_menu().is_some_and(|open| {
            open.menu.place == id
                && open.menu.key == key
                && (open.items.iter().any(|item| item.pick == pick)
                    || open.sub.is_some_and(|sub| sub.items.iter().any(|item| item.pick == pick)))
        });
        if let (true, ListPick::Open(sub), Some(Opened::List(open))) = (offered, pick, self.desk.menu.as_mut()) {
            open.sub = (open.sub != Some(sub)).then_some(sub);
            return Vec::new();
        }
        self.desk.menu = None;
        if !offered {
            return Vec::new();
        }
        let notify = |message| place::Message::Notify(message);
        match (key, pick) {
            (place::ListKey::Channel(channel), ListPick::Unmute) => {
                self.at(id, notify(crate::NotifyMessage::UnmuteChannel(channel)), at)
            }
            (place::ListKey::Channel(channel), ListPick::MuteFor(span)) => {
                self.at(id, notify(crate::NotifyMessage::MuteChannel(channel, span)), at)
            }
            (place::ListKey::Channel(channel), ListPick::Level(level)) => {
                self.at(id, notify(crate::NotifyMessage::ChannelLevel(channel, level)), at)
            }
            (place::ListKey::Channel(channel), ListPick::EditChannel) => {
                let mut todo = self.at(id, place::Message::Page(place::PageMessage::Settings(Page::Channels)), at);
                let choose = crate::channel_settings::Message::Choose(channel);
                todo.extend(self.at(id, place::Message::Page(place::PageMessage::Channels(choose)), at));
                todo
            }
            (place::ListKey::Channel(channel), ListPick::RenameChannel) => {
                self.channel(id, crate::channels::ChannelAction::Rename, &channel)
            }
            (place::ListKey::Channel(channel), ListPick::DeleteChannel) => {
                self.channel(id, crate::channels::ChannelAction::Delete, &channel)
            }
            (place::ListKey::Category(category), ListPick::CreateChannel) => self.create_channel(id, Some(category)),
            (place::ListKey::Category(category), ListPick::RenameCategory) => {
                self.category(id, crate::channels::ChannelAction::Rename, &category)
            }
            (place::ListKey::Category(category), ListPick::DeleteCategory) => {
                self.category(id, crate::channels::ChannelAction::Delete, &category)
            }
            // A submenu opens above; the rest are the other kind of row's.
            (_, ListPick::Open(_))
            | (
                place::ListKey::Channel(_),
                ListPick::CreateChannel | ListPick::RenameCategory | ListPick::DeleteCategory,
            )
            | (
                place::ListKey::Category(_),
                ListPick::EditChannel
                | ListPick::RenameChannel
                | ListPick::DeleteChannel
                | ListPick::Unmute
                | ListPick::MuteFor(_)
                | ListPick::Level(_),
            ) => Vec::new(),
        }
    }

    /// An item of a server's menu: the server opens, then the item does its work. An item its menu no longer offers
    /// (the server's state changed under it) does nothing.
    pub(super) fn pick(&mut self, id: PlaceId, pick: Pick, at: Moment) -> Vec<Ask> {
        self.desk.menu = None;
        let Some(place) = self.place(id) else { return Vec::new() };
        let offered = |from| entries(place, from).iter().any(|entry| entry.pick == pick);
        if !(offered(MenuFrom::Header) || offered(MenuFrom::Pane)) {
            return Vec::new();
        }
        let owner = place.role_place().is_owner();
        let call = place.live_call().map(|channel| channel.name.clone());
        let host = place.server().host().to_string();
        let first_page = place.pages().first().copied();
        let mut todo = if self.desk.open == Some(id) { Vec::new() } else { self.update(Message::Open(id), at) };
        let asking = match pick {
            Pick::SignIn => return todo,
            Pick::Copy => {
                self.desk.copying = Some(Copying { place: id, text: Secret::new(host), what: Copied::Address });
                return todo;
            }
            Pick::Settings => {
                if let Some(page) = first_page {
                    todo.extend(self.at(id, place::Message::Page(place::PageMessage::Settings(page)), at));
                }
                return todo;
            }
            Pick::Invite => {
                // The first arrival's note offers it too, and goes once it is taken.
                todo.extend(self.at(id, place::Message::Lobby(place::LobbyMessage::Dismiss), at));
                Asking::Invite(Inviting::default())
            }
            Pick::SignOut => Asking::SignOut,
            Pick::Remove => Asking::Remove,
            Pick::Leave if owner => Asking::OwnerLeaves,
            Pick::Leave => Asking::Leave(Proof::default()),
            Pick::CreateCategory => {
                todo.extend(self.create_category(id));
                return todo;
            }
            Pick::Notify => {
                todo.extend(self.at(id, place::Message::Notify(crate::NotifyMessage::OpenDialog), at));
                return todo;
            }
        };
        self.desk.dialog = Some(Dialog { place: id, asking, call });
        todo.extend(self.wake());
        todo
    }

    /// `id`'s menu opened from `from`, if the window holds it.
    pub(super) fn open_menu(&mut self, id: PlaceId, from: MenuFrom) -> Vec<Ask> {
        if self.place(id).is_some_and(|place| !entries(place, from).is_empty()) {
            self.desk.menu = Some(Opened::Server(Menu { place: id, from }));
            self.desk.toast = None;
        }
        Vec::new()
    }
}

/// A channel's submenu: the durations of a mute, or its levels with the server's beside "Use server default".
fn sub_items(place: &Place, channel: &ChannelId, open: ListSub) -> Vec<SubItem> {
    match open {
        ListSub::Mute => crate::MuteFor::ALL
            .into_iter()
            .map(|span| SubItem { pick: ListPick::MuteFor(span), said: span.phrase(), note: None, chosen: None })
            .collect(),
        ListSub::Notify => {
            let notifying = place.notifying().cloned().unwrap_or_default();
            let own = notifying.channel(channel).level;
            let default = SubItem {
                pick: ListPick::Level(crate::ChannelLevel::ServerDefault),
                said: Phrase::UseServerDefault,
                note: Some(crate::level_phrase(notifying.server.level)),
                chosen: Some(own.is_none()),
            };
            std::iter::once(default)
                .chain(proto::notify::Level::ALL.into_iter().map(|level| SubItem {
                    pick: ListPick::Level(crate::ChannelLevel::Own(level)),
                    said: crate::level_phrase(level),
                    note: None,
                    chosen: Some(own == Some(level)),
                }))
                .collect()
        }
    }
}

/// The items of a server's menu opened from `from`, by what the server can answer now: sign out and leave need it to
/// answer. From the pane's list, what is made there, and nothing for who does not manage the channels.
fn entries(place: &Place, from: MenuFrom) -> Vec<Entry> {
    let name = place.server().shown().to_string();
    let entry = |pick, said: Sentence, danger, rule_before| Entry { pick, said, danger, rule_before };
    if from == MenuFrom::Pane {
        return if place.manages_channels() && place.panel() != Panel::Lost {
            vec![entry(Pick::CreateCategory, Sentence::Phrase(Phrase::CreateCategory), false, false)]
        } else {
            Vec::new()
        };
    }
    let copy = entry(Pick::Copy, Sentence::Phrase(Phrase::CopyAddress), false, false);
    let grants = place.grants();
    let invite = grants
        .holds(proto::rules::Grant::InviteMember)
        .then(|| entry(Pick::Invite, Sentence::Phrase(Phrase::InvitePeople), false, false));
    let settings = (!place.pages().is_empty())
        .then(|| entry(Pick::Settings, Sentence::Phrase(Phrase::ServerSettings), false, false));
    let notify = entry(Pick::Notify, Sentence::Phrase(Phrase::NotificationSettings), false, false);
    match place.panel() {
        Panel::Channels
        | Panel::Discussion
        | Panel::Threads
        | Panel::Conversations
        | Panel::Call
        | Panel::Settings
        | Panel::Account => [invite, settings]
            .into_iter()
            .flatten()
            .chain([
                notify,
                copy,
                entry(Pick::SignOut, Sentence::SignOutOf(name.clone()), false, true),
                entry(Pick::Remove, Sentence::Phrase(Phrase::RemoveHere), false, false),
                entry(Pick::Leave, Sentence::Leave(name), true, true),
            ])
            .collect(),
        Panel::Connecting | Panel::Lost | Panel::NotAnswering { .. } | Panel::Deleted | Panel::KeyChanged => {
            vec![copy, entry(Pick::Remove, Sentence::Phrase(Phrase::RemoveHere), false, true)]
        }
        Panel::SignedOut | Panel::NewPassword => vec![
            entry(Pick::SignIn, Sentence::Phrase(Phrase::SignInAction), false, false),
            copy,
            entry(Pick::Remove, Sentence::Phrase(Phrase::RemoveHere), false, true),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::Heard;
    use crate::harness::{
        ADA, Clock, accepted, books, ended, flow, holding, id, in_call, inviter, owls, standing_as, tell, token,
        two_signed_in,
    };
    use crate::sign_in::{Report, Standing};
    use proto::chatkord_v1::CloseReason;
    use proto::rules::{Grant, Grants};
    use terms::{Ended, Secret};

    fn picks(window: &Window) -> Vec<Pick> {
        window.menu().unwrap().1.into_iter().map(|entry| entry.pick).collect()
    }

    #[test]
    fn a_servers_menu_offers_what_the_server_can_answer_now() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: Grants::default(),
            }),
        );
        window.desk.toast = Some(Toast::new(owls_id, Telling::plain(Sentence::Said("news".into())), clock.0));
        window.update(Message::Menu(owls_id, MenuFrom::Header), clock.at());
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "a menu puts the toast away"
        );
        let (menu, entries) = window.menu().unwrap();
        assert_eq!(menu, Menu { place: owls_id, from: MenuFrom::Header });
        let said: Vec<(String, bool, bool)> =
            entries.iter().map(|entry| (entry.said.in_english(), entry.danger, entry.rule_before)).collect();
        assert_eq!(
            said.iter().map(|(said, danger, rule)| (said.as_str(), *danger, *rule)).collect::<Vec<_>>(),
            [
                ("Notification settings", false, false),
                ("Copy server address", false, false),
                ("Sign out of Night Owls", false, true),
                ("Remove from this device", false, false),
                ("Leave Night Owls", true, true),
            ]
        );
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Disconnected(Ended { silent: true, ..Ended::default() })),
        );
        assert_eq!(picks(&window), [Pick::Copy, Pick::Remove], "lost: sign out and leave need an answer");
        tell(&mut window, &clock, books_id, ended(CloseReason::PasswordReset));
        window.update(Message::Menu(books_id, MenuFrom::Rail), clock.at());
        assert_eq!(picks(&window), [Pick::SignIn, Pick::Copy, Pick::Remove]);
        tell(
            &mut window,
            &clock,
            books_id,
            place::Message::Session(place::SessionMessage::Door(door::Message::Reported(Heard::Flow(token())))),
        );
        let (mut deleted, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let gone = id(&deleted, 0);
        tell(&mut deleted, &clock, gone, flow(token()));
        tell(&mut deleted, &clock, gone, flow(accepted(Grants::default())));
        tell(&mut deleted, &clock, gone, ended(CloseReason::AccountDeleted));
        deleted.update(Message::Menu(gone, MenuFrom::Rail), clock.at());
        assert_eq!(picks(&deleted), [Pick::Copy, Pick::Remove], "a deleted account has nothing to sign in to");
        window.update(Message::CloseMenu, clock.at());
        assert!(window.menu().is_none());
    }

    #[test]
    fn an_item_on_another_servers_menu_opens_that_server_and_one_no_longer_offered_does_nothing() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls_id, books_id) = (id(&window, 0), id(&window, 1));
        window.update(Message::Menu(books_id, MenuFrom::Rail), clock.at());
        let todo = window.update(Message::Pick(books_id, Pick::SignOut), clock.at());
        assert!(matches!(&todo[..], [Ask::RememberOpen(url)] if *url == books().url), "{todo:?}");
        assert_eq!(window.open().unwrap().id(), books_id);
        assert!(window.menu().is_none());
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::SignOut)));
        window.update(Message::Dialog(DialogMessage::Cancel), clock.at());
        assert!(window.dialog().is_none());
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Disconnected(Ended { silent: true, ..Ended::default() })),
        );
        assert!(window.update(Message::Pick(owls_id, Pick::Leave), clock.at()).is_empty());
        assert!(window.dialog().is_none(), "Leave is not offered while the connection is lost");
    }

    #[test]
    fn signing_out_asks_first_ends_the_call_here_and_says_why_on_the_panel() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        window.update(Message::Pick(owls_id, Pick::SignOut), clock.at());
        assert_eq!(window.dialog().unwrap().call(), Some("Lounge"), "the dialog says the call ends too");
        let todo = window.update(Message::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::EndAccount { identity: Some(identity), sign_out: true, row: place::Row::SignedOut, .. })] if *at == owls_id && identity == ADA),
            "{todo:?}"
        );
        let place = &window.places()[0];
        assert_eq!((place.panel(), place.call().is_none()), (Panel::SignedOut, true));
        assert_eq!(place.door().unwrap().why().unwrap().in_english(), "You signed out of Night Owls on this device.");
        assert_eq!(place.door().unwrap().username(), ADA);
        // The server closes the socket that asked; that is no reason to connect again.
        assert!(tell(&mut window, &clock, owls_id, ended(CloseReason::ClientShutdown)).is_empty());
        assert_eq!(window.places()[0].panel(), Panel::SignedOut);
    }

    #[test]
    fn a_temporary_password_can_be_signed_out_of_instead() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Server(books())], None, clock.0);
        let books_id = id(&window, 0);
        let door = |message| place::Message::Session(place::SessionMessage::Door(message));
        tell(&mut window, &clock, books_id, door(door::Message::Username(ADA.into())));
        tell(&mut window, &clock, books_id, door(door::Message::Password(Secret::new("river-copper-lamp-42".into()))));
        tell(&mut window, &clock, books_id, door(door::Message::Submit));
        let temporary = Standing { must_change: true, ..standing_as("ada") };
        tell(&mut window, &clock, books_id, flow(Report::Accepted { token: None, named: None, standing: temporary }));
        assert_eq!(window.places()[0].panel(), Panel::NewPassword);
        let todo = tell(&mut window, &clock, books_id, place::Message::Session(place::SessionMessage::SignOutInstead));
        assert!(
            matches!(
                &todo[..],
                [Ask::At(_, place::Ask::EndAccount { sign_out: true, row: place::Row::SignedOut, .. })]
            ),
            "{todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::SignedOut);
    }

    #[test]
    fn copying_the_address_is_toasted_once_it_took() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        window.update(Message::Pick(owls_id, Pick::Copy), clock.at());
        assert_eq!(window.copying(), Some("chat.example.invalid"));
        window.update(Message::Copied(Ok(())), clock.at());
        assert_eq!(
            (window.copying(), window.toast().map(|toast| toast.said().in_english()).as_deref()),
            (None, Some("Address of Night Owls copied"))
        );
        window.update(Message::Pick(owls_id, Pick::Copy), clock.at());
        window.update(Message::Copied(Err(super::NotCopied)), clock.at());
        assert_eq!(window.copying(), None);
    }

    #[test]
    fn inviting_and_the_settings_are_offered_by_the_words_held_in_the_order_drawn() {
        let clock = Clock(Instant::now());
        let (mut member, owls_id) = holding(&clock, Grants::default());
        member.update(Message::Menu(owls_id, MenuFrom::Header), clock.at());
        assert_eq!(picks(&member), [Pick::Notify, Pick::Copy, Pick::SignOut, Pick::Remove, Pick::Leave]);
        for (grants, creates) in
            [([Grant::InviteMember].into_iter().collect(), false), (Grant::ALL.into_iter().collect::<Grants>(), true)]
        {
            let (mut window, owls_id) = holding(&clock, grants);
            window.update(Message::Menu(owls_id, MenuFrom::Header), clock.at());
            let said: Vec<(String, bool)> = window
                .menu()
                .unwrap()
                .1
                .into_iter()
                .map(|entry| (entry.said.in_english(), entry.rule_before))
                .collect();
            assert_eq!(
                said,
                [
                    ("Invite people".to_string(), false),
                    ("Server settings".into(), false),
                    ("Notification settings".into(), false),
                    ("Copy server address".into(), false),
                    ("Sign out of Night Owls".into(), true),
                    ("Remove from this device".into(), false),
                    ("Leave Night Owls".into(), true),
                ],
                "nothing made here: that is the list's own menu, and the settings'"
            );
            // The list's menu, a right press where no row is: what is made there, for Manage channels alone.
            window.update(Message::CloseMenu, clock.at());
            window.update(Message::Menu(owls_id, MenuFrom::Pane), clock.at());
            let made: Vec<Pick> =
                window.menu().map(|(_, entries)| entries.iter().map(|entry| entry.pick).collect()).unwrap_or_default();
            let expected: &[Pick] = if creates { &[Pick::CreateCategory] } else { &[] };
            assert_eq!(made, expected, "and no empty menu opens for who makes nothing");
            if creates {
                window.update(Message::Pick(owls_id, Pick::CreateCategory), clock.at());
                assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::Name(_))), "its name is asked");
            }
        }
    }

    #[test]
    fn a_word_taken_closes_the_settings_and_the_menu_no_longer_offers_them() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = inviter(&clock);
        let todo = window.update(Message::Pick(owls_id, Pick::Settings), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListInvites)),
            "{todo:?}"
        );
        assert_eq!(window.open().unwrap().settings().map(crate::ServerSettings::page), Some(Page::Invites));
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: Grants::default(),
            }),
        );
        assert_eq!(window.open().unwrap().panel(), Panel::Discussion);
        window.update(Message::Menu(owls_id, MenuFrom::Header), clock.at());
        assert!(!picks(&window).contains(&Pick::Settings) && !picks(&window).contains(&Pick::Invite));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::CloseSettings));
    }
}
