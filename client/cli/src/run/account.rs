//! A person's own account: registering, signing in and out, ending sessions, the password, and checking an invite.

use super::{Ask, Servers, Terminal, chosen_sentence, greet, remember, sign_in as signed_in};
use platform::keyring;
use terms::Address;

pub(super) async fn register(
    address: Address,
    identity: String,
    display_name: Option<String>,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let invite = terminal.secret(Ask::Invite).await?;
    let password = terminal.secret(Ask::Password).await?;
    if terminal.secret(Ask::PasswordAgain).await? != password {
        return Err("The two passwords are not the same.".to_string());
    }
    // The token the server issues is saved under the address that greeted.
    let greeted = greet(&address, servers, terminal).await?;
    let ack = greeted
        .link
        .register(&identity, display_name.as_deref(), &password, &invite)
        .await
        .map_err(|e| chosen_sentence(&e))?;
    servers.parting.keep(&greeted.link);
    remember(servers.platform, &greeted.url, &identity, &ack, terminal).await;
    terminal.say(&format!("Registered and signed in as {}.", ack.display_name));
    Ok(())
}

pub(super) async fn sign_in(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (_link, _events, ack, _, _) = signed_in(&address, &identity, terminal, servers).await?;
    terminal.say(&format!("Signed in as {}.", ack.display_name));
    Ok(())
}

/// The server ends this token's session and the socket that asked; what is left is the token saved here. A server
/// whose greeting did not name the word is not asked, and forgetting here is all that can be done.
pub(super) async fn sign_out(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _, _, url) = signed_in(&address, &identity, terminal, servers).await?;
    link.ask(proto::chatkord_v1::SignOut {}).await.map_err(|e| crate::said(&e))?;
    match keyring::forget_token(&url, &identity).await {
        Ok(()) => terminal.say("Signed out: this session is over and the saved sign-in is gone from this device."),
        Err(_) => terminal.say("Signed out: this session is over, but the system keyring could not be used, so a token saved here is left behind."),
    }
    Ok(())
}

/// The session says which account and the password in force that it is still this person's, so that password is
/// asked for wherever this run did not just type it.
pub(super) async fn change_password(
    address: Address,
    identity: String,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, _events, _ack, current, _) = signed_in(&address, &identity, terminal, servers).await?;
    let asked_with = match current {
        Some(typed) => typed,
        None => terminal.secret(Ask::Password).await?,
    };
    let replacement = terminal.secret(Ask::NewPassword).await?;
    if terminal.secret(Ask::NewPasswordAgain).await? != replacement {
        return Err("The two passwords are not the same.".to_string());
    }
    // A change that changes nothing is a slip, answered here: a server would spend a hash to refuse it.
    if replacement == asked_with {
        return Err("That is the password the account already has.".to_string());
    }
    let change = proto::chatkord_v1::ChangePassword {
        password: replacement.expose().to_string(),
        current: asked_with.expose().to_string(),
    };
    link.ask(change).await.map_err(|e| chosen_sentence(&e))?;
    terminal.say("Changed the password and ended every other session this account has; they sign in again with the new password.");
    Ok(())
}

pub(super) async fn invite_check(
    address: Address,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let code = terminal.secret(Ask::Invite).await?;
    let answered = greet(&address, servers, terminal).await?;
    let standing = answered.link.ask(proto::chatkord_v1::InviteInfo { code: code.expose().to_string() });
    match standing.await.map_err(|e| crate::said(&e))?.usable {
        Some(usable) if usable.owner => {
            terminal.say(&format!("That invite works until {} and makes you the server owner.", usable.expires_at));
        }
        Some(usable) => terminal.say(&format!("That invite works until {}.", usable.expires_at)),
        None => terminal.say("That invite doesn't work: it was used up, ran out of time or never was."),
    }
    Ok(())
}
