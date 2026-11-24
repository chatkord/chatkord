//! A sign-in over a new connection, by the saved token or the password, and what it came to as the sign-in model's
//! report, so every front end reads a refusal one way.

use crate::Session;
use link::{Events, Link, Resolved, Trust, Vouched};
use platform::Platform;
use platform::keyring::load_token;
use proto::chatkord_v1::{CloseReason, ErrorCode, LoginAck};
use proto::pin::KeyPin;
use screens::{Refusal, Report, Standing};
use std::time::Duration;
use terms::{Address, Ended, Heard, LinkError, MemberId, Secret, ServerUrl};

/// How long a refused token's connection is read for the reason its session ended.
const ENDING_WAIT: Duration = Duration::from_secs(1);

/// Why a sign-in did not happen.
#[derive(Clone, Debug, PartialEq, Eq, report::Public)]
pub enum Failure {
    /// The server does not take the saved token any more, and how its session ended when it said.
    Expired(Option<CloseReason>),
    /// The server answered and said no.
    Refused(Refusal),
    /// The address did not give a signed-in connection.
    Unreached(LinkError),
}

/// The identity the server named for the account, or none when it named none (an empty field is no name).
pub fn named_identity(ack: &LoginAck) -> Option<String> {
    if ack.identity.is_empty() { None } else { Some(ack.identity.clone()) }
}

/// The account's own id, as the server named it; none when it named none (an empty field is no id).
pub fn me(ack: &LoginAck) -> Option<MemberId> {
    MemberId::parse(&ack.user_id)
}

/// What the server said of the account as it let it in; an answer that names no account id or username is malformed.
/// An absent place is the bottom of the roles' order, as the wire says.
pub fn standing(ack: &LoginAck) -> Result<Standing, LinkError> {
    Ok(Standing {
        user_id: me(ack).ok_or(LinkError::Malformed)?,
        username: named_identity(ack).ok_or(LinkError::Malformed)?,
        shown: Some(ack.display_name.clone()).filter(|name| !name.is_empty()),
        place: proto::rules::Place::of_wire(ack.place),
        must_change: ack.must_change_password,
        grants: proto::rules::Grants::from_words(&ack.grants),
        // A colour out of shape is a server not keeping the protocol, and is passed over as none.
        colour: ack.colour.as_deref().and_then(|colour| proto::rules::Colour::parse(colour).ok()),
    })
}

/// A refusal is the server's answer to what was sent; anything else is about the address or the connection.
pub(crate) fn failure(why: LinkError) -> Failure {
    match why.kind() {
        terms::ErrorKind::Refused(code) => Failure::Refused(Refusal { code }),
        terms::ErrorKind::Lasting | terms::ErrorKind::Unreached => Failure::Unreached(why),
    }
}

/// A name that led nowhere is told as no network when this device has no route to any: the name was never the trouble.
pub(crate) async fn told(error: LinkError) -> LinkError {
    if error != LinkError::NoSuchHost {
        return error;
    }
    let routed = crate::errand::blocking(report::word!("check the route"), platform::network::has_default_route)
        .await
        .unwrap_or(true);
    told_with(error, routed)
}

/// `error` as told when the device has a route to a network (`routed`) or has none.
fn told_with(error: LinkError, routed: bool) -> LinkError {
    match error {
        LinkError::NoSuchHost if !routed => LinkError::NoNetwork,
        other => other,
    }
}

/// Opens a connection, trusting the key kept for the server if one is. `build` is the caller's version, which is all
/// the server is told of it besides the system's name. What the server greeted with is remembered beside its address,
/// so the next run shows its name and colour now; a kept key the system's verifier vouches for now is dropped.
pub async fn connect(
    platform: &Platform,
    url: &ServerUrl,
    trust: &Trust,
    build: &str,
) -> Result<(Link, Events), LinkError> {
    let kept = platform::state::trusted_key(platform, url.clone()).await;
    let connected = Link::connect(url, &trust.for_server(kept), build, std::env::consts::OS).await;
    let (link, events) = match connected {
        Ok(opened) => opened,
        Err(error) => return Err(told(error).await),
    };
    let _ = platform::state::remember_server(platform, url.clone(), link.greeted().clone()).await;
    if kept.is_some() && link.vouched() == Vouched::System {
        let _ = platform::state::drop_key(platform, url.clone()).await;
    }
    Ok((link, events))
}

