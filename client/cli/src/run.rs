//! Carries out a command against a server: the dispatch, and what every command shares (the terminal, greeting a
//! server, signing in, the engine).

mod account;
mod admin;
mod call;
mod device;
mod order;
mod roles;

use crate::command::{Account, Act, Command};
use errands::signin;
use link::{Events, Link, Resolved, Trust};
use platform::Platform;
use proto::chatkord_v1::{ErrorCode, LoginAck};
use screens::route_warning;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use terms::{Address, LinkError, MemberId, Secret, ServerUrl};
use webrtc::Engine;
use words::{Phrase, Sentence};

/// What the person is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The account's password, or the one it is registered with.
    Password,
    /// That password again, refused unless the two match.
    PasswordAgain,
    /// The password that replaces it.
    NewPassword,
    /// That one again, refused unless the two match.
    NewPasswordAgain,
    /// An invite's code.
    Invite,
}

/// The person at the terminal.
pub trait Terminal {
    /// A secret, read without echo.
    fn secret(&mut self, ask: Ask) -> Pin<Box<dyn Future<Output = Result<Secret, String>> + Send>>;
    /// A line for the person.
    fn say(&mut self, line: &str);
    /// Resolves when the person asks to stop what is running.
    fn stop(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// What a command comes to: done, or one plain sentence for the person.
pub(crate) type Outcome<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

/// Carries out the command, then parts every connection it opened, so each says goodbye before the process ends.
pub fn run<'a>(
    command: Command,
    terminal: &'a mut dyn Terminal,
    platform: &'a Platform,
    trust: &'a Trust,
) -> Outcome<'a> {
    Box::pin(async move {
        let parting = Parting::default();
        let outcome = dispatch(command, terminal, &Servers { platform, trust, parting: &parting }).await;
        parting.part().await;
        outcome
    })
}

/// How a command reaches servers: where this device keeps what it remembers, whom it trusts, and where it keeps the
/// connections it signs in on.
pub(crate) struct Servers<'a> {
    pub(crate) platform: &'a Platform,
    pub(crate) trust: &'a Trust,
    pub(crate) parting: &'a Parting,
}

/// The signed-in connections a command opened, parted once it is over (a process that ends drops a goodbye unwritten).
#[derive(Default)]
pub(crate) struct Parting(std::sync::Mutex<Vec<Link>>);

impl Parting {
    pub(crate) fn keep(&self, link: &Link) {
        if let Ok(mut kept) = self.0.lock() {
            kept.push(link.clone());
        }
    }

    async fn part(self) {
        for link in self.0.into_inner().unwrap_or_default() {
            link.part().await;
        }
    }
}

