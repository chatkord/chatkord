//! What a place asks of its server, op by op: each the op as the schema has it, built from the screen's own values, and
//! the message its answer becomes. The one place a screen's value becomes a wire value and a reply a screen's.

use super::effects::{Asked, done};
use super::*;
use proto::chatkord_v1 as wire;
use proto::chatkord_v1::history;

/// An op asked as `Ask`, its answer becoming `answer`'s message.
fn op<A, M>(op: A, answer: impl FnOnce(Result<A::Reply, LinkError>) -> M + Send + 'static) -> Ask
where
    A: proto::ops::Ask,
    M: Into<Message>,
{
    Ask::Op(Asked::new(op, answer))
}

/// The server's channels, as the list shows them.
pub(crate) fn list_channels() -> Ask {
    op(wire::ChannelsList {}, |listed| LobbyMessage::Channels(listed.map(|summary| crate::listed(&summary))))
}

/// The roles, in their order.
pub(crate) fn list_roles() -> Ask {
    op(wire::ListRoles {}, |listed| ServerMessage::RolesListed(listed.map(|roles| roles.entries)))
}

/// A channel's overwrites.
pub(crate) fn list_overwrites(channel: ChannelId) -> Ask {
    let ask = wire::ListChannelOverwrites { channel_id: channel.as_str().to_owned() };
    op(ask, move |listed| ServerMessage::Overwrites { channel, listed: listed.map(|read| read.entries) })
}

/// A page of the members every member sees, after `after`.
pub(crate) fn list_people(after: Option<crate::Cursor>) -> Ask {
    op(wire::ListPeople { after: after.map(crate::Cursor::into_wire) }, |page| {
        PeopleMessage::Page(page.map(|people| PeoplePage::from(&people)))
    })
}

/// A page of the server's emoji, after `after`.
pub(crate) fn list_emoji(after: Option<crate::Cursor>) -> Ask {
    op(wire::ListEmoji { after: after.map(crate::Cursor::into_wire) }, |page| {
        EmojiMessage::Page(page.map(|page| EmojiPage::from(&page)))
    })
}

/// How much the server and each channel alert this account.
pub(crate) fn get_notify() -> Ask {
    op(wire::GetNotify {}, |read| NotifyMessage::Read(read.map(|read| Notifying::read(&read))))
}

/// Sets the account's settings for the server, whole.
pub(crate) fn set_server_notify(settings: proto::notify::ServerSettings) -> Ask {
    op(wire::SetServerNotify { settings: Some(settings.wire()) }, |answer| NotifyMessage::Set(done(answer)))
}

/// Sets the account's settings for `channel`, whole.
pub(crate) fn set_channel_notify(channel: &ChannelId, settings: proto::notify::ChannelSettings) -> Ask {
    let settings = Some(settings.wire(channel.as_str().to_owned()));
    op(wire::SetChannelNotify { settings }, |answer| NotifyMessage::Set(done(answer)))
}

/// This account's conversations.
pub(crate) fn list_conversations() -> Ask {
    op(wire::ListConversations {}, TalkMessage::Conversations)
}

/// The threads this account follows.
pub(crate) fn list_followed() -> Ask {
    op(wire::ListFollowed {}, TalkMessage::Followed)
}

/// The conversation with `member`, made first when there is none.
pub(crate) fn open_conversation(member: &MemberId) -> Ask {
    op(wire::OpenConversation { user_id: member.as_str().to_owned() }, |made| TalkMessage::Made(made.map(Box::new)))
}

/// Follows a thread, or stops following it.
pub(crate) fn follow_thread(timeline: TimelineId, follow: bool) -> Ask {
    let ask = wire::FollowThread { timeline_id: timeline.as_str().to_owned(), follow };
    op(ask, move |answer| TalkMessage::FollowAnswered { timeline, follow, answer: done(answer) })
}

/// Chooses how the account is shown.
pub(crate) fn set_status(chosen: proto::status::Chosen) -> Ask {
    let ask = wire::SetStatus { choice: chosen.choice.to_wire() as i32, until: chosen.until };
    op(ask, |answer| TalkMessage::StatusSet(done(answer)))
}

/// A page of a timeline's history at `anchor`, as many as the server gives at once; `end` is the end it extends.
pub(crate) fn history(timeline: TimelineId, anchor: terms::Anchor, end: Option<chat::End>) -> Ask {
    let anchor = match anchor {
        terms::Anchor::Newest => None,
        terms::Anchor::Before(place) => Some(history::Anchor::Before(place)),
        terms::Anchor::After(place) => Some(history::Anchor::After(place)),
        terms::Anchor::Around(place) => Some(history::Anchor::Around(place)),
    };
    let ask = wire::History { timeline_id: timeline.as_str().to_owned(), anchor, limit: 0 };
    op(ask, move |page| {
        let page = page.map(|page| chat::Page::from(&page));
        Message::Discussion(DiscussionMessage::Page { timeline, end, page })
    })
}

/// The message at `place` in `timeline`, if kept, in a page of at most one: what a reply quotes.
pub(crate) fn quoted(timeline: TimelineId, place: u64) -> Ask {
    let anchor = Some(history::Anchor::Around(place));
    let ask = wire::History { timeline_id: timeline.as_str().to_owned(), anchor, limit: 1 };
    op(ask, move |page| {
        let page = page.map(|page| chat::Page::from(&page));
        Message::Discussion(DiscussionMessage::Quoted { timeline, place, page })
    })
}

/// Every message of `timeline` changed after the revision `since`, or a word to read it again.
pub(crate) fn catch_up(timeline: TimelineId, since: u64) -> Ask {
    let ask = wire::CatchUp { timeline_id: timeline.as_str().to_owned(), since };
    op(ask, move |changes| {
        let changes = changes.map(|changes| Caught::from(&changes));
        Message::Discussion(DiscussionMessage::CaughtUp { timeline, changes })
    })
}

/// Deletes a message, answered with its tombstone.
pub(crate) fn delete_message(message_id: String) -> Ask {
    op(wire::DeleteMessage { message_id }, |changed| {
        let changed = changed.and_then(|message| read_message(&message));
        Message::Discussion(DiscussionMessage::Changed(changed.map(Box::new)))
    })
}

/// A message the server answered with, read at the edge: one out of shape is a server not keeping the protocol.
pub fn read_message(message: &wire::Message) -> Result<chat::Message, LinkError> {
    chat::Message::try_from(message).map_err(|_| LinkError::Malformed)
}

/// Reacts to a message with an emoji, else takes the reaction back.
pub(crate) fn react(message_id: &chat::MessageId, emoji: &chat::ReactionEmoji, add: bool) -> Ask {
    let (message_id, emoji) = (message_id.as_str().to_owned(), Some(emoji.wire()));
    let answered = |answer| Message::Discussion(DiscussionMessage::Reacted(done(answer)));
    if add {
        op(wire::React { message_id, emoji }, answered)
    } else {
        op(wire::Unreact { message_id, emoji }, answered)
    }
}

/// Pins a message, else unpins it.
pub(crate) fn pin(message_id: &chat::MessageId, pinned: bool) -> Ask {
    let message_id = message_id.as_str().to_owned();
    let answered = |answer| Message::Discussion(DiscussionMessage::Pinned(done(answer)));
    if pinned { op(wire::Pin { message_id }, answered) } else { op(wire::Unpin { message_id }, answered) }
}

/// A page of a timeline's pinned messages, before the pin time `before`.
pub(crate) fn list_pins(timeline: TimelineId, before: Option<wire::PinsBefore>) -> Ask {
    let ask = wire::ListPins { timeline_id: timeline.as_str().to_owned(), before };
    op(ask, move |page| {
        let page = page.map(|page| chat::Page::from(&page));
        Message::Discussion(DiscussionMessage::Pins { timeline, page })
    })
}

/// A page of the messages this account reads that hold the words of `query`, after the match `before`.
pub(crate) fn search(query: proto::rules::SearchQuery, before: Option<chat::MessageId>) -> Ask {
    let ask = wire::Search {
        query: query.words().join(" "),
        timeline_id: None,
        before: before.as_ref().map(|message| wire::SearchBefore { message_id: message.as_str().to_owned() }),
    };
    op(ask, move |page| {
        let page = page.map(|page| chat::Results::from(&page));
        SearchMessage::Answered { query, before, page }
    })
}

/// Marks `timeline` read through `revision`; a mark only rises, and the server tells the standing it makes.
pub(crate) fn mark_read(timeline: &TimelineId, revision: u64) -> Ask {
    let ask = wire::MarkRead { timeline_id: timeline.as_str().to_owned(), revision };
    Ask::Op(Asked::unheard(ask, |answer| match answer {
        Err(LinkError::Offline) | Ok(_) => {}
        Err(why) => report::note!(warn, "a read mark was not made", why = why),
    }))
}

/// Adds an emoji named `name` with the picture uploaded as `file`; the list learns it from the event every member is told.
pub(crate) fn add_emoji(name: &proto::rules::EmojiName, file: FileId) -> Ask {
    let ask = wire::AddEmoji { name: name.as_str().to_owned(), file_id: file.to_string() };
    op(ask, |answer| PageMessage::Emoji(crate::emoji_settings::Message::Added(done(answer))))
}

/// Renames an emoji.
pub(crate) fn rename_emoji(emoji: &chat::EmojiId, name: &proto::rules::EmojiName) -> Ask {
    let ask = wire::RenameEmoji { emoji_id: emoji.as_str().to_owned(), name: name.as_str().to_owned() };
    op(ask, |answer| PageMessage::Emoji(crate::emoji_settings::Message::Renamed(done(answer))))
}

/// Deletes an emoji, and its reactions with it.
pub(crate) fn delete_emoji(emoji: &chat::EmojiId) -> Ask {
    let ask = wire::DeleteEmoji { emoji_id: emoji.as_str().to_owned() };
    op(ask, |answer| PageMessage::Emoji(crate::emoji_settings::Message::Deleted(done(answer))))
}

/// Deletes this account, proved by the password in force.
pub(crate) fn leave_server(password: &Secret) -> Ask {
    let ask = wire::LeaveServer { current: password.expose().to_string(), delete_messages: false };
    op(ask, |answer| SessionMessage::LeaveAnswered(done(answer)))
}

/// Makes an invite for `uses` people and `days` days, giving `role` beside the default one, with a note.
pub(crate) fn create_invite(uses: u32, days: u32, role: Option<&RoleId>, note: Option<String>) -> Ask {
    let ask = wire::CreateInvite { uses, days, role_id: role.map(|role| role.as_str().to_owned()), note };
    op(ask, |made| {
        PageMessage::Invited(made.map(|made| Invited { code: Secret::new(made.code), expires_at: made.expires_at }))
    })
}

/// The invites that still admit someone.
pub(crate) fn list_invites() -> Ask {
    op(wire::ListInvites {}, |listed| PageMessage::Invites(listed.map(|invites| invites.entries)))
}

/// Stops an invite.
pub(crate) fn delete_invite(invite: &InviteId) -> Ask {
    op(wire::RevokeInvite { invite_id: invite.as_str().to_owned() }, |answer| PageMessage::InviteDeleted(done(answer)))
}

/// Changes the password, proved by the one in force.
pub(crate) fn change_password(current: &Secret, new: &Secret) -> Ask {
    let ask = wire::ChangePassword { password: new.expose().to_string(), current: current.expose().to_string() };
    op(ask, |answer| PageMessage::PasswordChanged(done(answer)))
}

/// Sets the name others see, or with none goes back to the username.
pub(crate) fn set_display_name(name: Option<String>) -> Ask {
    op(wire::SetDisplayName { display_name: name }, |answer| PageMessage::NameSet(done(answer)))
}

/// Sets the colour others see this account in, or with none takes it off.
pub(crate) fn set_profile_colour(colour: Option<proto::rules::Colour>) -> Ask {
    let ask = wire::SetProfileColour { colour: colour.map(|colour| colour.to_string()) };
    op(ask, |answer| PageMessage::ProfileColourSet(done(answer)))
}

/// Sets the picture others see this account with, or with none takes it off.
pub(crate) fn set_profile_picture(picture: Option<FileId>) -> Ask {
    let ask = wire::SetProfilePicture { picture: picture.map(|picture| picture.to_string()) };
    op(ask, |answer| PageMessage::ProfilePictureSet(done(answer)))
}

/// Names the server, or with none leaves it nameless.
pub(crate) fn set_server_name(name: Option<String>) -> Ask {
    op(wire::SetServerName { name }, |answer| PageMessage::ServerNameSet(done(answer)))
}

/// Colours the server's picture, or with none goes back to Chatkord's.
pub(crate) fn set_server_colour(colour: Option<proto::rules::Colour>) -> Ask {
    let ask = wire::SetServerColour { colour: colour.map(|colour| colour.to_string()) };
    op(ask, |answer| PageMessage::ServerColourSet(done(answer)))
}

/// Sets the server's picture to an uploaded file, or with none takes it off.
pub(crate) fn set_server_picture(picture: Option<FileId>) -> Ask {
    let ask = wire::SetServerPicture { picture: picture.map(|picture| picture.to_string()) };
    op(ask, |answer| PageMessage::ServerPictureSet(done(answer)))
}

/// Turns `switch` on or off for the whole server.
pub(crate) fn set_server_switch(switch: proto::rules::Switch, on: bool) -> Ask {
    let ask = wire::SetServerSwitch { switch: switch.wire() as i32, on };
    op(ask, move |answer| PageMessage::SwitchSet { switch, on, answer: done(answer) })
}

/// The server's hosting facts and the switches an administrator has set off.
pub(crate) fn get_server_hosting() -> Ask {
    op(wire::GetServerHosting {}, PageMessage::ServerHostingListed)
}

/// Sets the server's hosting facts whole.
pub(crate) fn set_server_hosting(hosting: wire::Hosting) -> Ask {
    op(wire::SetServerHosting { hosting: Some(hosting) }, |answer| PageMessage::ServerHostingSet(done(answer)))
}

/// Every webhook of the server, without its secret.
pub(crate) fn list_webhooks() -> Ask {
    op(wire::ListWebhooks {}, |listed| PageMessage::WebhooksListed(listed.map(|listed| listed.webhooks)))
}

/// A webhook made, with the secret that is shown this once.
fn webhook_made(made: Result<wire::WebhookMade, LinkError>) -> PageMessage {
    let made = made.and_then(|made| {
        let token = Secret::new(made.token);
        made.webhook.map(|webhook| (webhook, token)).ok_or(LinkError::Malformed)
    });
    PageMessage::WebhookMade(made)
}

/// Makes a webhook in `channel`.
pub(crate) fn create_webhook(channel: &ChannelId, name: String, mention_everyone: bool) -> Ask {
    let ask = wire::CreateWebhook { channel_id: channel.as_str().to_owned(), name, mention_everyone };
    op(ask, webhook_made)
}

/// Gives a webhook a new secret, which stops the old one at once.
pub(crate) fn regenerate_webhook(webhook: &crate::webhooks::WebhookId) -> Ask {
    op(wire::RegenerateWebhook { webhook_id: webhook.as_str().to_owned() }, webhook_made)
}

/// Deletes a webhook: its URL stops working, and the posts it made stay.
pub(crate) fn delete_webhook(webhook: &crate::webhooks::WebhookId) -> Ask {
    op(wire::DeleteWebhook { webhook_id: webhook.as_str().to_owned() }, |answer| {
        PageMessage::WebhookDeleted(done(answer))
    })
}

/// Gives the member `user_id` the role `role`, or takes it; `name` is what the answer says of the member.
pub(crate) fn set_role(user_id: &MemberId, role: &RoleId, name: String, held: bool) -> Ask {
    let ask = wire::SetRole { user_id: user_id.as_str().to_owned(), role_id: role.as_str().to_owned(), held };
    op(ask, move |answer| PageMessage::RoleSet { name, answer: done(answer) })
}

/// Gives the member `user_id` a temporary password, answered once to hand over.
pub(crate) fn reset_password(user_id: &MemberId) -> Ask {
    op(wire::ResetPassword { user_id: user_id.as_str().to_owned() }, |reset| {
        PageMessage::PasswordReset(reset.map(|reset| Secret::new(reset.temporary_password)))
    })
}

/// Hands ownership of the server to `user_id`, proved by the password in force.
pub(crate) fn transfer_ownership(user_id: &MemberId, password: &Secret) -> Ask {
    let ask = wire::TransferOwnership { user_id: user_id.as_str().to_owned(), current: password.expose().to_string() };
    op(ask, |answer| PageMessage::Transferred(done(answer)))
}

/// Signs `user_id` out of every session they hold.
pub(crate) fn kick(user_id: &MemberId) -> Ask {
    op(wire::Kick { user_id: user_id.as_str().to_owned() }, |answer| PageMessage::Kicked(done(answer)))
}

/// Bans `user_id`, or unbans them, proved by the password in force.
pub(crate) fn set_banned(user_id: &MemberId, banned: bool, password: &Secret) -> Ask {
    let ask = wire::SetBanned { user_id: user_id.as_str().to_owned(), banned, current: password.expose().to_string() };
    op(ask, |answer| PageMessage::BanSet(done(answer)))
}

/// Mutes `user_id` in voice for everyone, or lifts that mute, or takes them out of the room they are in.
pub(crate) fn moderate(user_id: MemberId, act: Moderation) -> Ask {
    let target = user_id.as_str().to_owned();
    let told = move |answer| ServerMessage::Moderated { user_id, act, answer: done(answer) };
    match act {
        Moderation::Mute(muted) => op(wire::VoiceMuteMember { user_id: target, muted }, told),
        Moderation::TakeOut => op(wire::VoiceTakeOut { user_id: target }, told),
    }
}

/// Makes a role named `name`, holding no word, just above the default role.
pub(crate) fn create_role(name: String) -> Ask {
    let ask = wire::CreateRole { name, colour: None };
    op(ask, |made| PageMessage::RoleCreated(made.map(|roles| roles.entries)))
}

/// Renames a role.
pub(crate) fn set_role_name(role: RoleId, name: String) -> Ask {
    let ask = wire::SetRoleName { role_id: role.as_str().to_owned(), name };
    op(ask, move |answer| PageMessage::RoleNamed { role, answer: done(answer) })
}

/// Colours a role, or with none takes its colour off; `name` is what the answer says of the role.
pub(crate) fn set_role_colour(role: &RoleId, name: String, colour: Option<proto::rules::Colour>) -> Ask {
    let ask =
        wire::SetRoleColour { role_id: role.as_str().to_owned(), colour: colour.map(|colour| colour.to_string()) };
    op(ask, move |answer| PageMessage::RoleChanged { name, answer: done(answer) })
}

/// Gives a role the grant word `grant`, or takes it.
pub(crate) fn set_role_word(role: &RoleId, name: String, grant: Grant, held: bool) -> Ask {
    let ask = wire::SetRoleWord { role_id: role.as_str().to_owned(), word: grant.word().to_string(), held };
    op(ask, move |answer| PageMessage::RoleChanged { name, answer: done(answer) })
}

/// Moves a role just above the role `before`, or with none just above the default role.
pub(crate) fn move_role(role: &RoleId, name: String, before: Option<&RoleId>) -> Ask {
    let ask = wire::MoveRole { role_id: role.as_str().to_owned(), before: before.map(|role| role.as_str().to_owned()) };
    op(ask, move |answer| PageMessage::RoleChanged { name, answer: done(answer) })
}

/// Deletes a role; its holders lose it.
pub(crate) fn delete_role(role: &RoleId) -> Ask {
    op(wire::DeleteRole { role_id: role.as_str().to_owned() }, |answer| PageMessage::RoleDeleted(done(answer)))
}

/// Makes a channel of `kind`, last in `category`, seen or written in as `access` says from its start and allowed to
/// `role` too.
pub(crate) fn create_channel(
    kind: ChannelKind,
    name: String,
    category: &CategoryId,
    access: Option<Access>,
    role: Option<&RoleId>,
) -> Ask {
    let ask = wire::ChannelCreate {
        kind: kind.into(),
        name,
        category_id: category.as_str().to_owned(),
        access: access.map(|access| access.wire().into()),
        role_id: role.map(|role| role.as_str().to_owned()),
    };
    op(ask, PageMessage::ChannelCreated)
}

/// Sets what `whom` may do with `word` in `channel`: allowed, denied, or with none left to the roles' own words.
pub(crate) fn set_overwrite(
    channel: ChannelId,
    whom: &Whom,
    word: Grant,
    state: Option<proto::rules::Overwrite>,
) -> Ask {
    let whom = match whom {
        Whom::Role(role) => wire::set_channel_overwrite::Whom::RoleId(role.as_str().to_owned()),
        Whom::Member(member) => wire::set_channel_overwrite::Whom::UserId(member.as_str().to_owned()),
    };
    let state = state.map_or(wire::OverwriteState::Inherit, proto::rules::Overwrite::wire);
    let ask = wire::SetChannelOverwrite {
        channel_id: channel.as_str().to_owned(),
        whom: Some(whom),
        word: word.word().to_string(),
        state: state.into(),
    };
    op(ask, move |answer| PageMessage::OverwriteSet { channel, answer: done(answer) })
}

/// Makes a category named `name`, last in the order.
pub(crate) fn create_category(name: String) -> Ask {
    op(wire::CategoryCreate { name }, PageMessage::CategoryCreated)
}

/// Renames a category.
pub(crate) fn rename_category(category: &CategoryId, name: String) -> Ask {
    let ask = wire::CategoryRename { category_id: category.as_str().to_owned(), name };
    op(ask, |answer| PageMessage::CategoryRenamed(done(answer)))
}

/// Deletes a category, with its channels and their messages.
pub(crate) fn delete_category(category: &CategoryId) -> Ask {
    op(wire::CategoryDelete { category_id: category.as_str().to_owned() }, |answer| {
        PageMessage::CategoryDeleted(done(answer))
    })
}

/// Moves a channel into `category`, just before the channel `before` there, or last; `version` is the list's as read.
pub(crate) fn move_channel(id: &ChannelId, category: &CategoryId, before: Option<&ChannelId>, version: u64) -> Ask {
    let ask = wire::ChannelMove {
        channel_id: id.as_str().to_owned(),
        category_id: category.as_str().to_owned(),
        before: before.map(|channel| channel.as_str().to_owned()),
        version,
    };
    op(ask, |answer| LobbyMessage::Moved(done(answer)))
}

/// Moves a category just before the category `before`, or last; `version` is the list's as read.
pub(crate) fn move_category(id: &CategoryId, before: Option<&CategoryId>, version: u64) -> Ask {
    let ask = wire::CategoryMove {
        category_id: id.as_str().to_owned(),
        before: before.map(|category| category.as_str().to_owned()),
        version,
    };
    op(ask, |answer| LobbyMessage::Moved(done(answer)))
}

/// Renames a channel.
pub(crate) fn rename_channel(channel: &ChannelId, name: String) -> Ask {
    let ask = wire::ChannelRename { channel_id: channel.as_str().to_owned(), name };
    op(ask, |answer| PageMessage::ChannelRenamed(done(answer)))
}

/// Deletes a channel; everyone in its voice room is taken out.
pub(crate) fn delete_channel(channel: &ChannelId) -> Ask {
    op(wire::ChannelDelete { channel_id: channel.as_str().to_owned() }, |answer| {
        PageMessage::ChannelDeleted(done(answer))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{role_id, wire as proto_wire};
    use proto::chatkord_v1::wire_message::Body;

    fn answered(ask: Ask, reply: Result<Body, LinkError>) -> Option<Message> {
        match ask {
            Ask::Op(asked) => asked.answered(reply),
            other => panic!("not one op: {other:?}"),
        }
    }

    #[test]
    fn an_answer_reaches_the_message_its_op_feeds_with_what_the_ask_carried() {
        let ask = set_role_name(role_id("r1"), "Mods".into());
        assert!(matches!(ask.op(), Some(Body::SetRoleName(set)) if set.role_id == "r1" && set.name == "Mods"));
        let told = answered(ask, Ok(Body::Ok(proto_wire::Ok {})));
        assert!(
            matches!(told, Some(Message::Page(PageMessage::RoleNamed { role, answer: Ok(()) })) if role.as_str() == "r1")
        );
    }

    #[test]
    fn a_refusal_is_the_answer_and_a_reply_of_another_op_is_malformed() {
        let refused = LinkError::Refused(proto_wire::ErrorCode::NotAuthorized);
        let told = answered(kick(&MemberId::parse("bo").unwrap()), Err(refused.clone()));
        assert!(matches!(told, Some(Message::Page(PageMessage::Kicked(Err(why)))) if why == refused));
        let told = answered(kick(&MemberId::parse("bo").unwrap()), Ok(Body::Roles(proto_wire::Roles::default())));
        assert!(matches!(told, Some(Message::Page(PageMessage::Kicked(Err(LinkError::Malformed))))));
    }

    #[test]
    fn a_webhook_made_with_no_webhook_in_it_is_malformed_and_one_with_it_keeps_its_secret() {
        let ask = || create_webhook(&ChannelId::parse("c1").unwrap(), "Build".into(), false);
        let made = |webhook| Ok(Body::WebhookMade(proto_wire::WebhookMade { webhook, token: "secret".into() }));
        let told = answered(ask(), made(None));
        assert!(matches!(told, Some(Message::Page(PageMessage::WebhookMade(Err(LinkError::Malformed))))));
        let told = answered(ask(), made(Some(proto_wire::Webhook::default())));
        assert!(
            matches!(told, Some(Message::Page(PageMessage::WebhookMade(Ok((_, token))))) if token.expose() == "secret")
        );
    }

    #[test]
    fn a_read_mark_tells_the_window_nothing_whatever_the_answer() {
        let ask = || mark_read(&TimelineId::parse("t1").unwrap(), 5);
        assert!(answered(ask(), Ok(Body::Ok(proto_wire::Ok {}))).is_none());
        assert!(answered(ask(), Err(LinkError::Timeout)).is_none());
    }
}
