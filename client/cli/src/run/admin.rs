//! What a signed-in member asks of the server: its channels, invites, members, roles and picture, its ownership, and
//! leaving it.

use super::{Ask, Servers, Terminal, sign_in};
use crate::command::{Access, InviteTerms};
use link::Link;
use platform::keyring;
use proto::chatkord_v1 as wire;
use proto::chatkord_v1::{ChannelKind, ChannelsList, ErrorCode};
use terms::{Address, LinkError};

pub(super) async fn channels(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    use proto::chatkord_v1::{Event, event};
    use proto::rules::Kind;
    use screens::ListEntry;
    let (link, mut events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let summary = link.ask(ChannelsList {}).await.map_err(|e| crate::said(&e))?;
    // Who is in each voice room was told as the session signed in, before this answer.
    let mut rooms = std::collections::HashMap::new();
    while let Some(heard) = events.told() {
        if let terms::Heard::Event(Event { kind: Some(event::Kind::VoiceOccupancy(room)), .. }) = heard {
            rooms.insert(room.channel_id, room.people.len());
        }
    }
    // A category's line comes before its channels'.
    for entry in screens::listed(&summary).in_order() {
        terminal.say(&match entry {
            ListEntry::Category(category) => format!("{}\tcategory\t{}", category.id.as_str(), category.name),
            ListEntry::Text(channel) => format!("{}\t{}\t{}", channel.id.as_str(), Kind::Text.word(), channel.name),
            ListEntry::Voice(channel) => {
                let id = channel.id.as_str();
                let people = rooms.get(id).copied().unwrap_or(0);
                format!("{id}\t{}\t{}\t{people} in voice", Kind::Voice.word(), channel.name)
            }
        });
    }
    Ok(())
}

/// The id of the one role called `name`, in any case: a role is named by its id, and two may share a name.
pub(super) async fn role_called(link: &Link, name: &str) -> Result<String, String> {
    let roles = link.ask(wire::ListRoles {}).await.map_err(|e| crate::said(&e))?.entries;
    let name = name.to_lowercase();
    let mut called = roles.into_iter().filter(|role| role.name.to_lowercase() == name);
    match (called.next(), called.next()) {
        (Some(role), None) => Ok(role.role_id),
        (None, _) => Err(format!("No role is called {name}.")),
        (Some(_), Some(_)) => Err(format!("More than one role is called {name}.")),
    }
}

/// Each role's name by its id; every member may list them.
pub(super) async fn role_names(link: &Link) -> Result<std::collections::HashMap<String, String>, String> {
    let roles = link.ask(wire::ListRoles {}).await.map_err(|e| crate::said(&e))?.entries;
    Ok(roles.into_iter().map(|role| (role.role_id, role.name)).collect())
}

pub(super) async fn invite_create(
    address: Address,
    identity: String,
    terms: InviteTerms,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = match terms.role.as_deref() {
        Some(name) => Some(role_called(&link, name).await?),
        None => None,
    };
    let create = wire::CreateInvite { uses: terms.uses, days: terms.days, role_id: role, note: terms.note };
    let made = link.ask(create).await.map_err(|e| crate::said(&e))?;
    terminal.say(&made.code);
    Ok(())
}

pub(super) async fn invite_list(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let names = role_names(&link).await?;
    for invite in link.ask(wire::ListInvites {}).await.map_err(|e| crate::said(&e))?.entries {
        let role = invite.role_id.map(|id| names.get(&id).cloned().unwrap_or(id)).unwrap_or_default();
        let note = invite.note.unwrap_or_default();
        terminal.say(&format!(
            "{}\t{} of {} used\tends {}\t{role}\t{note}",
            invite.invite_id, invite.uses, invite.max_uses, invite.expires_at
        ));
    }
    Ok(())
}

pub(super) async fn invite_revoke(
    address: Address,
    identity: String,
    invite_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::RevokeInvite { invite_id }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Invite deleted.");
    Ok(())
}

pub(super) async fn member_reset(
    address: Address,
    identity: String,
    user_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let reset = link.ask(wire::ResetPassword { user_id }).await.map_err(|e| crate::said(&e))?;
    let temporary = terms::Secret::new(reset.temporary_password);
    terminal.say("Their sessions ended. Give them this temporary password; they choose their own when they sign in:");
    terminal.say(temporary.expose());
    Ok(())
}

pub(super) async fn member_list(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let names = role_names(&link).await?;
    for member in link.list_members().await.map_err(|e| crate::said(&e))? {
        let owner = if proto::rules::Place::of_wire(member.place).is_owner() { "\tserver owner" } else { "" };
        let banned = if member.banned { "\tbanned" } else { "" };
        let roles: Vec<&str> =
            member.role_ids.iter().map(|id| names.get(id).map_or(id.as_str(), String::as_str)).collect();
        terminal.say(&format!(
            "{}\t{}\t{}\t{}{owner}{banned}",
            member.user_id,
            member.handle,
            member.display_name,
            roles.join(", ")
        ));
    }
    Ok(())
}

pub(super) async fn member_role(
    address: Address,
    identity: String,
    user_id: String,
    role: String,
    held: bool,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    link.ask(wire::SetRole { user_id, role_id: role, held }).await.map_err(|e| crate::said(&e))?;
    terminal.say(if held { "Role given." } else { "Role taken." });
    Ok(())
}

pub(super) async fn role_list(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    // In the order, highest first: a role acts only on the roles and members below it.
    for role in link.ask(wire::ListRoles {}).await.map_err(|e| crate::said(&e))?.entries {
        let what = match (role.is_default, role.is_owner) {
            (true, _) => "\teveryone holds it",
            (_, true) => "\tthe owner role",
            (false, false) => "",
        };
        let words = if role.grants.is_empty() { "no words".to_string() } else { role.grants.join(", ") };
        terminal.say(&format!("{}\t{}\t{}\t{words}{what}", role.role_id, role.position, role.name));
    }
    Ok(())
}

pub(super) async fn member_kick(
    address: Address,
    identity: String,
    user_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::Kick { user_id }).await.map_err(|e| crate::said(&e))?;
    terminal.say("They are signed out everywhere; they can sign in again.");
    Ok(())
}

