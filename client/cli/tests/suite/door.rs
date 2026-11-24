//! The door driven by the code the window uses to carry out its errands, against a real server process and an
//! in-memory keyring: an account made with an invite link, a sign-in by address with a wrong password first, a
//! remembered account opening by itself, one whose session ended saying how, an address nothing answers at, and the
//! name others see set and heard.

use crate::common;

use common::{PASSWORD, Server};
use errands::{Session, carry_out};
use link::Trust;
use platform::Platform;
use proto::chatkord_v1::CloseReason;
use screens::{Arrival, Door, DoorAsk, Message, Opening, Problem, Step};
use std::sync::Once;
use terms::{Secret, ServerUrl};
use words::Sentence;

const ADA: &str = "Ada";
const BUILD: &str = "test";
const CHANGED: &str = "another correct phrase entirely";

fn in_memory_keyring() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a store")));
}

fn secret(text: &str) -> Secret {
    Secret::new(text.to_string())
}

/// Gives the door a message and carries out whatever it then asks, until it waits for a person again; returns the
/// signed-in connection if there is one.
async fn drive(door: &mut Door, first: Message) -> Option<Session> {
    let mut todo: Vec<DoorAsk> = door.update(first);
    let mut signed_in = None;
    while !todo.is_empty() {
        let mut next = Vec::new();
        for each in todo {
            let DoorAsk::Go(errand) = each else { panic!("a host's own job in a test with no window: {each:?}") };
            let carried = carry_out(&Platform::default(), errand, &Trust::system(), BUILD).await;
            signed_in = carried.session.or(signed_in);
            if let Some(heard) = carried.heard {
                next.extend(door.update(Message::Reported(heard)));
            }
        }
        todo = next;
    }
    signed_in
}

fn typed(door: &mut Door, username: &str, password: &str) {
    door.update(Message::Username(username.into()));
    door.update(Message::Password(secret(password)));
    door.update(Message::Again(secret(password)));
}

/// A server and an account on it, made through the door with the link its invite makes.
async fn joined(tag: &str) -> (Server, Session) {
    in_memory_keyring();
    let (server, invites) = Server::start(tag, 1).await;
    let link = proto::envelope::join_link(&server.host(), &invites[0]);
    let mut door = Door::default();
    door.update(Message::Where(link));
    assert!(drive(&mut door, Message::Submit).await.is_none());
    assert_eq!(door.step(), Step::Preview, "{:?}", door.problem());
    assert!(door.can_create(), "the invite works");
    door.update(Message::Submit);
    typed(&mut door, ADA, PASSWORD);
    let session = drive(&mut door, Message::Submit).await.expect("an account made and signed in");
    assert!(door.signed_in(), "{:?}", door.problem());
    assert_eq!(door.arrival(), Some(Arrival::Joined { name: ADA.into() }));
    assert_eq!(session.ack.identity, ADA, "the capital is kept");
    (server, session)
}

fn remembered(server: &Server) -> Opening {
    let url = ServerUrl::parse(&server.url()).unwrap();
    Opening::Account(screens::Server { url, name: None, colour: None, picture: None }, ADA.into())
}

mod server {
    use super::*;

    #[tokio::test]
    async fn an_invite_link_makes_an_account_and_the_next_run_opens_by_itself() {
        let (server, _) = joined("door-join").await;
        let mut again = Door::opening(remembered(&server));
        let session = drive(&mut again, Message::Begin).await.expect("signed in on the saved token");
        assert!(again.signed_in() && again.arrival().is_none(), "a remembered account opens with no note");
        assert_eq!(session.ack.identity, ADA);
    }

    #[tokio::test]
    async fn an_address_signs_in_and_a_wrong_password_is_said_and_cleared() {
        let (server, _) = joined("door-sign-in").await;
        let mut door = Door::default();
        door.update(Message::Where(server.host()));
        drive(&mut door, Message::Submit).await;
        assert_eq!(door.step(), Step::Preview);
        assert!(!door.can_create(), "an address is for an account that exists");
        door.update(Message::Submit);
        assert_eq!(door.step(), Step::SignIn);
        door.update(Message::Username("ada".into()));
        door.update(Message::Password(secret("not the password at all")));
        assert!(drive(&mut door, Message::Submit).await.is_none());
        assert_eq!(door.problem(), Some(&Problem::Wrong { server: server.host() }));
        assert!(door.password().is_empty() && door.username() == "ada");
        door.update(Message::Password(secret(PASSWORD)));
        assert!(drive(&mut door, Message::Submit).await.is_some(), "the username is folded, the password right");
        assert!(door.signed_in());
    }

