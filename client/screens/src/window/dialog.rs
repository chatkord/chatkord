//! The confirmation open over the window, about one server: what it asks, what is done in it, and the server's
//! answers to it.

use super::*;
use crate::place::ops;
use crate::shortened::shortened;

/// What a confirmation asks.
#[derive(Debug)]
pub enum Asking {
    /// Signing out of the server on this device.
    SignOut,
    /// Removing the server from this device.
    Remove,
    /// Leaving a server, proved by the current password.
    Leave(Proof),
    /// A server's owner cannot leave until someone else owns it.
    OwnerLeaves,
    /// Inviting people: the terms, then the link.
    Invite(Inviting),
    /// Deleting one of the server’s invites.
    DeleteInvite(DeletingInvite),
    /// Resetting a member's password, then the temporary one to hand over.
    Reset(Resetting),
    /// Making a member the server owner.
    Transfer(Transferring),
    /// Signing a member out everywhere.
    Kick(Kicking),
    /// Banning a member, or lifting their ban.
    Ban(Banning),
    /// A name for a new role.
    CreateRole(Creating),
    /// Deleting a role.
    DeleteRole(DeletingRole),
    /// A new channel: its kind, name, category and who sees it or writes in it.
    MakeChannel(Making),
    /// A new name for a channel, a new category's name, or a new name for one.
    Name(Naming),
    /// Deleting a channel.
    DeleteChannel(Deleting),
    /// Deleting a category, which keeps its channels.
    DeleteCategory(DeletingCategory),
    /// A name and a colour for a server its owner just arrived at.
    SetUp(SetUp),
    /// Closing the window, which ends the call held at this server.
    Quit,
    /// Deleting one of the server's emoji, and its reactions.
    DeleteEmoji(DeletingEmoji),
    /// Deleting a message.
    DeleteMessage(DeletingMessage),
}

/// The longest line of a message's text a confirmation quotes, in characters.
const QUOTED_CHARS: usize = 80;

/// A message asked to be deleted: which, who wrote it and the start of what it says, as the confirmation shows them.
#[derive(Debug)]
pub struct DeletingMessage {
    pane: place::Pane,
    message: chat::MessageId,
    author: Option<String>,
    line: Option<String>,
}

impl DeletingMessage {
    /// The name its writer is shown by; none for one who is gone.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// The first line of its text, cut at a bound with an ellipsis; none for a message with no text.
    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }
}

impl Confirming for DeletingMessage {
    fn holds_outside(&self) -> bool {
        false
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm => {
                Next::Telling(self.pane.carrying(place::DiscussionMessage::DeleteConfirmed(self.message.clone())))
            }
            _ => Next::Stays,
        }
    }
}

/// The first non-empty line `body` shows, its markdown markers left out, cut to `QUOTED_CHARS` characters with an
/// ellipsis.
fn first_line(body: &chat::Body) -> Option<String> {
    body.first_line_shown().map(|line| shortened(&line, QUOTED_CHARS))
}

/// An emoji asked to be deleted: which, and its name as the confirmation says it.
#[derive(Debug)]
pub struct DeletingEmoji {
    emoji: chat::EmojiId,
    name: proto::rules::EmojiName,
}

impl DeletingEmoji {
    /// The emoji's name, without colons.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

impl Confirming for DeletingEmoji {
    fn holds_outside(&self) -> bool {
        false
    }

    // The page says a refusal; the confirmation has nothing to wait for.
    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm => Next::Closing(vec![ops::delete_emoji(&self.emoji)]),
            _ => Next::Stays,
        }
    }
}

/// A confirmation open over the window, about one server.
#[derive(Debug)]
pub struct Dialog {
    pub(super) place: PlaceId,
    pub(super) asking: Asking,
    /// The name of the channel of this window's call when the server holds it: confirming leaves the call too.
    pub(super) call: Option<String>,
}

impl Dialog {
    /// The server it is about.
    pub fn place(&self) -> PlaceId {
        self.place
    }

    /// What it asks, with what is typed or chosen in it so far.
    pub fn asking(&self) -> &Asking {
        &self.asking
    }

    /// The name of the channel of this window's call when the server holds it: confirming leaves the call too.
    pub fn call(&self) -> Option<&str> {
        self.call.as_deref()
    }
}