/// The address that greets of those a person's words are worth trying as, trusting `pin` where no authority vouches,
/// and the connection that greeted. It is remembered with the name it gave, keeping an account already remembered at
/// it, and with `pin` when that is what vouched for it.
pub async fn greet(
    platform: &Platform,
    found: &Address,
    pin: Option<KeyPin>,
    trust: &Trust,
    build: &str,
) -> Result<Resolved, LinkError> {
    let resolved = Link::resolve(found, &trust.for_server(pin), build, std::env::consts::OS).await;
    let greeted = match resolved {
        Ok(greeted) => greeted,
        Err(error) => return Err(told(error).await),
    };
    let _ = platform::state::remember_server(platform, greeted.url.clone(), greeted.link.greeted().clone()).await;
    if let (Vouched::Pin, Some(pin)) = (greeted.link.vouched(), pin) {
        let _ = platform::state::trust_key(platform, greeted.url.clone(), pin).await;
    }
    Ok(greeted)
}

/// The key kept for the first of the addresses a person's words are worth trying as that has one.
pub async fn kept_key(platform: &Platform, found: &Address) -> Option<KeyPin> {
    for url in found.tried() {
        if let Some(pin) = platform::state::trusted_key(platform, url.clone()).await {
            return Some(pin);
        }
    }
    None
}

/// How a refused token's session ended: the server closes the connection with the reason right after the refusal.
async fn ending(mut events: Events) -> Option<CloseReason> {
    let read = async {
        loop {
            match events.next().await {
                Some(Heard::Event(_) | Heard::Behind) => continue,
                Some(Heard::Ended(Ended { reason, .. })) => return reason,
                None => return None,
            }
        }
    };
    tokio::time::timeout(ENDING_WAIT, read).await.ok().flatten()
}

/// Signs in with the saved token over a new connection.
pub async fn with_token(
    platform: &Platform,
    url: &ServerUrl,
    identity: &str,
    token: &Secret,
    trust: &Trust,
    build: &str,
) -> Result<Session, Failure> {
    let (link, events) = connect(platform, url, trust, build).await.map_err(failure)?;
    match link.sign_in_with_token(identity, token).await {
        Ok(ack) => Ok(Session { link, events, ack }),
        Err(LinkError::Refused(ErrorCode::Unauthenticated)) => Err(Failure::Expired(ending(events).await)),
        Err(other) => Err(failure(other)),
    }
}

/// Signs in with the password over a new connection: the server ends a connection whose sign-in it refused.
pub async fn with_password(
    platform: &Platform,
    url: &ServerUrl,
    identity: &str,
    password: &Secret,
    trust: &Trust,
    build: &str,
) -> Result<Session, Failure> {
    let (link, events) = connect(platform, url, trust, build).await.map_err(failure)?;
    match link.sign_in_with_password(identity, password).await {
        Ok(ack) => Ok(Session { link, events, ack }),
        // A proved password the server still turns away (a ban) is closed with why, which is the failure to say.
        Err(LinkError::Refused(code)) => match ending(events).await {
            Some(reason) if reason != CloseReason::Normal => {
                let gone = Ended { reason: Some(reason), refusal: Some(code), ..Ended::default() };
                Err(failure(LinkError::Closed(gone)))
            }
            Some(_) | None => Err(failure(LinkError::Refused(code))),
        },
        Err(other) => Err(failure(other)),
    }
}

