//! The command line against a real server process, with the keyring replaced by the library's
//! in-memory store: register, sign in on the saved token, lists and joins, a wrong password,
//! a stale token, ending a session, every session and a password.

use crate::common;

use cli::{Access, Account, Act, Ask, Command, Whom, run};
use common::{PASSWORD, Scripted, Server};
use keyring_core::Entry;
use link::{Link, Trust};
use platform::Platform;
use proto::chatkord_v1::{ChannelAccess, ChannelsList, CloseReason, ErrorCode};
use std::sync::Once;
use std::time::Duration;
use terms::{Address, Heard, LinkError, Secret, ServerUrl};

const ADA: &str = "ada";
const GRACE: &str = "grace";

fn in_memory_keyring() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a store")));
}

fn saved(url: &str, identity: &str) -> Option<String> {
    Entry::new(platform::keyring::SERVICE, &format!("{identity}@{url}")).expect("entry").get_password().ok()
}

/// This machine's plain socket, named in full: what a script writes.
fn url(server: &Server) -> ServerUrl {
    ServerUrl::parse(&server.url()).expect("a loopback address")
}

/// The same server as a person types it, which is the addresses worth trying.
fn address(server: &Server) -> Address {
    Address::typed(&server.url()).expect("worth trying")
}

/// `act`, done on `server` as `identity`.
fn on(server: &Server, identity: &str, act: Act) -> Command {
    Command::As { account: Account { address: address(server), identity: identity.to_string() }, act }
}

async fn register(server: &Server, identity: &str, invite: &'static str, display: &str) -> Scripted {
    let mut person =
        Scripted::answering(&[(Ask::Invite, invite), (Ask::Password, PASSWORD), (Ask::PasswordAgain, PASSWORD)]);
    let command = on(server, identity, Act::Register(Some(display.to_string())));
    run(command, &mut person, &Platform::default(), &Trust::system()).await.expect("registered");
    person
}

mod server {
    use super::*;

    #[tokio::test]
    async fn registering_saves_the_token_and_the_next_sign_in_needs_no_password() {
        in_memory_keyring();
        let (server, invites) = Server::start("save", 1).await;
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        let person = register(&server, ADA, invite, "Ada").await;
        assert!(person.heard("Registered"));
        let first = saved(&server.url(), ADA).expect("the token was saved");

        let mut person = Scripted::default();
        run(on(&server, ADA, Act::SignIn), &mut person, &Platform::default(), &Trust::system())
            .await
            .expect("signed in");
        assert!(person.asked.is_empty(), "the saved token was enough, but the person was asked for {:?}", person.asked);
        let second = saved(&server.url(), ADA).expect("a token is still saved");
        assert_eq!(first, second, "a device keeps its token: a token sign-in issues no new one");
    }

    #[tokio::test]
    async fn channels_are_listed_with_how_many_are_in_voice() {
        in_memory_keyring();
        let (server, invites) = Server::start("channels", 1).await;
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        register(&server, ADA, invite, "Ada").await;

        let mut person = Scripted::default();
        run(on(&server, ADA, Act::Channels), &mut person, &Platform::default(), &Trust::system())
            .await
            .expect("listed");
        let line = person.said.iter().find(|l| l.contains("Booth")).expect("the voice channel is listed");
        assert!(line.contains("\tvoice\t"), "{line}");
        assert!(line.ends_with("0 in voice"), "nobody is in its room yet: {line}");
    }

    #[tokio::test]
    async fn a_wrong_password_is_refused_and_nothing_is_saved() {
        in_memory_keyring();
        let (server, invites) = Server::start("wrong", 1).await;
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        register(&server, GRACE, invite, "Grace").await;
        Entry::new(platform::keyring::SERVICE, &format!("{GRACE}@{}", server.url()))
            .expect("entry")
            .delete_credential()
            .expect("cleared");

        let mut person = Scripted::answering(&[(Ask::Password, "not the password at all")]);
        let result = run(on(&server, GRACE, Act::SignIn), &mut person, &Platform::default(), &Trust::system()).await;
        assert!(result.is_err());
        assert_eq!(saved(&server.url(), GRACE), None, "a refused sign-in saves nothing");
    }