/// What a person does in a confirmation.
#[derive(Clone, Debug)]
pub enum DialogMessage {
    /// The current password, as typed.
    Password(Secret),
    /// The button beside the password that shows or hides it.
    Reveal,
    /// Its main button, or Enter.
    Confirm,
    /// Cancel, its ✕, or Escape.
    Cancel,
    /// A press outside it: ignored while something is typed or made, or an answer awaited.
    Outside,
    /// Something done in the invite dialog.
    Invite(InviteMessage),
    /// Copy beside a temporary password.
    CopyTemporary,
    /// A name, as typed: a channel's, a new role's.
    Named(String),
    /// Something done in the set-up dialog a server's owner sees first.
    SetUp(SetUpMessage),
    /// Something done in the create-channel dialog, apart from its name.
    Making(MakingMessage),
}

impl Asking {
    /// The feature whose dialog it is, which takes what is done in it; none for a question of the window's own.
    fn feature(&mut self) -> Option<&mut dyn Confirming> {
        match self {
            Asking::Leave(proof) => Some(proof),
            Asking::Invite(inviting) => Some(inviting),
            Asking::DeleteInvite(deleting) => Some(deleting),
            Asking::Reset(resetting) => Some(resetting),
            Asking::Transfer(transferring) => Some(transferring),
            Asking::Kick(kicking) => Some(kicking),
            Asking::Ban(banning) => Some(banning),
            Asking::CreateRole(creating) => Some(creating),
            Asking::DeleteRole(deleting) => Some(deleting),
            Asking::MakeChannel(making) => Some(making),
            Asking::Name(naming) => Some(naming),
            Asking::DeleteChannel(deleting) => Some(deleting),
            Asking::DeleteCategory(deleting) => Some(deleting),
            Asking::SetUp(first) => Some(first),
            Asking::DeleteEmoji(deleting) => Some(deleting),
            Asking::DeleteMessage(deleting) => Some(deleting),
            Asking::SignOut | Asking::Remove | Asking::OwnerLeaves | Asking::Quit => None,
        }
    }
}

/// What becomes of a confirmation once its feature has taken what a person did in it or the server's answer, for the
/// window to carry out.
#[derive(Debug)]
pub(crate) enum Next {
    /// It stays open, as it now is.
    Stays,
    /// It closes.
    Closes,
    /// The server is asked this, the dialog open for the answer.
    Asks(place::Ask),
    /// It closes, and the server is asked these.
    Closing(Vec<place::Ask>),
    /// It closes, and its place is told this, which decides what is asked.
    Telling(place::Message),
    /// Its text goes on the clipboard, and its button says so once it went.
    Copies(Secret, Copied),
    /// It closes and the server's settings open on this page.
    Opens(Page),
    /// It asks this instead.
    Becomes(Asking),
    /// It closes, and a toast says this.
    Done(Sentence),
}

/// A feature's confirmation: what keeps it open, and what is done in it.
pub(crate) trait Confirming {
    /// Whether a press outside leaves it open: something typed or made would be lost, or an answer is awaited.
    fn holds_outside(&self) -> bool;

    /// Whether Cancel leaves it open too: an answer under way would be lost.
    fn holds_cancel(&self) -> bool {
        false
    }

    /// What `said`, never Cancel or a press outside, does to it.
    fn said(&mut self, said: DialogMessage) -> Next;
}

impl Window {
    /// Whether the dialog open is the one setting up server `id`, which a picture picked belongs to.
    pub(super) fn asking_set_up(&self, id: PlaceId) -> bool {
        self.desk.dialog.as_ref().is_some_and(|open| open.place == id && matches!(open.asking, Asking::SetUp(_)))
    }

    /// A picture picked for the server being set up, said to its dialog.
    pub(super) fn on_picture(&mut self, message: crate::picture_row::Message, at: Moment) -> Vec<Ask> {
        self.update(Message::Dialog(DialogMessage::SetUp(SetUpMessage::Picture(message))), at)
    }

    /// The confirmation open over the window, if one is.
    pub fn dialog(&self) -> Option<&Dialog> {
        self.desk.dialog.as_ref()
    }

