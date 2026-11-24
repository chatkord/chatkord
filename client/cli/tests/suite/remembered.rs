//! What one run leaves for the next, read back through the crate that owns that file: the server that greeted, then
//! the account that opened at it, kept when the server is greeted again. Each test runs in a process of its
//! own (nextest).

use crate::common;

use cli::{Account, Act, Ask, Command, run};
use common::{PASSWORD, Scripted, Server};
use errands::carry_out;
use link::Trust;
use platform::Platform;
use platform::state::{self, Row};
use screens::Errand;
use terms::{Address, ServerUrl};

const ADA: &str = "ada";
const BUILD: &str = "test";

async fn find(platform: &Platform, server: &Server) {
    let address = Address::typed(&server.host()).expect("worth trying");
    let found = carry_out(platform, Errand::Find { address, code: None, pin: None }, &Trust::system(), BUILD).await;
    assert!(found.heard.is_some(), "the door is told which server greeted");
}

mod server {
    use super::*;

    #[tokio::test]
    async fn a_run_remembers_the_server_that_greeted_then_its_account_and_a_greeting_keeps_it() {
        let (server, invites) = Server::start("remembered", 1).await;
        let platform = Platform::default().config_in(server.folder());
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a store"));
        let at = ServerUrl::parse(&server.url()).expect("an address");

        find(&platform, &server).await;
        assert_eq!(
            state::servers(&platform).await,
            [Row { server: at.clone(), name: None, colour: None, picture: None, identity: None, pin: None }]
        );

        let mut person = Scripted::answering(&[
            (Ask::Invite, Box::leak(invites[0].clone().into_boxed_str())),
            (Ask::Password, PASSWORD),
            (Ask::PasswordAgain, PASSWORD),
        ]);
        let address = Address::typed(&server.host()).expect("worth trying");
        run(
            Command::As { account: Account { address, identity: ADA.into() }, act: Act::Register(None) },
            &mut person,
            &platform,
            &Trust::system(),
        )
        .await
        .expect("registered");
        let signed = [Row {
            server: at.clone(),
            name: None,
            colour: None,
            picture: None,
            identity: Some(ADA.into()),
            pin: None,
        }];
        assert_eq!(
            state::servers(&platform).await,
            signed,
            "the account that signed in at it, by the name the server gave"
        );

        // Any later command greets first; the account stays.
        find(&platform, &server).await;
        assert_eq!(state::servers(&platform).await, signed);

        // A configuration that lost the account while the keyring kept its token: a sign-in by that token remembers
        // the account again, as one by password does.
        state::signed_out(&platform, at.clone()).await.expect("forgotten");
        assert_eq!(
            state::servers(&platform).await,
            [Row { server: at.clone(), name: None, colour: None, picture: None, identity: None, pin: None }]
        );
        let address = Address::typed(&server.host()).expect("worth trying");
        run(
            Command::As { account: Account { address, identity: ADA.into() }, act: Act::SignIn },
            &mut Scripted::answering(&[]),
            &platform,
            &Trust::system(),
        )
        .await
        .expect("signed in by the kept token, no password asked");
        assert_eq!(state::servers(&platform).await, signed, "the window finds the account again");
    }
}