    #[tokio::test]
    async fn a_saved_token_the_server_no_longer_takes_is_dropped_and_the_password_is_asked_once() {
        in_memory_keyring();
        let (server, invites) = Server::start("stale", 1).await;
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        register(&server, ADA, invite, "Ada").await;
        Entry::new(platform::keyring::SERVICE, &format!("{ADA}@{}", server.url()))
            .expect("entry")
            .set_password("0000")
            .expect("overwritten");

        let mut person = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, Act::SignIn), &mut person, &Platform::default(), &Trust::system())
            .await
            .expect("signed in");
        assert!(person.heard("expired"));
        assert_eq!(person.asked, vec![Ask::Password]);
        assert_ne!(saved(&server.url(), ADA).as_deref(), Some("0000"), "the stale token was replaced");
    }

    #[tokio::test]
    async fn signing_out_ends_this_socket_s_session_and_leaves_the_account_s_other_one() {
        in_memory_keyring();
        let (server, invites) = Server::start("out", 1).await;
        register(&server, ADA, Box::leak(invites[0].clone().into_boxed_str()), "Ada").await;

        // A sign-out ends the session its socket asked with, not the account's other one.
        let (holder, _events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a second socket");
        holder.sign_in_with_password(ADA, &Secret::new(PASSWORD.into())).await.expect("a second session");

        let mut person = Scripted::default();
        run(on(&server, ADA, Act::SignOut), &mut person, &Platform::default(), &Trust::system())
            .await
            .expect("signed out");
        assert_eq!(saved(&server.url(), ADA), None, "the saved sign-in is gone from this device");

        // The other session still answers; this one is over, so signing in again starts at the password.
        holder.ask(ChannelsList {}).await.expect("the account's other session answers");
        let mut back = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, Act::SignIn), &mut back, &Platform::default(), &Trust::system())
            .await
            .expect("signed in again");
        assert_eq!(back.asked, vec![Ask::Password], "the sign-out left nothing here to sign in on");
    }

    /// How the link ended, passing over what it was told before.
    async fn ended(events: &mut link::Events) -> Option<terms::Ended> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.next().await? {
                    Heard::Ended(ended) => return Some(ended),
                    Heard::Event(_) | Heard::Behind => {}
                }
            }
        })
        .await
        .ok()
        .flatten()
    }

    #[tokio::test]
    async fn changing_a_password_replaces_it_and_ends_every_other_session() {
        in_memory_keyring();
        const NEW: &str = "another correct phrase entirely";
        let (server, invites) = Server::start("phi", 1).await;
        register(&server, ADA, Box::leak(invites[0].clone().into_boxed_str()), "Ada").await;
        let (holder, mut events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a second socket");
        holder.sign_in_with_password(ADA, &Secret::new(PASSWORD.into())).await.expect("a second session");

        // Signed in on the saved token, so the password in force is asked for, and the new one twice: a mistyped
        // one would lock the person out of their own account.
        let mut person =
            Scripted::answering(&[(Ask::Password, PASSWORD), (Ask::NewPassword, NEW), (Ask::NewPasswordAgain, NEW)]);
        run(on(&server, ADA, Act::ChangePassword), &mut person, &Platform::default(), &Trust::system())
            .await
            .expect("changed");
        assert_eq!(
            person.asked,
            vec![Ask::Password, Ask::NewPassword, Ask::NewPasswordAgain],
            "the password in force is part of the ask, so it is asked of the person too",
        );
        assert!(saved(&server.url(), ADA).is_some(), "the session that asked is the person's own and keeps its token");
        let Some(ended) = ended(&mut events).await else { panic!("the other link was not ended") };
        assert_eq!(
            ended.reason,
            Some(CloseReason::PasswordChanged),
            "a token that signed for the old password is gone"
        );
        let mut kept = Scripted::default();
        run(on(&server, ADA, Act::SignIn), &mut kept, &Platform::default(), &Trust::system())
            .await
            .expect("the token saved here still signs in");
        assert!(kept.asked.is_empty(), "and needs no password: {:?}", kept.asked);

        // The old password is not the account's, and the new one opens a new session.
        let (stale, _events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a third socket");
        let refused = stale.sign_in_with_password(ADA, &Secret::new(PASSWORD.into())).await;
        assert!(refused.is_err(), "the old password no longer opens the account");
        let (fresh, _events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a fourth socket");
        fresh.sign_in_with_password(ADA, &Secret::new(NEW.into())).await.expect("signed in with the new password");
    }

    #[tokio::test]
    async fn two_passwords_that_differ_stop_a_registration_before_the_server_is_asked() {
        in_memory_keyring();
        struct Differs(u8);
        impl cli::Terminal for Differs {
            fn secret(
                &mut self,
                ask: Ask,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Secret, String>> + Send>> {
                self.0 += 1;
                Box::pin(std::future::ready(Ok(Secret::new(format!("{ask:?}{}", self.0)))))
            }
            fn say(&mut self, _: &str) {}
            fn stop(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
                Box::pin(std::future::pending())
            }
        }
        let command = Command::As {
            account: Account {
                address: Address::typed("ws://127.0.0.1:9").expect("nothing listening there"),
                identity: ADA.into(),
            },
            act: Act::Register(Some("Ada".into())),
        };
        let result = run(command, &mut Differs(0), &Platform::default(), &Trust::system()).await;
        assert_eq!(result, Err("The two passwords are not the same.".to_string()));
    }

    #[tokio::test]
    async fn a_password_change_stops_here_when_the_two_asked_do_not_match() {
        in_memory_keyring();
        const NEW: &str = "another correct phrase entirely";
        let (server, invites) = Server::start("phi-stop", 1).await;
        register(&server, ADA, Box::leak(invites[0].clone().into_boxed_str()), "Ada").await;

        // A mistyped new password is caught before the server is asked.
        let mut person = Scripted::answering(&[
            (Ask::Password, PASSWORD),
            (Ask::NewPassword, NEW),
            (Ask::NewPasswordAgain, PASSWORD),
        ]);
        let stopped =
            run(on(&server, ADA, Act::ChangePassword), &mut person, &Platform::default(), &Trust::system()).await;
        assert_eq!(stopped, Err("The two passwords are not the same.".to_string()));
        assert!(saved(&server.url(), ADA).is_some(), "the account keeps the password it had");
        let mut back = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, Act::SignIn), &mut back, &Platform::default(), &Trust::system())
            .await
            .expect("the old password still opens the account");
    }

    #[tokio::test]
    async fn a_wrong_current_password_is_refused_in_the_sign_in_s_words() {
        in_memory_keyring();
        const NEW: &str = "another correct phrase entirely";
        const STRANGER: &str = "the password a stranger would guess";
        let (server, invites) = Server::start("phi-proof", 1).await;
        register(&server, ADA, Box::leak(invites[0].clone().into_boxed_str()), "Ada").await;

        // A wrong current password is refused as a wrong sign-in is: what a copied token alone cannot get past.
        let mut person =
            Scripted::answering(&[(Ask::Password, STRANGER), (Ask::NewPassword, NEW), (Ask::NewPasswordAgain, NEW)]);
        let refused =
            run(on(&server, ADA, Act::ChangePassword), &mut person, &Platform::default(), &Trust::system()).await;
        assert_eq!(
            person.asked,
            vec![Ask::Password, Ask::NewPassword, Ask::NewPasswordAgain],
            "the current password is asked for, not assumed"
        );
        assert!(refused.is_err(), "a wrong current password ends nothing and changes nothing");

        // A change that was not proved wrote nothing.
        let (holder, _events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a socket of its own");
        holder
            .sign_in_with_password(ADA, &Secret::new(PASSWORD.into()))
            .await
            .expect("the password in force still signs in");
        let (stranger, _other_events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("another socket");
        let stayed_out = stranger.sign_in_with_password(ADA, &Secret::new(STRANGER.into())).await;
        assert!(
            matches!(stayed_out, Err(LinkError::Refused(ErrorCode::Unauthenticated))),
            "the password that was refused as proof is not the account's",
        );
    }

    #[tokio::test]
    async fn a_change_to_the_password_in_force_stops_before_the_server_is_asked() {
        in_memory_keyring();
        let (server, invites) = Server::start("phi-same", 1).await;
        register(&server, ADA, Box::leak(invites[0].clone().into_boxed_str()), "Ada").await;

        // A change that changes nothing is not asked of the server.
        let mut person = Scripted::answering(&[
            (Ask::Password, PASSWORD),
            (Ask::NewPassword, PASSWORD),
            (Ask::NewPasswordAgain, PASSWORD),
        ]);
        let stopped =
            run(on(&server, ADA, Act::ChangePassword), &mut person, &Platform::default(), &Trust::system()).await;
        assert_eq!(stopped, Err("That is the password the account already has.".to_string()));
        assert_eq!(
            person.asked,
            vec![Ask::Password, Ask::NewPassword, Ask::NewPasswordAgain],
            "the person was asked as if for a change, and stopped before the socket was",
        );
        let mut back = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, Act::SignIn), &mut back, &Platform::default(), &Trust::system())
            .await
            .expect("the account kept its password");
    }

    #[tokio::test]
    async fn an_invite_is_checked_and_a_member_leaves_with_their_password() {
        in_memory_keyring();
        let (server, invites) = Server::start("leave", 2).await;
        let code: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        let mut checking = Scripted::answering(&[(Ask::Invite, code)]);
        run(Command::InviteCheck(address(&server)), &mut checking, &Platform::default(), &Trust::system())
            .await
            .expect("checked");
        assert!(checking.heard("That invite works until"), "a working invite is told so");
        register(&server, ADA, code, "Ada").await;
        let mut again = Scripted::answering(&[(Ask::Invite, code)]);
        run(Command::InviteCheck(address(&server)), &mut again, &Platform::default(), &Trust::system())
            .await
            .expect("checked");
        assert!(again.heard("doesn't work"), "a spent one is told so");

        let mut wrong = Scripted::answering(&[(Ask::Password, "not the password this account has")]);
        let refused = run(on(&server, ADA, Act::Leave), &mut wrong, &Platform::default(), &Trust::system()).await;
        assert_eq!(refused, Err("That is not this account's password; nothing was deleted.".to_string()));
        let mut person = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, Act::Leave), &mut person, &Platform::default(), &Trust::system()).await.expect("left");
        assert_eq!(
            person.asked,
            vec![Ask::Password],
            "signed in on the saved token, so the password is asked for the proof"
        );
        assert_eq!(saved(&server.url(), ADA), None, "nothing is remembered of an account that is gone");
        let mut back = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        let refused = run(on(&server, ADA, Act::SignIn), &mut back, &Platform::default(), &Trust::system()).await;
        assert!(refused.is_err(), "the account no longer signs in");
    }

    /// Roles by name at the terminal, by id on the wire: listed with their ids and the owner's and default flags, given by a name the role
    /// list resolves in any case, shown by name in the member list, and a name no role has refused before anything is asked.
    #[tokio::test]
    async fn a_role_named_at_the_terminal_is_given_by_its_id() {
        in_memory_keyring();
        let (server, invites) = Server::start("roles", 1).await;
        let owner: &'static str = Box::leak(server.owner_code().into_boxed_str());
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        register(&server, ADA, owner, "Ada").await;
        register(&server, GRACE, invite, "Grace").await;
        let mut roles = Scripted::default();
        run(on(&server, ADA, Act::RoleList), &mut roles, &Platform::default(), &Trust::system()).await.expect("listed");
        let name = |line: &str| line.split('\t').nth(2).expect("a name").to_string();
        let admin = roles.said.iter().find(|line| line.split('\t').nth(2) == Some("Admin")).expect("the admin role");
        assert!(admin.split('\t').next().is_some_and(|id| id.parse::<u64>().is_ok()), "listed by its id: {admin}");
        let member = roles.said.iter().find(|line| line.ends_with("\teveryone holds it")).expect("the default role");
        let (admin, member) = (name(admin), name(member));
        let mut members = Scripted::default();
        run(on(&server, ADA, Act::MemberList), &mut members, &Platform::default(), &Trust::system())
            .await
            .expect("listed");
        let grace = members.said.iter().find(|line| line.contains("\tgrace\t")).expect("grace").to_string();
        let grace_id = grace.split('\t').next().expect("an id").to_string();
        let give = |role: &str| Act::MemberRole { user_id: grace_id.clone(), role: role.to_string(), held: true };
        let unknown = run(
            on(&server, ADA, give("no such role")),
            &mut Scripted::default(),
            &Platform::default(),
            &Trust::system(),
        )
        .await;
        assert_eq!(unknown, Err("No role is called no such role.".to_string()));
        run(
            on(&server, ADA, give(&admin.to_uppercase())),
            &mut Scripted::default(),
            &Platform::default(),
            &Trust::system(),
        )
        .await
        .expect("given");
        let mut after = Scripted::default();
        run(on(&server, ADA, Act::MemberList), &mut after, &Platform::default(), &Trust::system())
            .await
            .expect("listed");
        let grace = after.said.iter().find(|line| line.contains("\tgrace\t")).expect("grace");
        assert!(grace.ends_with(&format!("\t{admin}, {member}")), "by name, in the roles' order: {grace}");
    }

    /// A server's picture from a file, against the real server: refused before it is sent when it is not a picture,
    /// refused to a member, set by the server owner, named in the next greeting and fetched as the server's own square, and
    /// taken off again.
    #[tokio::test]
    async fn the_owner_sets_the_servers_picture_from_a_file_and_takes_it_off() {
        use proto::files::FileKind;
        in_memory_keyring();
        let (server, invites) = Server::start("picture", 1).await;
        let owner: &'static str = Box::leak(server.owner_code().into_boxed_str());
        let invite: &'static str = Box::leak(invites[0].clone().into_boxed_str());
        register(&server, ADA, owner, "Ada").await;
        register(&server, GRACE, invite, "Grace").await;
        let folder = std::env::temp_dir().join(format!("chatkord-cli-picture-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let photo = folder.join("owl.png");
        image::RgbaImage::from_pixel(600, 400, image::Rgba([40, 90, 200, 255])).save(&photo).unwrap();
        let text = folder.join("owl.txt");
        std::fs::write(&text, "not a picture").unwrap();
        let set = |identity: &str, file: Option<&std::path::Path>| {
            on(&server, identity, Act::ServerPicture(file.map(|file| file.display().to_string())))
        };
        let greeted = || async {
            let url = ServerUrl::parse(&server.url()).unwrap();
            let (link, _events) = Link::connect(&url, &Trust::system(), "test", "test").await.unwrap();
            link.greeted().picture
        };

        let refused =
            run(set(ADA, Some(&text)), &mut Scripted::default(), &Platform::default(), &Trust::system()).await;
        assert_eq!(refused, Err("That isn’t a PNG, JPEG or WebP picture.".to_string()), "the Words board's sentence");
        let refused =
            run(set(GRACE, Some(&photo)), &mut Scripted::default(), &Platform::default(), &Trust::system()).await;
        assert_eq!(refused, Err("You can’t change this server’s picture.".to_string()));
        assert_eq!(greeted().await, None, "nothing was set");

        let mut person = Scripted::default();
        run(set(ADA, Some(&photo)), &mut person, &Platform::default(), &Trust::system()).await.expect("set");
        assert!(person.heard("picture is set"));
        let id = greeted().await.expect("the next greeting names it");
        let url = ServerUrl::parse(&server.url()).unwrap();
        let grace = errands::signin::with_password(
            &Platform::default(),
            &url,
            GRACE,
            &Secret::new(PASSWORD.into()),
            &Trust::system(),
            "test",
        )
        .await
        .map_err(|_| ())
        .expect("a member signs in");
        let kept = grace.link.fetch(FileKind::ServerPicture, id).await.expect("a member sees it");
        portrait::after_fetch(&kept).expect("the server's own square");

        run(set(ADA, None), &mut Scripted::default(), &Platform::default(), &Trust::system()).await.expect("taken off");
        assert_eq!(greeted().await, None, "and the next greeting names none");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// `act` done on `server` as `identity`, asked nothing, and what the person was told.
    async fn done(server: &Server, identity: &str, act: Act) -> Result<Scripted, String> {
        let mut person = Scripted::default();
        run(on(server, identity, act), &mut person, &Platform::default(), &Trust::system()).await.map(|()| person)
    }

    /// The line `listed` printed whose `field`th field is `value`.
    fn line_of(listed: &Scripted, field: usize, value: &str) -> String {
        listed.said.iter().find(|line| line.split('\t').nth(field) == Some(value)).cloned().expect(value)
    }

    /// The id a printed line starts with.
    fn id_of(line: &str) -> String {
        line.split('\t').next().expect("an id").to_string()
    }

    /// A server whose owner is Ada, signed in on this device. Each op is a run of its own, connecting afresh, so a
    /// test of many passes the accept rate one address is held to.
    async fn owned(tag: &str, invites: usize) -> (Server, Vec<String>) {
        in_memory_keyring();
        let (server, codes) = Server::start_with(tag, invites, &[(testserver::Limit::AcceptsPerSecond, 1000)]).await;
        let owner: &'static str = Box::leak(server.owner_code().into_boxed_str());
        register(&server, ADA, owner, "Ada").await;
        (server, codes)
    }

    /// A role's place and words in `role list`, and how a role is made, given a word, moved, renamed and recoloured by
    /// name; a name two roles share is refused before anything is asked.
    #[tokio::test]
    async fn a_role_is_made_given_words_moved_and_renamed_by_its_name() {
        use proto::rules::{Colour, Grant};
        let (server, _) = owned("role-ops", 0).await;
        let teal = Colour::parse("#2A9D8F").expect("a colour");
        let made = done(&server, ADA, Act::RoleCreate { name: "stewards".into(), colour: Some(teal) }).await;
        assert!(made.expect("made").heard("Role created"));
        let word = |held| Act::RoleWord { role: "stewards".into(), word: Grant::ManageMessages, held };
        done(&server, ADA, word(true)).await.expect("given");

        let listed = done(&server, ADA, Act::RoleList).await.expect("listed");
        let place = |line: &str| line.split('\t').nth(1).and_then(|place| place.parse::<u32>().ok()).expect("a place");
        let stewards = line_of(&listed, 2, "stewards");
        assert!(stewards.ends_with("\tMANAGE_MESSAGES"), "listed with its words: {stewards}");
        let admin = line_of(&listed, 2, "Admin");
        assert!(place(&admin) < place(&stewards), "made just above the default role: {admin} / {stewards}");
        let member = line_of(&listed, 2, "Member");
        assert!(member.ends_with("\teveryone holds it"), "{member}");

        done(&server, ADA, Act::RoleMove { role: "stewards".into(), before: Some("admin".into()) })
            .await
            .expect("moved");
        done(&server, ADA, word(false)).await.expect("taken");
        let listed = done(&server, ADA, Act::RoleList).await.expect("listed");
        let (stewards, admin) = (line_of(&listed, 2, "stewards"), line_of(&listed, 2, "Admin"));
        assert!(place(&stewards) < place(&admin), "now above admin: {stewards} / {admin}");
        let at = |line: &String| listed.said.iter().position(|said| said == line);
        assert!(at(&stewards) < at(&admin), "listed in the order");
        assert!(stewards.ends_with("\tno words"), "{stewards}");

        done(&server, ADA, Act::RoleRename { role: "stewards".into(), name: "moderators".into() })
            .await
            .expect("renamed");
        done(&server, ADA, Act::RoleColour { role: "moderators".into(), colour: None }).await.expect("recoloured");
        done(&server, ADA, Act::RoleCreate { name: "moderators".into(), colour: None }).await.expect("made");
        let twice = done(&server, ADA, Act::RoleDelete("moderators".into())).await;
        assert_eq!(twice.err(), Some("More than one role is called moderators.".to_string()));
        let unknown = done(&server, ADA, Act::RoleDelete("stewards".into())).await;
        assert_eq!(unknown.err(), Some("No role is called stewards.".to_string()), "renamed, the old name is gone");
    }

    /// A channel's overwrite for a role, then for a member, set, listed and left to the roles' own words again, and
    /// what each does to the member's words there; a channel made private is hidden from them from its start.
    #[tokio::test]
    async fn a_channel_overwrite_is_set_listed_and_taken_off() {
        use proto::rules::{Grant, Grants, Overwrite};
        let (server, invites) = owned("overwrite", 1).await;
        register(&server, GRACE, Box::leak(invites[0].clone().into_boxed_str()), "Grace").await;
        let channels = done(&server, ADA, Act::Channels).await.expect("listed");
        let general = id_of(&line_of(&channels, 2, testserver::GENERAL));
        let roles = done(&server, ADA, Act::RoleList).await.expect("listed");
        let everyone = roles.said.iter().find(|line| line.ends_with("\teveryone holds it")).expect("the default role");
        let everyone = everyone.split('\t').nth(2).expect("its name").to_string();
        let grace = errands::signin::with_password(
            &Platform::default(),
            &url(&server),
            GRACE,
            &Secret::new(PASSWORD.into()),
            &Trust::system(),
            "test",
        )
        .await
        .map_err(|_| ())
        .expect("a member signs in");
        let words_there = || async {
            let listed = grace.link.ask(ChannelsList {}).await.expect("listed");
            let standing = listed.standings.iter().find(|standing| standing.channel_id == general).expect("general");
            Grants::from_words(&standing.grants)
        };
        assert!(words_there().await.holds(Grant::SendMessage), "a member sends in #general");

        let set = |whom, overwrite| Act::ChannelOverwrite {
            channel_id: general.clone(),
            whom,
            word: Grant::SendMessage,
            overwrite,
        };
        done(&server, ADA, set(Whom::Role(everyone.clone()), Some(Overwrite::Deny))).await.expect("denied");
        let listed = done(&server, ADA, Act::ChannelOverwrites(general.clone())).await.expect("listed");
        assert!(listed.heard(&format!("role\t{everyone}\tSEND_MESSAGE\tdeny")), "{:?}", listed.said);
        assert!(!words_there().await.holds(Grant::SendMessage), "denied to the role, denied to its holder there");

        done(&server, ADA, set(Whom::Role(everyone.clone()), None)).await.expect("inherited");
        let listed = done(&server, ADA, Act::ChannelOverwrites(general.clone())).await.expect("listed");
        assert!(listed.heard("Every role's own words decide here."), "{:?}", listed.said);
        assert!(words_there().await.holds(Grant::SendMessage), "the role's own words again");

        let members = done(&server, ADA, Act::MemberList).await.expect("listed");
        let grace_id = id_of(&line_of(&members, 1, GRACE));
        done(&server, ADA, set(Whom::Member(grace_id.clone()), Some(Overwrite::Deny))).await.expect("denied");
        let listed = done(&server, ADA, Act::ChannelOverwrites(general.clone())).await.expect("listed");
        assert!(listed.heard(&format!("member\t{grace_id}\tSEND_MESSAGE\tdeny")), "{:?}", listed.said);
        assert!(!words_there().await.holds(Grant::SendMessage), "denied to the member alone");
        let inherited = done(&server, ADA, set(Whom::Member(grace_id), None)).await.expect("inherited");
        assert!(inherited.heard("The member's roles decide in this channel now."), "{:?}", inherited.said);
        assert!(words_there().await.holds(Grant::SendMessage), "the roles' words again");

        let kept = Some(Access { kept: ChannelAccess::Private, role: None });
        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        let lobby = id_of(&line_of(&listed, 2, testserver::CATEGORY));
        let made = Act::ChannelCreate { voice: false, name: "notes".into(), category: lobby, access: kept };
        done(&server, ADA, made).await.expect("made private");
        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        assert!(listed.said.iter().any(|line| line.split('\t').nth(2) == Some("notes")), "the maker sees it");
        let seen = grace.link.ask(ChannelsList {}).await.expect("listed");
        assert!(seen.channels.iter().all(|channel| channel.name != "notes"), "hidden from the default role");
    }

    /// Categories made, a channel moved into one and a category moved, as `channels` prints them; a move that names a
    /// list the server has since changed moves nothing and says so, and a deleted category takes its channels with it.
    #[tokio::test]
    async fn a_channel_moves_into_a_category_and_a_stale_move_moves_nothing() {
        let (server, _) = owned("categories", 0).await;
        done(&server, ADA, Act::CategoryCreate("Games".into())).await.expect("made");
        done(&server, ADA, Act::CategoryCreate("Talk".into())).await.expect("made");
        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        let (games, talk) = (id_of(&line_of(&listed, 2, "Games")), id_of(&line_of(&listed, 2, "Talk")));
        let general = id_of(&line_of(&listed, 2, testserver::GENERAL));
        let moved =
            done(&server, ADA, Act::ChannelMove { channel_id: general.clone(), category: talk.clone(), before: None });
        assert!(moved.await.expect("moved").heard("Channel moved."));
        done(&server, ADA, Act::CategoryMove { category_id: talk.clone(), before: Some(games.clone()) })
            .await
            .expect("moved");

        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        let at = |name: &str| listed.said.iter().position(|line| line.split('\t').nth(2) == Some(name)).expect(name);
        assert!(at(testserver::BOOTH) < at("Talk"), "the first category's channel first: {:?}", listed.said);
        assert!(at("Talk") < at(testserver::GENERAL) && at(testserver::GENERAL) < at("Games"), "{:?}", listed.said);
        assert!(line_of(&listed, 2, "Talk").contains("\tcategory\t"));

        let (ada, _events) =
            Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a socket");
        ada.sign_in_with_password(ADA, &Secret::new(PASSWORD.into())).await.expect("signed in");
        let stale = ada.ask(ChannelsList {}).await.expect("listed").version;
        done(&server, ADA, Act::CategoryRename { category_id: games.clone(), name: "Play".into() })
            .await
            .expect("renamed");
        let moved = proto::chatkord_v1::ChannelMove {
            channel_id: general.to_string(),
            category_id: games.to_string(),
            before: None,
            version: stale,
        };
        let refused = ada.ask(moved).await;
        assert!(matches!(refused, Err(LinkError::Refused(ErrorCode::ListChanged))), "{refused:?}");
        let said = refused.map_err(|e| words::Words::english().said(&words::failure::said(&e)));
        assert_eq!(said, Err("The list changed meanwhile; try again.".to_string()));
        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        let at = |name: &str| listed.said.iter().position(|line| line.split('\t').nth(2) == Some(name)).expect(name);
        assert!(at("Talk") < at(testserver::GENERAL) && at(testserver::GENERAL) < at("Play"), "nothing moved");

        done(&server, ADA, Act::CategoryDelete(talk)).await.expect("deleted, #general with it");
        let listed = done(&server, ADA, Act::Channels).await.expect("listed");
        let at = |name: &str| listed.said.iter().position(|line| line.split('\t').nth(2) == Some(name));
        assert_eq!(at("Talk"), None, "{:?}", listed.said);
        assert_eq!(at(testserver::GENERAL), None, "its channels go with it: {:?}", listed.said);
        assert!(
            at(testserver::BOOTH).is_some_and(|booth| at("Play").is_some_and(|play| booth < play)),
            "{:?}",
            listed.said
        );
        done(&server, ADA, Act::CategoryDelete(games)).await.expect("an empty one is deleted");
    }

    /// A kick ends a member's sessions and lets them back; a ban, proved by the asker's password, ends them on
    /// `BANNED` and keeps them out, said in words, until an unban.
    #[tokio::test]
    async fn a_kicked_member_signs_in_again_and_a_banned_one_only_once_unbanned() {
        let (server, invites) = owned("ban", 1).await;
        register(&server, GRACE, Box::leak(invites[0].clone().into_boxed_str()), "Grace").await;
        let members = done(&server, ADA, Act::MemberList).await.expect("listed");
        let grace_id = id_of(&line_of(&members, 1, GRACE));
        let signed_in = || async {
            let (link, events) =
                Link::connect(&url(&server), &Trust::system(), "cli-test", "test").await.expect("a socket");
            let signed = link.sign_in_with_password(GRACE, &Secret::new(PASSWORD.into())).await;
            (link, events, signed)
        };

        let (_grace, mut events, signed) = signed_in().await;
        signed.expect("Grace signs in");
        done(&server, ADA, Act::MemberKick(grace_id.clone())).await.expect("kicked");
        assert_eq!(ended(&mut events).await.and_then(|ended| ended.reason), Some(CloseReason::Kicked));
        let (_grace, mut events, signed) = signed_in().await;
        signed.expect("a kicked member signs in again");

        let ban = |banned| Act::MemberBan { user_id: grace_id.clone(), banned };
        let mut wrong = Scripted::answering(&[(Ask::Password, "not the password this account has")]);
        let refused = run(on(&server, ADA, ban(true)), &mut wrong, &Platform::default(), &Trust::system()).await;
        assert_eq!(refused, Err("That is not this account's password; nothing changed.".to_string()));
        let mut person = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, ban(true)), &mut person, &Platform::default(), &Trust::system()).await.expect("banned");
        assert_eq!(person.asked, vec![Ask::Password], "signed in on the saved token, so the password is asked");
        assert_eq!(ended(&mut events).await.and_then(|ended| ended.reason), Some(CloseReason::Banned));
        let (_grace, mut events, signed) = signed_in().await;
        assert!(signed.is_err(), "a banned member signed in");
        let gone = ended(&mut events).await.expect("the refusal ends the socket");
        assert_eq!(gone.reason, Some(CloseReason::Banned), "told why, its password proved");
        let said = words::Words::english().said(&words::failure::ended(&gone));
        assert_eq!(said, "An admin banned this account from this server.");
        let listed = done(&server, ADA, Act::MemberList).await.expect("listed");
        assert!(line_of(&listed, 1, GRACE).ends_with("\tbanned"), "the list says who is banned");
        let mut typing = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        let refused = run(on(&server, GRACE, Act::SignIn), &mut typing, &Platform::default(), &Trust::system()).await;
        let banned = "An admin banned this account from this server.".to_string();
        assert_eq!(refused, Err(banned), "a sign-in at the terminal says why, not that the password failed");

        let mut person = Scripted::answering(&[(Ask::Password, PASSWORD)]);
        run(on(&server, ADA, ban(false)), &mut person, &Platform::default(), &Trust::system()).await.expect("unbanned");
        let (_grace, _events, signed) = signed_in().await;
        signed.expect("unbanned, she signs in");
    }
}