/// Builds the asked command's future, alone, and returns before it runs: a match awaiting every command in place
/// keeps a slot for each in one frame, which in a debug build overflowed Windows' 1 MiB main stack.
fn dispatch<'a>(command: Command, terminal: &'a mut dyn Terminal, servers: &'a Servers<'a>) -> Outcome<'a> {
    match command {
        Command::As { account: Account { address, identity }, act } => match act {
            Act::Register(display_name) => {
                Box::pin(account::register(address, identity, display_name, terminal, servers))
            }
            Act::SignIn => Box::pin(account::sign_in(address, identity, terminal, servers)),
            Act::SignOut => Box::pin(account::sign_out(address, identity, terminal, servers)),
            Act::ChangePassword => Box::pin(account::change_password(address, identity, terminal, servers)),
            Act::Channels => Box::pin(admin::channels(address, identity, terminal, servers)),
            Act::InviteCreate(terms) => Box::pin(admin::invite_create(address, identity, terms, terminal, servers)),
            Act::InviteList => Box::pin(admin::invite_list(address, identity, terminal, servers)),
            Act::InviteRevoke(invite_id) => {
                Box::pin(admin::invite_revoke(address, identity, invite_id, terminal, servers))
            }
            Act::MemberReset(user_id) => Box::pin(admin::member_reset(address, identity, user_id, terminal, servers)),
            Act::MemberList => Box::pin(admin::member_list(address, identity, terminal, servers)),
            Act::MemberRole { user_id, role, held } => {
                Box::pin(admin::member_role(address, identity, user_id, role, held, terminal, servers))
            }
            Act::MemberKick(user_id) => Box::pin(admin::member_kick(address, identity, user_id, terminal, servers)),
            Act::MemberBan { user_id, banned } => {
                Box::pin(admin::member_ban(address, identity, user_id, banned, terminal, servers))
            }
            Act::RoleList => Box::pin(admin::role_list(address, identity, terminal, servers)),
            Act::RoleCreate { name, colour } => {
                Box::pin(roles::create(address, identity, name, colour, terminal, servers))
            }
            Act::RoleRename { role, name } => Box::pin(roles::rename(address, identity, role, name, terminal, servers)),
            Act::RoleColour { role, colour } => {
                Box::pin(roles::colour(address, identity, role, colour, terminal, servers))
            }
            Act::RoleMove { role, before } => {
                Box::pin(roles::move_to(address, identity, role, before, terminal, servers))
            }
            Act::RoleDelete(role) => Box::pin(roles::delete(address, identity, role, terminal, servers)),
            Act::RoleWord { role, word, held } => {
                Box::pin(roles::word(address, identity, role, word, held, terminal, servers))
            }
            Act::OwnerTransfer(user_id) => {
                Box::pin(admin::owner_transfer(address, identity, user_id, terminal, servers))
            }
            Act::Leave => Box::pin(admin::leave(address, identity, terminal, servers)),
            Act::ChannelCreate { voice, name, category, access } => {
                let made = admin::NewChannel { voice, name, category, access };
                Box::pin(admin::channel_create(address, identity, made, terminal, servers))
            }
            Act::ChannelDelete(channel_id) => {
                Box::pin(admin::channel_delete(address, identity, channel_id, terminal, servers))
            }
            Act::ChannelMove { channel_id, category, before } => {
                Box::pin(order::channel_move(address, identity, channel_id, category, before, terminal, servers))
            }
            Act::ChannelOverwrites(channel_id) => {
                Box::pin(roles::overwrites(address, identity, channel_id, terminal, servers))
            }
            Act::ChannelOverwrite { channel_id, whom, word, overwrite } => {
                let set = roles::Overwritten { channel_id, whom, word, overwrite };
                Box::pin(roles::overwrite(address, identity, set, terminal, servers))
            }
            Act::CategoryCreate(name) => Box::pin(order::category_create(address, identity, name, terminal, servers)),
            Act::CategoryRename { category_id, name } => {
                Box::pin(order::category_rename(address, identity, category_id, name, terminal, servers))
            }
            Act::CategoryDelete(category_id) => {
                Box::pin(order::category_delete(address, identity, category_id, terminal, servers))
            }
            Act::CategoryMove { category_id, before } => {
                Box::pin(order::category_move(address, identity, category_id, before, terminal, servers))
            }
            Act::ServerPicture(file) => Box::pin(admin::server_picture(address, identity, file, terminal, servers)),
            Act::Speak { channel_id, tone } => {
                Box::pin(call::speak(address, identity, channel_id, tone, terminal, servers))
            }
            Act::Listen(channel_id) => Box::pin(call::listen(address, identity, channel_id, terminal, servers)),
            Act::Share { want, sound, channel_id } => {
                Box::pin(call::share(want, sound, address, identity, channel_id, terminal, servers))
            }
            Act::Watch { channel_id, to } => {
                Box::pin(call::watch(address, identity, channel_id, to, terminal, servers))
            }
        },
        Command::InviteCheck(address) => Box::pin(account::invite_check(address, terminal, servers)),
        Command::Devices => Box::pin(device::devices(terminal, servers.platform)),
        Command::UseMicrophone(name) => {
            Box::pin(device::use_device(name, device::Device::Microphone, terminal, servers.platform))
        }
        Command::UseSpeaker(name) => {
            Box::pin(device::use_device(name, device::Device::Speaker, terminal, servers.platform))
        }
        Command::Pick(want) => Box::pin(device::pick(want, terminal)),
        Command::Capture(want) => Box::pin(device::capture(want, terminal)),
    }
}

/// The sentence for a refused registration or password change. The server gives one answer for every field it
/// will not take, so the person is told what a password asks of them first.
fn chosen_sentence(refused: &LinkError) -> String {
    match refused {
        LinkError::Refused(ErrorCode::InvalidRequest) => {
            "That password is too short, or a name does not fit.".to_string()
        }
        other => crate::said(other),
    }
}

