//! With no keyring at all (none installed, or locked) signing in still works with the password,
//! and the person is told the sign-in will not be remembered. Each test runs in a process of its
//! own (nextest) because the default keyring store is process-wide.

use crate::common;

use cli::{Account, Act, Ask, Command, run};
use common::{PASSWORD, Scripted, Server};
use link::Trust;
use platform::Platform;
use terms::Address;

mod server {
    use super::*;

    #[tokio::test]
    async fn a_machine_without_a_keyring_signs_in_with_the_password_and_says_it_will_not_remember() {
        let (server, invites) = Server::start("nokeyring", 1).await;
        // As a person types it: the address is answered before the password is asked.
        let address = Address::typed(&server.host()).expect("worth trying");
        let mut person = Scripted::answering(&[
            (Ask::Invite, Box::leak(invites[0].clone().into_boxed_str())),
            (Ask::Password, PASSWORD),
            (Ask::PasswordAgain, PASSWORD),
        ]);
        run(
            Command::As {
                account: Account { address: address.clone(), identity: "ada".into() },
                act: Act::Register(Some("Ada".into())),
            },
            &mut person,
            &Platform::default(),
            &Trust::system(),
        )
        .await
        .expect("registered");
        assert!(person.heard("keyring is not available"));

        let mut person = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(
            Command::As { account: Account { address, identity: "ada".into() }, act: Act::SignIn },
            &mut person,
            &Platform::default(),
            &Trust::system(),
        )
        .await
        .expect("signed in");
        assert_eq!(person.asked, vec![Ask::Password], "nothing was remembered, so the password is asked");
    }
}