/// Ends at the server the session this account's saved token stands for, over a connection made for it, for a server
/// taken off the device while none is open. No saved token, or one the server no longer takes, is a session already
/// over. Past `within` it gives up: the caller forgets the token either way, and the session lapses at the server.
pub async fn end_kept(
    platform: &Platform,
    url: &ServerUrl,
    identity: &str,
    trust: &Trust,
    build: &str,
    within: Duration,
) -> Result<(), Failure> {
    let end = async {
        let Some(token) = load_token(url, identity).await.ok().flatten() else { return Ok(()) };
        // Not `connect`: a server being forgotten is not remembered again.
        let trust = trust.for_server(platform::state::trusted_key(platform, url.clone()).await);
        let (link, _events) = Link::connect(url, &trust, build, std::env::consts::OS).await.map_err(failure)?;
        match link.sign_in_with_token(identity, &token).await {
            Ok(_) => link.ask(proto::chatkord_v1::SignOut {}).await.map(|_| ()).map_err(failure),
            Err(LinkError::Refused(ErrorCode::Unauthenticated)) => Ok(()),
            Err(other) => Err(failure(other)),
        }
    };
    tokio::time::timeout(within, end).await.unwrap_or(Err(Failure::Unreached(LinkError::Timeout)))
}

/// What a sign-in attempt came to, as the report the sign-in model takes, and the connection if it signed in; one
/// whose answer is malformed is not kept.
pub fn report(attempt: Result<Session, Failure>) -> (Report, Option<Session>) {
    match attempt {
        Ok(session) => match standing(&session.ack) {
            Ok(standing) => (
                Report::Accepted {
                    token: session.ack.session_token.clone().map(Secret::new),
                    named: named_identity(&session.ack),
                    standing,
                },
                Some(session),
            ),
            Err(malformed) => (Report::Unreached(malformed), None),
        },
        Err(Failure::Expired(reason)) => (Report::Expired(reason), None),
        Err(Failure::Refused(refusal)) => (Report::Refused(refusal), None),
        Err(Failure::Unreached(failed)) => (Report::Unreached(failed), None),
    }
}

/// What a server greeted a connection with: its name, colour and picture.
pub fn greeting(link: &Link) -> terms::ServerIdentity {
    link.greeted().clone()
}

#[cfg(test)]
mod tests {
    use proto::chatkord_v1::LoginAck;
    use terms::LinkError;

    use super::{named_identity, standing, told_with};

    const ADA: &str = "ada";

    fn ack(identity: &str) -> LoginAck {
        LoginAck {
            session_token: Some("t".into()),
            display_name: "Ada".into(),
            identity: identity.into(),
            place: proto::rules::Place::OWNER.wire(),
            must_change_password: true,
            grants: vec!["MANAGE_SERVER".into()],
            user_id: identity.into(),
            colour: None,
        }
    }

    #[test]
    fn a_name_that_led_nowhere_is_no_network_only_where_no_route_exists() {
        assert_eq!(told_with(LinkError::NoSuchHost, false), LinkError::NoNetwork);
        assert_eq!(told_with(LinkError::NoSuchHost, true), LinkError::NoSuchHost);
        for other in [LinkError::Connect, LinkError::Timeout, LinkError::Offline] {
            assert_eq!(told_with(other.clone(), false), other, "only a name that led nowhere is told again");
        }
    }

    #[test]
    fn an_identity_the_server_named_is_read_as_the_name_it_gave() {
        assert_eq!(named_identity(&ack(ADA)), Some(ADA.into()));
    }

    #[test]
    fn a_peer_that_names_no_identity_is_read_as_naming_none() {
        assert_eq!(named_identity(&ack("")), None, "an empty name is no name");
    }

    #[test]
    fn the_standing_is_read_from_the_ack() {
        let read = standing(&ack(ADA)).expect("an account id");
        assert_eq!(read.user_id.as_str(), ADA);
        assert!(read.place == proto::rules::Place::OWNER && read.must_change);
        assert_eq!(read.username, ADA, "and the username");
        assert!(read.grants.holds(proto::rules::Grant::ManageServer), "the words are read too");
        assert_eq!(read.shown.as_deref(), Some("Ada"), "and the name others see");
    }

    #[test]
    fn a_sign_in_whose_answer_names_no_account_id_is_malformed() {
        let nameless = LoginAck { user_id: String::new(), ..ack(ADA) };
        assert_eq!(standing(&nameless), Err(LinkError::Malformed), "\"unknown\" is no id");
    }
}
