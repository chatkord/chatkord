//! A server's roles as a member holding `MANAGE_ROLES` changes them, and what a role or a member may do in one channel.
//! A role is named by its name at the terminal and by its id on the wire; a member by their id at both.

use super::admin::{role_called, role_names};
use super::{Servers, Terminal, sign_in};
use crate::command::Whom;
use proto::chatkord_v1 as wire;
use proto::chatkord_v1::{channel_overwrite, set_channel_overwrite};
use proto::rules::{Colour, Grant, Overwrite};
use terms::Address;

pub(super) async fn create(
    address: Address,
    identity: String,
    name: String,
    colour: Option<Colour>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let create = wire::CreateRole { name, colour: colour.map(|colour| colour.to_string()) };
    link.ask(create).await.map_err(|e| crate::said(&e))?;
    terminal.say("Role created, just above the default role, holding no word.");
    Ok(())
}

pub(super) async fn rename(
    address: Address,
    identity: String,
    role: String,
    name: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    link.ask(wire::SetRoleName { role_id: role, name }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Role renamed.");
    Ok(())
}

pub(super) async fn colour(
    address: Address,
    identity: String,
    role: String,
    colour: Option<Colour>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    let set = wire::SetRoleColour { role_id: role, colour: colour.map(|colour| colour.to_string()) };
    link.ask(set).await.map_err(|e| crate::said(&e))?;
    terminal.say(if colour.is_some() { "The role's colour is set." } else { "The role has no colour now." });
    Ok(())
}

pub(super) async fn move_to(
    address: Address,
    identity: String,
    role: String,
    before: Option<String>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    let before = match before {
        Some(name) => Some(role_called(&link, &name).await?),
        None => None,
    };
    link.ask(wire::MoveRole { role_id: role, before }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Role moved.");
    Ok(())
}

pub(super) async fn delete(
    address: Address,
    identity: String,
    role: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    link.ask(wire::DeleteRole { role_id: role }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Role deleted; its holders no longer hold it.");
    Ok(())
}

pub(super) async fn word(
    address: Address,
    identity: String,
    role: String,
    word: Grant,
    held: bool,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let role = role_called(&link, &role).await?;
    let set = wire::SetRoleWord { role_id: role, word: word.word().to_string(), held };
    link.ask(set).await.map_err(|e| crate::said(&e))?;
    terminal.say(if held { "Word given." } else { "Word taken." });
    Ok(())
}

pub(super) async fn overwrites(
    address: Address,
    identity: String,
    channel_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let entries = link.ask(wire::ListChannelOverwrites { channel_id }).await.map_err(|e| crate::said(&e))?.entries;
    let names = role_names(&link).await?;
    // An entry the server should never list (no state or whom, or a state this build does not know) is passed over.
    let said: Vec<String> = entries
        .into_iter()
        .filter_map(|entry| {
            let overwrite = Overwrite::asked(entry.state).ok().flatten()?;
            let whom = match entry.whom? {
                channel_overwrite::Whom::RoleId(id) => format!("role\t{}", names.get(&id).cloned().unwrap_or(id)),
                channel_overwrite::Whom::UserId(id) => format!("member\t{id}"),
            };
            Some(format!("{whom}\t{}\t{}", entry.word, overwrite.word()))
        })
        .collect();
    if said.is_empty() {
        terminal.say("Every role's own words decide here.");
    }
    for line in said {
        terminal.say(&line);
    }
    Ok(())
}

/// What one role or member may do with one word in one channel, as typed.
pub(super) struct Overwritten {
    /// The channel's id.
    pub(super) channel_id: String,
    /// The role, by its name, or the member, by their id.
    pub(super) whom: Whom,
    /// The word, one a channel takes.
    pub(super) word: Grant,
    /// Allowed or denied there; none leaves it to the role's own words.
    pub(super) overwrite: Option<Overwrite>,
}

pub(super) async fn overwrite(
    address: Address,
    identity: String,
    set: Overwritten,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let (whom, inherited) = match set.whom {
        Whom::Role(name) => (
            set_channel_overwrite::Whom::RoleId(role_called(&link, &name).await?),
            "The role's own words decide in this channel now.",
        ),
        Whom::Member(id) => (set_channel_overwrite::Whom::UserId(id), "The member's roles decide in this channel now."),
    };
    let state = set.overwrite.map_or(wire::OverwriteState::Inherit, Overwrite::wire);
    let overwritten = wire::SetChannelOverwrite {
        channel_id: set.channel_id,
        whom: Some(whom),
        word: set.word.word().to_string(),
        state: state.into(),
    };
    link.ask(overwritten).await.map_err(|e| crate::said(&e))?;
    terminal.say(match set.overwrite {
        Some(Overwrite::Allow) => "Allowed in this channel.",
        Some(Overwrite::Deny) => "Denied in this channel.",
        None => inherited,
    });
    Ok(())
}