pub(super) async fn member_ban(
    address: Address,
    identity: String,
    user_id: String,
    banned: bool,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, typed, _) = sign_in(&address, &identity, terminal, servers).await?;
    let current = match typed {
        Some(typed) => typed,
        None => terminal.secret(Ask::Password).await?,
    };
    let ban = wire::SetBanned { user_id, banned, current: current.expose().to_string() };
    link.ask(ban).await.map_err(unproved)?;
    terminal.say(if banned {
        "They are banned: signed out everywhere, and they cannot sign in until unbanned."
    } else {
        "They are unbanned and can sign in again."
    });
    Ok(())
}

pub(super) async fn owner_transfer(
    address: Address,
    identity: String,
    user_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, typed, _) = sign_in(&address, &identity, terminal, servers).await?;
    let current = match typed {
        Some(typed) => typed,
        None => terminal.secret(Ask::Password).await?,
    };
    let transfer = wire::TransferOwnership { user_id, current: current.expose().to_string() };
    link.ask(transfer).await.map_err(unproved)?;
    terminal.say("They own this server now; you keep the role just below theirs, if there is one.");
    Ok(())
}

/// What an ask proved by the account's password says when it is refused: a wrong password in the command's words.
fn unproved(refused: LinkError) -> String {
    match refused {
        LinkError::Refused(ErrorCode::Unauthenticated) => {
            "That is not this account's password; nothing changed.".to_string()
        }
        other => crate::said(&other),
    }
}

pub(super) async fn leave(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, typed, url) = sign_in(&address, &identity, terminal, servers).await?;
    let current = match typed {
        Some(typed) => typed,
        None => terminal.secret(Ask::Password).await?,
    };
    let leaving = wire::LeaveServer { current: current.expose().to_string(), delete_messages: false };
    link.ask(leaving).await.map_err(|refused| match refused {
        LinkError::Refused(ErrorCode::Unauthenticated) => {
            "That is not this account's password; nothing was deleted.".to_string()
        }
        other => crate::said(&other),
    })?;
    let _ = keyring::forget_token(&url, &identity).await;
    terminal.say("Your account on this server is deleted.");
    Ok(())
}

/// A channel to make, as typed.
pub(super) struct NewChannel {
    /// A voice channel, else a text one.
    pub(super) voice: bool,
    /// Its name.
    pub(super) name: String,
    /// The category it goes in, by id.
    pub(super) category: String,
    /// Who sees or writes in it from its start, the role by its name.
    pub(super) access: Option<Access>,
}

pub(super) async fn channel_create(
    address: Address,
    identity: String,
    made: NewChannel,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let kind = if made.voice { ChannelKind::Voice } else { ChannelKind::Text };
    let role = match made.access.as_ref().and_then(|access| access.role.as_deref()) {
        Some(name) => Some(role_called(&link, name).await?),
        None => None,
    };
    let kept = made.access.map(|access| access.kept);
    let create = wire::ChannelCreate {
        kind: kind.into(),
        name: made.name,
        category_id: made.category,
        access: kept.map(Into::into),
        role_id: role,
    };
    link.ask(create).await.map_err(|e| crate::said(&e))?;
    terminal.say("Channel created.");
    Ok(())
}

pub(super) async fn channel_delete(
    address: Address,
    identity: String,
    channel_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::ChannelDelete { channel_id }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Channel deleted.");
    Ok(())
}

pub(super) async fn server_picture(
    address: Address,
    identity: String,
    file: Option<String>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let picture = match file {
        None => None,
        Some(file) => Some(upload_picture(&link, file).await?),
    };
    let set = wire::SetServerPicture { picture: picture.map(|picture| picture.to_string()) };
    link.ask(set).await.map_err(|e| crate::said(&e))?;
    terminal.say(if picture.is_some() { "The server's picture is set." } else { "The server has no picture now." });
    Ok(())
}

/// A picture file checked the way the server checks it, then sent; its id.
async fn upload_picture(link: &Link, file: String) -> Result<proto::files::FileId, String> {
    let kind = proto::files::FileKind::ServerPicture;
    let bytes = platform::pick::file(file.into()).await.map_err(|_| "That file could not be read.".to_string())?;
    let checked = tokio::task::spawn_blocking(move || portrait::before_upload(&bytes, kind).map(|()| bytes)).await;
    let bytes = checked.map_err(|_| "That file could not be read.".to_string())?.map_err(|e| crate::file_said(&e))?;
    link.upload(kind, bytes).await.map_err(|e| crate::file_said(&e))
}
