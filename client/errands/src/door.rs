//! The door's errands carried out over the keyring and the network, the same way for every host.

use crate::Session;
use crate::signin::{greet, greeting, named_identity, report, standing, with_password, with_token};
use link::Trust;
use platform::Platform;
use platform::keyring::{forget_token, load_token, store_token};
use proto::pin::KeyPin;
use screens::{Admitted, Errand, Found, Invite, Report, Server};
use terms::{Address, LinkError, Secret, ServerUrl};

/// Greets an address, trusting `pin` where no authority vouches, and with a code asks on the same connection whether
/// that invite works: the server ends the connection after it answers.
async fn find(
    platform: &Platform,
    address: &Address,
    code: Option<&Secret>,
    pin: Option<KeyPin>,
    trust: &Trust,
    build: &str,
) -> Result<Found, LinkError> {
    let greeted = greet(platform, address, pin, trust, build).await?;
    let said = greeting(&greeted.link);
    let server = Server { url: greeted.url.clone(), name: said.name, colour: said.colour, picture: said.picture };
    let invite = match code {
        Some(code) => {
            let standing = greeted.link.ask(proto::chatkord_v1::InviteInfo { code: code.expose().to_string() }).await?;
            Some(Invite::from(standing.usable))
        }
        None => None,
    };
    Ok(Found { server, invite })
}

/// Makes an account with an invite over a new connection.
async fn register(
    platform: &Platform,
    url: &ServerUrl,
    identity: &str,
    password: &Secret,
    code: &Secret,
    trust: &Trust,
    build: &str,
) -> Result<Session, LinkError> {
    let (link, events) = crate::signin::connect(platform, url, trust, build).await?;
    let ack = link.register(identity, None, password, code).await?;
    Ok(Session { link, events, ack })
}

/// What carrying out one errand came to: what to tell the door, if it waits to be told (forgetting a token is not
/// answered), and the signed-in connection if there is now one.
pub struct Carried {
    /// What to tell the door; none when it waits for no answer.
    pub heard: Option<screens::Heard>,
    /// The connection the errand signed in on, if it did.
    pub session: Option<Session>,
}

/// Carries out one errand over the keyring and the network, and says what happened as the door's own report.
pub async fn carry_out(platform: &Platform, errand: Errand, trust: &Trust, build: &str) -> Carried {
    use screens::Heard;
    let flow = |report: Report, session| Carried { heard: Some(Heard::Flow(report)), session };
    match errand {
        Errand::Find { address, code, pin } => {
            let found = find(platform, &address, code.as_ref(), pin, trust, build).await;
            Carried { heard: Some(Heard::Found(found)), session: None }
        }
        Errand::Register { server, identity, password, code } => {
            match register(platform, &server, &identity, &password, &code, trust, build).await {
                Ok(session) => match standing(&session.ack) {
                    Ok(standing) => {
                        let admitted = Admitted {
                            token: session.ack.session_token.clone().map(Secret::new),
                            named: named_identity(&session.ack),
                            standing,
                        };
                        Carried { heard: Some(Heard::Registered(Ok(admitted))), session: Some(session) }
                    }
                    Err(malformed) => Carried { heard: Some(Heard::Registered(Err(malformed))), session: None },
                },
                Err(why) => Carried { heard: Some(Heard::Registered(Err(why))), session: None },
            }
        }
        Errand::LoadToken(target) => {
            flow(Report::TokenLoaded(load_token(&target.server, &target.identity).await.ok().flatten()), None)
        }
        Errand::TryToken(target, token) => {
            let (reported, session) =
                report(with_token(platform, &target.server, &target.identity, &token, trust, build).await);
            flow(reported, session)
        }
        Errand::ForgetToken(target) => {
            let _ = forget_token(&target.server, &target.identity).await;
            Carried { heard: None, session: None }
        }
        Errand::SubmitPassword(target, password) => {
            let (reported, session) =
                report(with_password(platform, &target.server, &target.identity, &password, trust, build).await);
            flow(reported, session)
        }
        Errand::StoreToken { target, named, token } => flow(
            Report::Stored(store_token(platform, &target.server, &target.identity, named, token).await.is_ok()),
            None,
        ),
    }
}