/// The engine a command makes its call on, in the command line's one-sentence errors.
async fn engine() -> Result<Engine, String> {
    ::call::devices::build().await.map_err(|refused| crate::failed(&::call::failure::voice(refused)))
}

/// How often the loudness of each stream being heard is said.
const LEVEL_EVERY: Duration = Duration::from_secs(1);

/// What a call of the command line is joined with before its command sets what it does: no device chosen, no
/// screen shown, the encoder survey and the voice chain of its own.
pub(crate) fn joining(
    channel_id: terms::ChannelId,
    me: Option<MemberId>,
    source: ::call::room::Source,
) -> ::call::room::Join {
    ::call::room::Join {
        channel_id,
        me,
        source,
        microphone: None,
        speaker: None,
        processing: choices::VoiceProcessing::default(),
        level_every: LEVEL_EVERY,
        screens: None,
        own_screen: None,
        own_camera: None,
        painting: None,
        jitter: jitter(),
        encoders: std::sync::Arc::new(media::Survey::system()),
        chain: ::call::ChainWatch::default(),
    }
}

/// The command's one source of randomness: the spread of a wait before a connection is made again.
fn jitter() -> screens::Jitter {
    use std::hash::{BuildHasher, Hasher};
    screens::Jitter::from_bits(std::hash::RandomState::new().build_hasher().finish())
}

/// Which of the addresses worth trying greets as a Chatkord server, said before any credential is asked for, with
/// the warning an unencrypted route carries.
async fn greet(found: &Address, servers: &Servers<'_>, terminal: &mut dyn Terminal) -> Result<Resolved, String> {
    let pin = signin::kept_key(servers.platform, found).await;
    let answered = signin::greet(servers.platform, found, pin, servers.trust, terms::BUILD)
        .await
        .map_err(|why| crate::said(&why))?;
    let host = answered.url.authority();
    terminal.say(&match route_warning(&answered.url) {
        Some(word) => format!("{host} · {}", crate::english(&Sentence::Phrase(word))),
        None => host.to_string(),
    });
    Ok(answered)
}

/// Signs in at the address that greeted, never at what was typed: a credential never crosses an address that turned
/// out to be something else, and the keyring keys on that address. The greeting connection is closed first.
async fn sign_in(
    found: &Address,
    identity: &str,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(Link, Events, LoginAck, Option<Secret>, ServerUrl), String> {
    let at = greet(found, servers, terminal).await?.url;
    let (session, typed) =
        errands::sign_in_at(servers.platform, &at, identity, servers.trust, terms::BUILD, &mut AtTerminal(terminal))
            .await
            .map_err(|stopped| match stopped {
                errands::Stopped::Failed(said) => crate::english(&said),
                errands::Stopped::Prompt(said) => said,
            })?;
    servers.parting.keep(&session.link);
    if session.ack.must_change_password {
        terminal.say("You signed in with a temporary password; choose your own with change-password.");
    }
    Ok((session.link, session.events, session.ack, typed, at))
}

/// The person at the terminal, as the one a sign-in asks for the password and tells what happens.
struct AtTerminal<'a>(&'a mut dyn Terminal);

impl errands::Prompt for AtTerminal<'_> {
    fn password(&mut self) -> impl Future<Output = Result<Secret, String>> + Send {
        self.0.secret(Ask::Password)
    }

    fn say(&mut self, said: Phrase) {
        self.0.say(&crate::english(&Sentence::Phrase(said)));
    }
}

/// Saves the token the server just issued, if it issued one; a keyring that cannot be used is said, not fatal.
async fn remember(platform: &Platform, url: &ServerUrl, identity: &str, ack: &LoginAck, terminal: &mut dyn Terminal) {
    let Some(token) = ack.session_token.clone().map(Secret::new) else { return };
    if platform::keyring::store_token(platform, url, identity, signin::named_identity(ack), token).await.is_err() {
        terminal.say(&crate::english(&Sentence::Phrase(Phrase::KeyringUnavailable)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_the_server_will_not_take_names_the_password_rule() {
        let refused = LinkError::Refused(ErrorCode::InvalidRequest);
        assert_eq!(chosen_sentence(&refused), "That password is too short, or a name does not fit.");
        let other = LinkError::Refused(ErrorCode::Unauthenticated);
        assert_eq!(chosen_sentence(&other), "This session is not signed in; sign in again.");
    }
}
