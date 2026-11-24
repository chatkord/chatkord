//! A server taken off the device while no connection to it is open, against the real server: the session its kept
//! token stands for is ended there, and a server that answers nothing is given up on within the bound.

use errands::signin::{Failure, end_kept};
use link::{Link, Trust};
use platform::{Platform, keyring};
use proto::chatkord_v1::ErrorCode;
use std::time::Duration;
use terms::{LinkError, Secret, ServerUrl};
use testserver::Server;
use tokio::time::Instant;

const PASSWORD: &str = "a test password that is long enough";
const ADA: &str = "ada";
const BEA: &str = "bea";
const WAIT: Duration = Duration::from_secs(10);

/// An account made at `url` whose token is in the keyring and whose connection is gone, as a lost connection leaves it.
async fn kept_token(url: &ServerUrl, identity: &str, invite: &str) -> Secret {
    let (link, events) = Link::connect(url, &Trust::system(), "end_kept", "test").await.expect("connected");
    let (password, invite) = (Secret::new(PASSWORD.into()), Secret::new(invite.into()));
    let ack = link.register(identity, None, &password, &invite).await.expect("registered");
    let token = Secret::new(ack.session_token.expect("a token"));
    keyring::store(url, identity, None, &token).expect("kept");
    drop((link, events));
    token
}

async fn signs_in(url: &ServerUrl, identity: &str, token: &Secret) -> Result<(), LinkError> {
    let (link, _events) = Link::connect(url, &Trust::system(), "end_kept", "test").await.expect("connected");
    link.sign_in_with_token(identity, token).await.map(|_| ())
}

mod server {
    use super::*;

    #[tokio::test]
    async fn a_kept_token_s_session_is_ended_at_the_server_and_a_silent_server_is_given_up_on() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let (server, invites) = Server::start("errands_end_kept", 2).await;
        let url = ServerUrl::parse(&server.url()).expect("a loopback address");
        let ada = kept_token(&url, ADA, &invites[0]).await;
        let _bea = kept_token(&url, BEA, &invites[1]).await;
        signs_in(&url, ADA, &ada).await.expect("the token opens a session while nothing ended it");

        end_kept(&Platform::default(), &url, ADA, &Trust::system(), "end_kept", WAIT).await.expect("ended");
        let refused = signs_in(&url, ADA, &ada).await;
        assert!(
            matches!(refused, Err(LinkError::Refused(ErrorCode::Unauthenticated))),
            "the token still opens a session: {refused:?}"
        );
        end_kept(&Platform::default(), &url, ADA, &Trust::system(), "end_kept", WAIT)
            .await
            .expect("a session already over is ended");

        server.pause();
        let asked = Instant::now();
        let gave_up =
            end_kept(&Platform::default(), &url, BEA, &Trust::system(), "end_kept", Duration::from_millis(500)).await;
        let took = asked.elapsed();
        server.resume();
        assert_eq!(gave_up, Err(Failure::Unreached(LinkError::Timeout)));
        assert!(took < Duration::from_secs(2), "given up on after {took:?}");
    }
}
