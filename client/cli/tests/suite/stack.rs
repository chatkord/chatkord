//! A command fits the stack the binary's `main` runs on: Windows gives the main thread 1 MiB, and the test harness's
//! threads more, so a test of its own runs the commands on a thread of that size, in a debug build like the gate's.

use crate::common;

use cli::{Account, Act, Ask, Command, run};
use common::{PASSWORD, Scripted, Server};
use link::Trust;
use platform::Platform;
use terms::Address;

/// The stack Windows gives a program's main thread unless its executable asks for more.
const MAIN_STACK: usize = 1024 * 1024;

mod server {
    use super::*;

    #[test]
    fn registering_signing_in_and_out_fit_the_main_threads_stack() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (server, invites) = runtime.block_on(Server::start("stack", 1));
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        let url = server.url();
        let said = std::thread::Builder::new()
            .stack_size(MAIN_STACK)
            .spawn(move || {
                let main = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                main.block_on(async {
                    let address = || Address::typed(&url).unwrap();
                    let trust = Trust::system();
                    let mut person = Scripted::answering(&[
                        (Ask::Invite, invite),
                        (Ask::Password, PASSWORD),
                        (Ask::PasswordAgain, PASSWORD),
                    ]);
                    let commands = [
                        Command::As {
                            account: Account { address: address(), identity: "ada".into() },
                            act: Act::Register(None),
                        },
                        Command::As {
                            account: Account { address: address(), identity: "ada".into() },
                            act: Act::SignIn,
                        },
                        Command::As {
                            account: Account { address: address(), identity: "ada".into() },
                            act: Act::SignOut,
                        },
                    ];
                    for command in commands {
                        run(command, &mut person, &Platform::default(), &trust).await.expect("the command ran");
                    }
                    person.said
                })
            })
            .unwrap()
            .join()
            .expect("the commands ran within the stack");
        assert!(said.iter().any(|line| line.starts_with("Signed out")), "signed out at the end: {said:?}");
        drop(server);
    }
}