    #[tokio::test]
    async fn a_remembered_account_whose_sessions_ended_says_how_and_asks_for_the_password() {
        // The reason the server closed with reaches the person, not "expired" whatever it was.
        let (server, session) = joined("door-ended").await;
        // Another device changes the password: the one that asks stays, so the saved token is the one that ends.
        let url = ServerUrl::parse(&server.url()).unwrap();
        let elsewhere =
            errands::signin::with_password(&Platform::default(), &url, ADA, &secret(PASSWORD), &Trust::system(), BUILD)
                .await
                .map_err(|_| ())
                .expect("a second device signs in");
        elsewhere
            .link
            .ask(proto::chatkord_v1::ChangePassword { password: CHANGED.into(), current: PASSWORD.into() })
            .await
            .unwrap();
        drop(session);
        let mut door = Door::opening(remembered(&server));
        assert!(drive(&mut door, Message::Begin).await.is_none());
        assert_eq!(door.step(), Step::SignIn);
        assert_eq!(
            door.why(),
            Some(&Sentence::Ended { server: server.host(), reason: Some(CloseReason::PasswordChanged) })
        );
        door.update(Message::Password(secret(CHANGED)));
        assert!(drive(&mut door, Message::Submit).await.is_some());
    }

    #[tokio::test]
    async fn an_address_nothing_answers_at_is_said_on_the_first_step() {
        in_memory_keyring();
        let mut door = Door::default();
        door.update(Message::Where("127.0.0.1:1".into()));
        assert!(drive(&mut door, Message::Submit).await.is_none());
        assert_eq!(door.step(), Step::Add);
        assert_eq!(door.problem(), Some(&Problem::NotFound { host: "127.0.0.1:1".into() }));
        let mut plain = Door::default();
        plain.update(Message::Where("ws://example.invalid/v1".into()));
        assert!(plain.update(Message::Submit).is_empty(), "a password never goes in the open to another machine");
        assert!(matches!(plain.problem(), Some(Problem::Untrusted { .. })));
    }