    /// A role ticked in a member's Roles menu, given or taken at once with the menu left open; an entry of their More
    /// menu, or Unban, asked first. One the row no longer offers (the list or the account's standing changed under it)
    /// does nothing.
    pub(super) fn member(&mut self, id: PlaceId, action: MemberAction, user_id: &MemberId) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let Some(Loaded::Ready(members)) = place.settings().map(crate::ServerSettings::members) else {
            return Vec::new();
        };
        let Some(member) = members.iter().find(|member| &member.user_id == user_id) else { return Vec::new() };
        if !member.actions(place.role_list(), place.role_place(), place.grants()).contains(&action) {
            return Vec::new();
        }
        let (user_id, name) = (member.user_id.clone(), member.name.clone());
        let held = matches!(action, MemberAction::GiveRole(..));
        let asking = match action {
            MemberAction::GiveRole(role, _) | MemberAction::TakeRole(role, _) => {
                if let Some(place) = self.places.iter_mut().find(|place| place.id() == id) {
                    place.hold(&user_id, &role, held);
                }
                return vec![Ask::At(id, ops::set_role(&user_id, &role, name, held))];
            }
            MemberAction::ResetPassword => Asking::Reset(Resetting::new(user_id, name)),
            MemberAction::MakeOwner => Asking::Transfer(Transferring { user_id, name, proof: Proof::default() }),
            MemberAction::Kick => Asking::Kick(Kicking::new(user_id, name)),
            MemberAction::Ban => Asking::Ban(Banning { user_id, name, ask: BanAsk::Ban, proof: Proof::default() }),
            MemberAction::Unban => Asking::Ban(Banning { user_id, name, ask: BanAsk::Unban, proof: Proof::default() }),
        };
        if let Some(place) = self.places.iter_mut().find(|place| place.id() == id) {
            place.close_member_menu();
        }
        self.desk.dialog = Some(Dialog { place: id, asking, call: None });
        Vec::new()
    }

    /// Create role on the roles page: a name asked for the new role, from whoever manages roles there.
    pub(super) fn create_role(&mut self, id: PlaceId) -> Vec<Ask> {
        let Some(place) = self.place(id).filter(|place| place.grants().holds(Grant::ManageRoles)) else {
            return Vec::new();
        };
        let Some(above) = place.roles().and_then(|roles| roles.default_role()).map(|role| role.name.clone()) else {
            return Vec::new();
        };
        self.desk.dialog = Some(Dialog { place: id, asking: Asking::CreateRole(Creating::new(above)), call: None });
        Vec::new()
    }

    /// Delete role, under the chosen role: asked first, for a role this account may change that is not the default.
    pub(super) fn delete_role(&mut self, id: PlaceId) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let asker = place.role_place();
        let chosen = place.roles().and_then(|roles| roles.chosen());
        let Some(role) = chosen.filter(|role| role.movable(asker) && place.grants().holds(Grant::ManageRoles)) else {
            return Vec::new();
        };
        let deleting = DeletingRole::new(role.id.clone(), role.name.clone());
        self.desk.dialog = Some(Dialog { place: id, asking: Asking::DeleteRole(deleting), call: None });
        Vec::new()
    }

    /// A button on a channel's row, or an item of its menu: rename it, or delete it after asking. A channel no longer
    /// listed does nothing.
    pub(super) fn channel(&mut self, id: PlaceId, action: ChannelAction, channel: &ChannelId) -> Vec<Ask> {
        let listed = match self.place(id).filter(|place| place.manages_channels()).and_then(Place::lobby) {
            Some(Lobby::Ready(listed)) => listed.find(channel),
            Some(Lobby::Loading | Lobby::Failed(_)) | None => None,
        };
        let (kind, name) = match listed {
            Some(place::ListEntry::Text(listed)) => (Kind::Text, listed.name.clone()),
            Some(place::ListEntry::Voice(listed)) => (Kind::Voice, listed.name.clone()),
            Some(place::ListEntry::Category(_)) | None => return Vec::new(),
        };
        let channel = channel.clone();
        let asking = match action {
            ChannelAction::Rename => Asking::Name(Naming::new(Named::Channel { id: channel, kind, name })),
            ChannelAction::Delete => Asking::DeleteChannel(Deleting::new(channel, kind, name)),
        };
        self.desk.dialog = Some(Dialog { place: id, asking, call: None });
        Vec::new()
    }

    /// Rename or delete on a category's menu: a category no longer listed does nothing.
    pub(super) fn category(&mut self, id: PlaceId, action: ChannelAction, category: &terms::CategoryId) -> Vec<Ask> {
        let Some(Lobby::Ready(listed)) = self.place(id).filter(|place| place.manages_channels()).and_then(Place::lobby)
        else {
            return Vec::new();
        };
        let Some(found) = listed.categories.iter().find(|one| one.id == *category) else { return Vec::new() };
        let (category, name) = (found.id.clone(), found.name.clone());
        let asking = match action {
            ChannelAction::Rename => Asking::Name(Naming::new(Named::Category { id: category, name })),
            ChannelAction::Delete => {
                let holds = listed
                    .placed
                    .iter()
                    .filter(|(channel, placed)| placed.category == category && listed.find(channel).is_some())
                    .count();
                let holds = u32::try_from(holds).unwrap_or(u32::MAX);
                Asking::DeleteCategory(DeletingCategory::new(category, name, holds))
            }
        };
        self.desk.dialog = Some(Dialog { place: id, asking, call: None });
        Vec::new()
    }

    /// A name asked for a new category, from whoever manages the channels.
    pub(super) fn create_category(&mut self, id: PlaceId) -> Vec<Ask> {
        if self.place(id).is_some_and(Place::manages_channels) {
            self.desk.dialog =
                Some(Dialog { place: id, asking: Asking::Name(Naming::new(Named::NewCategory)), call: None });
        }
        Vec::new()
    }

    /// Delete on a row of `id`'s invites: asked first.
    pub(super) fn delete_invite(&mut self, id: PlaceId, invite: InviteId) -> Vec<Ask> {
        if self.place(id).is_some() {
            let deleting = DeletingInvite::new(invite);
            self.desk.dialog = Some(Dialog { place: id, asking: Asking::DeleteInvite(deleting), call: None });
        }
        Vec::new()
    }

    /// A card's bin on the emoji page: asked first, by a holder of Manage emoji, for an emoji the server holds.
    pub(super) fn delete_emoji(&mut self, id: PlaceId, emoji: &chat::EmojiId) -> Vec<Ask> {
        let Some(place) = self.place(id).filter(|place| place.grants().holds(Grant::ManageEmoji)) else {
            return Vec::new();
        };
        let Some(held) = place.emoji().get(emoji) else { return Vec::new() };
        let deleting = DeletingEmoji { emoji: held.id.clone(), name: held.name.clone() };
        self.desk.dialog = Some(Dialog { place: id, asking: Asking::DeleteEmoji(deleting), call: None });
        Vec::new()
    }

    /// Delete on a message's menu or hover bar in the discussion `pane` names: asked first, for a message this account
    /// may delete; the menu goes away either way.
    pub(super) fn delete_message(
        &mut self,
        id: PlaceId,
        pane: place::Pane,
        message: &chat::MessageId,
        at: Moment,
    ) -> Vec<Ask> {
        let deleting = self.place(id).and_then(|place| {
            let open = place.in_pane(pane)?;
            let held = open.timeline()?.get(message).filter(|held| place.may_delete(pane, held))?;
            Some(DeletingMessage {
                pane,
                message: held.id.clone(),
                author: place.author_shown(open, &held.author),
                line: held.body.as_ref().and_then(first_line),
            })
        });
        if let Some(deleting) = deleting {
            self.desk.dialog = Some(Dialog { place: id, asking: Asking::DeleteMessage(deleting), call: None });
        }
        self.at(id, pane.carrying(place::DiscussionMessage::CloseMenu), at)
    }

    /// A new channel asked for, a text one to start with, in `category` or, with none given, the first category, from
    /// whoever manages the channels; nothing where no such category is listed. Who sees it is offered to whoever
    /// manages roles too.
    pub(super) fn create_channel(&mut self, id: PlaceId, category: Option<terms::CategoryId>) -> Vec<Ask> {
        let Some(place) = self.place(id).filter(|place| place.manages_channels()) else { return Vec::new() };
        let Some(Lobby::Ready(listed)) = place.lobby() else { return Vec::new() };
        let category = match category {
            Some(wanted) => listed.categories.iter().find(|one| one.id == wanted),
            None => listed.categories.first(),
        };
        let Some(category) = category.map(|one| one.id.clone()) else { return Vec::new() };
        let access = place.grants().holds(Grant::ManageRoles);
        let making = Making::new(Kind::Text, category, access);
        self.desk.dialog = Some(Dialog { place: id, asking: Asking::MakeChannel(making), call: None });
        Vec::new()
    }

    /// What is done in a confirmation: Cancel and a press outside close it unless its feature holds it, and the rest is
    /// its feature's, or the window's for a question of its own.
    pub(super) fn confirm(&mut self, said: DialogMessage, at: Moment) -> Vec<Ask> {
        let Some(dialog) = self.desk.dialog.as_mut() else { return Vec::new() };
        let id = dialog.place;
        let next = match (dialog.asking.feature(), said) {
            (Some(feature), DialogMessage::Cancel) if feature.holds_cancel() => Next::Stays,
            (_, DialogMessage::Cancel) => Next::Closes,
            (Some(feature), DialogMessage::Outside) if feature.holds_outside() => Next::Stays,
            (_, DialogMessage::Outside) => Next::Closes,
            (Some(feature), said) => feature.said(said),
            (None, DialogMessage::Confirm) => return self.confirmed(id, at),
            (
                None,
                DialogMessage::Password(_)
                | DialogMessage::Reveal
                | DialogMessage::Invite(_)
                | DialogMessage::CopyTemporary
                | DialogMessage::Named(_)
                | DialogMessage::SetUp(_)
                | DialogMessage::Making(_),
            ) => Next::Stays,
        };
        self.next(id, next, at)
    }

    /// A question of the window's own, confirmed.
    fn confirmed(&mut self, id: PlaceId, at: Moment) -> Vec<Ask> {
        let Some(dialog) = self.desk.dialog.as_ref() else { return Vec::new() };
        match dialog.asking {
            // "Choose a new owner": the members, where Make owner is.
            Asking::OwnerLeaves => {
                if self.place(id).is_some_and(|place| place.pages().contains(&Page::Members)) {
                    self.desk.dialog = None;
                    return self.at(id, place::Message::Page(place::PageMessage::Settings(Page::Members)), at);
                }
                Vec::new()
            }
            Asking::SignOut => {
                self.desk.dialog = None;
                let Some(place) = self.places.iter_mut().find(|place| place.id() == id) else { return Vec::new() };
                let mut todo: Vec<Ask> = place.sign_out().into_iter().map(|asked| Ask::At(id, asked)).collect();
                todo.extend(self.join_pending(at));
                todo
            }
            Asking::Quit => {
                self.desk.dialog = None;
                vec![Ask::Quit]
            }
            Asking::Remove => {
                self.desk.dialog = None;
                self.remove(id, at)
            }
            Asking::Leave(_)
            | Asking::Invite(_)
            | Asking::DeleteInvite(_)
            | Asking::Reset(_)
            | Asking::Transfer(_)
            | Asking::Kick(_)
            | Asking::Ban(_)
            | Asking::CreateRole(_)
            | Asking::DeleteRole(_)
            | Asking::MakeChannel(_)
            | Asking::Name(_)
            | Asking::DeleteChannel(_)
            | Asking::DeleteCategory(_)
            | Asking::SetUp(_)
            | Asking::DeleteEmoji(_)
            | Asking::DeleteMessage(_) => Vec::new(),
        }
    }

    /// Carries out what becomes of the confirmation over `id`.
    fn next(&mut self, id: PlaceId, next: Next, at: Moment) -> Vec<Ask> {
        match next {
            Next::Stays => Vec::new(),
            Next::Closes => {
                self.desk.dialog = None;
                Vec::new()
            }
            Next::Asks(asked) => vec![Ask::At(id, asked)],
            Next::Closing(asked) => {
                self.desk.dialog = None;
                asked.into_iter().map(|asked| Ask::At(id, asked)).collect()
            }
            Next::Telling(told) => {
                self.desk.dialog = None;
                self.at(id, told, at)
            }
            Next::Copies(text, what) => {
                self.desk.copying = Some(Copying { place: id, text, what });
                Vec::new()
            }
            Next::Opens(page) => {
                self.desk.dialog = None;
                self.at(id, place::Message::Page(place::PageMessage::Settings(page)), at)
            }
            Next::Becomes(asking) => {
                if let Some(dialog) = self.desk.dialog.as_mut() {
                    dialog.asking = asking;
                }
                Vec::new()
            }
            Next::Done(said) => match self.desk.dialog.take() {
                Some(closed) => self.done(closed.place, said),
                None => Vec::new(),
            },
        }
    }

    /// The server's answer to what the confirmation over `id` asked, taken by its feature when `answered` finds the
    /// confirmation open that asked it; none when no such one is.
    fn answered(
        &mut self,
        id: PlaceId,
        answered: impl FnOnce(&mut Asking) -> Option<Next>,
        at: Moment,
    ) -> Option<Vec<Ask>> {
        let dialog = self.desk.dialog.as_mut().filter(|dialog| dialog.place == id)?;
        let next = answered(&mut dialog.asking)?;
        Some(self.next(id, next, at))
    }

    /// The server's answer to leaving: gone from the window, or the dialog says why not.
    pub(super) fn left(&mut self, id: PlaceId, answer: Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        match answer {
            Ok(()) => {
                self.desk.dialog = self.desk.dialog.take().filter(|dialog| dialog.place != id);
                let mut todo = self.remove(id, at);
                // The server ended the account and its sessions itself.
                for done in &mut todo {
                    if let Ask::At(at, place::Ask::EndAccount { sign_out, .. }) = done
                        && *at == id
                    {
                        *sign_out = false;
                    }
                }
                todo
            }
            Err(refused) => {
                let answered = |asking: &mut Asking| match asking {
                    Asking::Leave(proof) => Some(proof.leave_refused(&refused)),
                    _ => None,
                };
                self.answered(id, answered, at).unwrap_or_default()
            }
        }
    }

    /// The server's answer to making an invite: the link in the dialog, or why not; the invites listed again when the
    /// settings show them.
    pub(super) fn invited(&mut self, id: PlaceId, answer: Result<terms::Invited, LinkError>, at: Moment) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let host = place.server().host().to_string();
        let listed = answer.is_ok() && place.settings().is_some_and(|open| open.page() == Page::Invites);
        let answered = |asking: &mut Asking| match asking {
            Asking::Invite(inviting) => Some(inviting.answered(answer, &host)),
            _ => None,
        };
        self.answered(id, answered, at);
        if listed { vec![Ask::At(id, ops::list_invites())] } else { Vec::new() }
    }

    /// The server's answer to deleting an invite.
    pub(super) fn invite_deleted(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::DeleteInvite(deleting) => Some(deleting.answered(answer)),
            _ => None,
        };
        self.answered(id, answered, at).unwrap_or_default()
    }

    /// The server's answer to making a channel: the dialog closes and the list shows it, the rest of who it lets in
    /// asked now; or the dialog says why not. A made channel's list is the answer.
    pub(super) fn channel_made(
        &mut self,
        id: PlaceId,
        answer: Result<proto::chatkord_v1::ChannelsSummary, LinkError>,
        at: Moment,
    ) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let made = answer.as_ref().ok().and_then(|summary| {
            let listed = place::listed(summary);
            let known = match place.lobby() {
                Some(Lobby::Ready(known)) => Some(known),
                Some(Lobby::Loading | Lobby::Failed(_)) | None => None,
            };
            listed.in_order().into_iter().find_map(|entry| match entry {
                place::ListEntry::Text(channel) if known.is_none_or(|known| known.find(&channel.id).is_none()) => {
                    Some(channel.id.clone())
                }
                place::ListEntry::Voice(channel) if known.is_none_or(|known| known.find(&channel.id).is_none()) => {
                    Some(channel.id.clone())
                }
                place::ListEntry::Text(_) | place::ListEntry::Voice(_) | place::ListEntry::Category(_) => None,
            })
        });
        let everyone = place.role_list().default_role().map(|role| role.id.clone());
        let (me, owner) = (place.me().cloned(), place.role_place().is_owner());
        let mut after = Vec::new();
        let answered = |asking: &mut Asking| match asking {
            Asking::MakeChannel(making) if making.asking() => {
                if let Some(made) = &made {
                    after = making.after_made(made, everyone.as_ref(), me.as_ref(), owner);
                }
                Some(making.answered(answer.as_ref().map(|_| ())))
            }
            _ => None,
        };
        let mut todo = self.answered(id, answered, at).unwrap_or_default();
        todo.extend(after.into_iter().map(|asked| Ask::At(id, asked)));
        if let Ok(listed) = answer {
            let listed = place::Message::Lobby(place::LobbyMessage::Channels(Ok(place::listed(&listed))));
            todo.extend(self.at(id, listed, at));
        }
        todo
    }

    /// The server's answer to a name asked in a dialog: renaming a channel, making or renaming a category. The dialog
    /// closes and the list shows it, or the dialog says why not; a rename asked from the channels page is that page's.
    /// A made category's list is the answer; a rename's comes with the server's event.
    pub(super) fn named(
        &mut self,
        id: PlaceId,
        answer: Result<Option<proto::chatkord_v1::ChannelsSummary>, LinkError>,
        at: Moment,
    ) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::Name(naming) if naming.asking() => Some(naming.answered(answer.as_ref().map(|_| ()))),
            _ => None,
        };
        let mut todo = match self.answered(id, answered, at) {
            Some(todo) => todo,
            None => {
                if let Some(place) = self.places.iter_mut().find(|place| place.id() == id) {
                    place.channel_named(&answer.as_ref().map(|_| ()).map_err(Clone::clone));
                }
                Vec::new()
            }
        };
        if let Ok(Some(listed)) = answer {
            let listed = place::Message::Lobby(place::LobbyMessage::Channels(Ok(place::listed(&listed))));
            todo.extend(self.at(id, listed, at));
        }
        todo
    }

    /// The server's answer to deleting a category.
    pub(super) fn category_deleted(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::DeleteCategory(deleting) if deleting.asking() => Some(deleting.answered(answer)),
            _ => None,
        };
        self.answered(id, answered, at).unwrap_or_default()
    }

    /// The server's answer to deleting a channel.
    pub(super) fn channel_deleted(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::DeleteChannel(deleting) if deleting.asking() => Some(deleting.answered(answer)),
            _ => None,
        };
        self.answered(id, answered, at).unwrap_or_default()
    }

    /// The members asked for again, when the settings still show them.
    fn list_members(&self, id: PlaceId) -> Vec<Ask> {
        match self.place(id) {
            Some(place) if place.showing(Page::Members) => vec![Ask::At(id, place::Ask::ListMembers)],
            Some(_) | None => Vec::new(),
        }
    }

    /// The server's answer to giving or taking a role: the list shows it when it went through, and the page says why
    /// when it did not; the list is asked for again.
    pub(super) fn role_set(&mut self, id: PlaceId, name: String, answer: &Result<(), LinkError>) -> Vec<Ask> {
        let Some(place) = self.place(id) else { return Vec::new() };
        let server = place.server().shown().to_string();
        let todo = self.list_members(id);
        let Some(refused) = members::roles_refused(name, server, answer) else { return todo };
        if let Some(place) = self.places.iter_mut().find(|place| place.id() == id) {
            place.raise_on_page(Notice::new(Subject::Roles, design::Status::Warning, refused));
        }
        todo
    }

    /// The server's answer to a reset; the list is asked for again.
    pub(super) fn reset(&mut self, id: PlaceId, answer: Result<Secret, LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::Reset(resetting) => Some(resetting.answered(answer)),
            _ => None,
        };
        let Some(mut todo) = self.answered(id, answered, at) else { return Vec::new() };
        todo.extend(self.list_members(id));
        todo
    }

    /// The server's answer to a kick: said in a toast once done, and the list asked for again.
    pub(super) fn kicked(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::Kick(kicking) => Some(kicking.answered(answer)),
            _ => None,
        };
        let Some(mut todo) = self.answered(id, answered, at) else { return Vec::new() };
        todo.extend(self.list_members(id));
        todo
    }

    /// The server's answer to a ban or an unban: once the dialog closes, the list is asked for again.
    pub(super) fn ban_set(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::Ban(banning) => Some(banning.answered(answer)),
            _ => None,
        };
        let Some(mut todo) = self.answered(id, answered, at) else { return Vec::new() };
        if self.desk.dialog.is_none() {
            todo.extend(self.list_members(id));
        }
        todo
    }

    /// The server's answer to making a role: the dialog closes and the page lists the roles it answered with, the new
    /// one chosen; or the dialog says why not.
    pub(super) fn role_created(
        &mut self,
        id: PlaceId,
        answer: Result<Vec<proto::chatkord_v1::RoleEntry>, LinkError>,
        at: Moment,
    ) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::CreateRole(creating) if creating.asking() => Some(creating.answered(answer.as_ref().map(|_| ()))),
            _ => None,
        };
        let mut todo = self.answered(id, answered, at).unwrap_or_default();
        if let Ok(listed) = answer
            && let Some(place) = self.places.iter_mut().find(|place| place.id() == id)
        {
            todo.extend(place.roles_created(listed).into_iter().map(|asked| Ask::At(id, asked)));
        }
        todo
    }

    /// The server's answer to deleting a role: the dialog closes and the role leaves the list, or the dialog says why
    /// not.
    pub(super) fn role_deleted(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let mut gone = None;
        let answered = |asking: &mut Asking| match asking {
            Asking::DeleteRole(deleting) if deleting.asking() => {
                let next = deleting.answered(answer);
                if matches!(next, Next::Closes) {
                    gone = Some(deleting.id.clone());
                }
                Some(next)
            }
            _ => None,
        };
        let todo = self.answered(id, answered, at).unwrap_or_default();
        if let Some(role) = gone
            && let Some(place) = self.places.iter_mut().find(|place| place.id() == id)
        {
            place.role_deleted(&role);
        }
        todo
    }

    /// The server's answer to a change of a role shown at once: when it did not go through, the page says why, a move
    /// is put back and the roles are read again.
    pub(super) fn role_changed(&mut self, id: PlaceId, name: String, answer: &Result<(), LinkError>) -> Vec<Ask> {
        let Some(place) = self.places.iter_mut().find(|place| place.id() == id) else { return Vec::new() };
        if answer.is_ok() {
            place.role_kept();
            return Vec::new();
        }
        let server = place.server().shown().to_string();
        if let Some(refused) = crate::roles::change_refused(name, server, answer) {
            place.raise_on_page(Notice::new(Subject::Roles, design::Status::Warning, refused));
        }
        place.role_refused();
        if place.showing(Page::Roles) { vec![Ask::At(id, ops::list_roles())] } else { Vec::new() }
    }

    /// The server's answer to handing ownership on: once the dialog closes, the list is asked for again.
    pub(super) fn transferred(&mut self, id: PlaceId, answer: &Result<(), LinkError>, at: Moment) -> Vec<Ask> {
        let answered = |asking: &mut Asking| match asking {
            Asking::Transfer(transferring) => Some(transferring.answered(answer)),
            _ => None,
        };
        let Some(mut todo) = self.answered(id, answered, at) else { return Vec::new() };
        if self.desk.dialog.is_none() {
            todo.extend(self.list_members(id));
        }
        todo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confirmation_quotes_the_first_line_cut_at_its_bound() {
        let line = |text: &str| first_line(&chat::Body::plain(text));
        assert_eq!(line("\n  hi all \nsecond").as_deref(), Some("hi all"));
        assert_eq!(line("  \n"), None);
        let long = "x".repeat(QUOTED_CHARS + 1);
        assert_eq!(line(&long), Some(format!("{}…", "x".repeat(QUOTED_CHARS - 1))));
        assert_eq!(line(&"x".repeat(QUOTED_CHARS)).map(|line| line.chars().count()), Some(QUOTED_CHARS));
    }

    #[test]
    fn a_confirmation_quotes_a_styled_message_without_its_markers() {
        for (written, shown) in
            [("**yes** now", "yes now"), ("`cargo run`", "cargo run"), ("||the twist||", "the twist")]
        {
            assert_eq!(first_line(&chat::Body::plain(written)).as_deref(), Some(shown), "{written}");
        }
    }
}
