//! The order of a server's channel list: its categories made, renamed, moved and deleted, and a channel moved. A
//! move names the list's version it was read at, read just before it; another is refused and moves nothing.

use super::{Servers, Terminal, sign_in};
use link::Link;
use proto::chatkord_v1 as wire;
use proto::chatkord_v1::ChannelsList;
use terms::Address;

/// The list's version as the server holds it now, which a move names.
async fn version_now(link: &Link) -> Result<u64, String> {
    link.ask(ChannelsList {}).await.map(|summary| summary.version).map_err(|e| crate::said(&e))
}

pub(super) async fn category_create(
    address: Address,
    identity: String,
    name: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::CategoryCreate { name }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Category created.");
    Ok(())
}

pub(super) async fn category_rename(
    address: Address,
    identity: String,
    category_id: String,
    name: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::CategoryRename { category_id, name }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Category renamed.");
    Ok(())
}

pub(super) async fn category_delete(
    address: Address,
    identity: String,
    category_id: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    link.ask(wire::CategoryDelete { category_id }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Category deleted, with its channels.");
    Ok(())
}

pub(super) async fn category_move(
    address: Address,
    identity: String,
    category_id: String,
    before: Option<String>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let version = version_now(&link).await?;
    link.ask(wire::CategoryMove { category_id, before, version }).await.map_err(|e| crate::said(&e))?;
    terminal.say("Category moved.");
    Ok(())
}

pub(super) async fn channel_move(
    address: Address,
    identity: String,
    channel_id: String,
    category: String,
    before: Option<String>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let version = version_now(&link).await?;
    let moved = wire::ChannelMove { channel_id, category_id: category, before, version };
    link.ask(moved).await.map_err(|e| crate::said(&e))?;
    terminal.say("Channel moved.");
    Ok(())
}