    #[tokio::test]
    async fn the_name_others_see_is_set_heard_by_every_session_and_read_at_the_next_sign_in() {
        use proto::chatkord_v1::{ErrorCode, event};
        use terms::Heard;
        let (server, mut session) = joined("door-renamed").await;
        session.link.ask(proto::chatkord_v1::SetDisplayName { display_name: Some("Countess".into()) }).await.unwrap();
        let heard = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                match session.events.next().await {
                    Some(Heard::Event(event)) => match event.kind {
                        Some(event::Kind::MemberRenamed(renamed)) => break renamed,
                        _ => continue,
                    },
                    other => panic!("the link ended: {other:?}"),
                }
            }
        })
        .await
        .expect("told within 5 s");
        assert_eq!((heard.user_id, heard.display_name.as_str()), (session.ack.user_id.clone(), "Countess"));
        let url = ServerUrl::parse(&server.url()).unwrap();
        let again =
            errands::signin::with_password(&Platform::default(), &url, ADA, &secret(PASSWORD), &Trust::system(), BUILD)
                .await
                .map_err(|_| ())
                .expect("signs in again");
        assert_eq!(errands::signin::standing(&again.ack).expect("an account id").shown.as_deref(), Some("Countess"));
        let too_long = "n".repeat(proto::rules::MAX_NAME_CHARS + 1);
        let refused = session.link.ask(proto::chatkord_v1::SetDisplayName { display_name: Some(too_long) }).await;
        assert!(matches!(refused, Err(terms::LinkError::Refused(ErrorCode::InvalidRequest))));
        session.link.ask(proto::chatkord_v1::SetDisplayName { display_name: None }).await.unwrap();
        let back =
            errands::signin::with_password(&Platform::default(), &url, ADA, &secret(PASSWORD), &Trust::system(), BUILD)
                .await
                .map_err(|_| ())
                .expect("signs in again");
        assert_eq!(back.ack.display_name, ADA, "none goes back to the username");
    }

    /// A person's own colour, against the real server: offered in the greeting, heard by every session, read into the
    /// next sign-in's standing, and cleared with none.
    #[tokio::test]
    async fn a_colour_picked_is_heard_by_every_session_and_read_at_the_next_sign_in() {
        use proto::chatkord_v1::event;
        use proto::rules::Colour;
        use terms::Heard;
        let (server, mut session) = joined("door-recoloured").await;
        let blue = Colour::parse("#3d7fd6").ok();
        session
            .link
            .ask(proto::chatkord_v1::SetProfileColour { colour: blue.map(|blue| blue.to_string()) })
            .await
            .unwrap();
        let heard = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                match session.events.next().await {
                    Some(Heard::Event(event)) => match event.kind {
                        Some(event::Kind::MemberRecoloured(recoloured)) => break recoloured,
                        _ => continue,
                    },
                    other => panic!("the link ended: {other:?}"),
                }
            }
        })
        .await
        .expect("told within 5 s");
        assert_eq!((heard.user_id, heard.colour.as_deref()), (session.ack.user_id.clone(), Some("#3D7FD6")));
        let url = ServerUrl::parse(&server.url()).unwrap();
        let again =
            errands::signin::with_password(&Platform::default(), &url, ADA, &secret(PASSWORD), &Trust::system(), BUILD)
                .await
                .map_err(|_| ())
                .expect("signs in again");
        assert_eq!(errands::signin::standing(&again.ack).expect("an account id").colour, blue);
        session.link.ask(proto::chatkord_v1::SetProfileColour { colour: None }).await.unwrap();
        let back =
            errands::signin::with_password(&Platform::default(), &url, ADA, &secret(PASSWORD), &Trust::system(), BUILD)
                .await
                .map_err(|_| ())
                .expect("signs in again");
        assert_eq!(errands::signin::standing(&back.ack).expect("an account id").colour, None, "none is none again");
    }

    /// The server's name and colour, against the real server: refused to a member, set by one holding `MANAGE_SERVER`, told to
    /// every session, and stated in the next greeting, which remembers it beside the address.
    #[tokio::test]
    async fn an_admin_names_and_colours_the_server_and_the_next_greeting_says_so() {
        use proto::chatkord_v1::{ErrorCode, event};
        use proto::rules::Colour;
        use terms::Heard;
        let (server, mut session) = joined("door-server-identity").await;
        let refused = session.link.ask(proto::chatkord_v1::SetServerName { name: Some("Server S".into()) }).await;
        assert!(matches!(refused, Err(terms::LinkError::Refused(ErrorCode::NotAuthorized))));
        server.give_only("MANAGE_SERVER", "moderators", "ada");
        session.link.ask(proto::chatkord_v1::SetServerName { name: Some("Server S".into()) }).await.unwrap();
        let blue = Colour::parse("#3d7fd6").ok();
        session
            .link
            .ask(proto::chatkord_v1::SetServerColour { colour: blue.map(|blue| blue.to_string()) })
            .await
            .unwrap();
        let told = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut told = Vec::new();
            while told.len() < 2 {
                match session.events.next().await {
                    Some(Heard::Event(event)) => {
                        if let Some(event::Kind::ServerChanged(changed)) = event.kind {
                            told.push(terms::ServerIdentity::read(
                                changed.name.as_deref(),
                                changed.colour.as_deref(),
                                changed.picture.as_deref(),
                            ));
                        }
                    }
                    other => panic!("the link ended: {other:?}"),
                }
            }
            told
        })
        .await
        .expect("told within 5 s");
        let named = terms::ServerIdentity { name: Some("Server S".into()), colour: None, picture: None };
        assert_eq!(
            told,
            [Some(named), Some(terms::ServerIdentity { name: Some("Server S".into()), colour: blue, picture: None })]
        );
        let url = ServerUrl::parse(&server.url()).unwrap();
        let (again, _) = errands::signin::connect(&Platform::default(), &url, &Trust::system(), BUILD).await.unwrap();
        assert_eq!(
            again.greeted(),
            &terms::ServerIdentity { name: Some("Server S".into()), colour: blue, picture: None }
        );
    }
}
