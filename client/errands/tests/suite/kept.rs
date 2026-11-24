//! A kept connection's listener, against the real server: what its events say is told as it comes, not when someone
//! looks; a listener whose `Kept` was replaced tells nothing more; and lent events are the borrower's alone.

use errands::{Kept, Session};
use link::{Events, Link, Trust};
use proto::chatkord_v1::event;
use std::time::Duration;
use terms::{Heard, Secret, ServerUrl};
use testserver::Server;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};

const PASSWORD: &str = "a test password that is long enough";
const ADA: &str = "ada";
const BEA: &str = "bea";
const WAIT: Duration = Duration::from_secs(10);

async fn registered(url: &str, identity: &str, invite: &str) -> Session {
    let url = ServerUrl::parse(url).expect("a loopback address");
    let (link, events) = Link::connect(&url, &Trust::system(), "kept", "test").await.expect("connected");
    let (password, invite) = (Secret::new(PASSWORD.into()), Secret::new(invite.into()));
    let ack = link.register(identity, None, &password, &invite).await.expect("registered");
    Session { link, events, ack }
}

/// A kept session whose listener tells the names others take, and its end, into a channel.
fn kept(session: Session) -> (Kept, mpsc::UnboundedReceiver<Heard>) {
    let (into, told) = mpsc::unbounded_channel();
    let kept = Kept::listen(session, move |heard: Heard| {
        let into = into.clone();
        async move {
            let wanted = match &heard {
                Heard::Event(event) => matches!(event.kind, Some(event::Kind::MemberRenamed(_))),
                Heard::Behind | Heard::Ended(_) => true,
            };
            if wanted {
                let _ = into.send(heard);
            }
        }
    });
    (kept, told)
}

fn renamed_to(heard: &Heard) -> Option<String> {
    match heard {
        Heard::Event(event) => match &event.kind {
            Some(event::Kind::MemberRenamed(renamed)) => Some(renamed.display_name.clone()),
            _ => None,
        },
        Heard::Behind | Heard::Ended(_) => None,
    }
}

/// The next rename `events` read, within `WAIT`.
async fn next_rename(events: &mut Events) -> String {
    timeout(WAIT, async {
        loop {
            if let Some(name) = renamed_to(&events.next().await.expect("an event")) {
                return name;
            }
        }
    })
    .await
    .expect("a rename read")
}

mod server {
    use super::*;

    #[tokio::test]
    async fn an_event_is_told_as_it_comes_and_a_replaced_listener_tells_nothing_more() {
        let (server, invites) = Server::start("errands_kept", 3).await;
        let (old, mut old_told) = kept(registered(&server.url(), ADA, &invites[0]).await);
        let bea = registered(&server.url(), BEA, &invites[1]).await;

        let asked = Instant::now();
        bea.link
            .ask(proto::chatkord_v1::SetDisplayName { display_name: Some("Bea one".into()) })
            .await
            .expect("renamed");
        let heard = timeout(WAIT, old_told.recv()).await.expect("told").expect("a rename");
        let took = asked.elapsed();
        assert_eq!(renamed_to(&heard).as_deref(), Some("Bea one"));
        assert!(took < Duration::from_millis(100), "told {took:?} after the rename");

        // The same account signed in again: the newer connection is kept, and the older one's listener goes with it.
        let again = registered(&server.url(), "ada2", &invites[2]).await;
        let (newer, mut newer_told) = kept(again);
        drop(old);
        bea.link
            .ask(proto::chatkord_v1::SetDisplayName { display_name: Some("Bea two".into()) })
            .await
            .expect("renamed");
        let heard = timeout(WAIT, newer_told.recv()).await.expect("told").expect("a rename");
        assert_eq!(renamed_to(&heard).as_deref(), Some("Bea two"));
        assert!(
            timeout(Duration::from_millis(500), old_told.recv()).await.is_ok_and(|told| told.is_none()),
            "the replaced listener told something more"
        );
        drop((newer, server));
    }

    #[tokio::test]
    async fn lent_events_are_the_borrowers_and_are_told_again_once_given_back() {
        let (server, invites) = Server::start("errands_kept_lent", 2).await;
        let (ada, mut told) = kept(registered(&server.url(), ADA, &invites[0]).await);
        let bea = registered(&server.url(), BEA, &invites[1]).await;

        let mut lent = ada.lend().await.expect("lent");
        bea.link
            .ask(proto::chatkord_v1::SetDisplayName { display_name: Some("Bea lent".into()) })
            .await
            .expect("renamed");
        assert_eq!(next_rename(&mut lent.events).await, "Bea lent", "the borrower reads it");
        lent.back.send(lent.events).ok().expect("given back");
        bea.link
            .ask(proto::chatkord_v1::SetDisplayName { display_name: Some("Bea back".into()) })
            .await
            .expect("renamed");
        let heard = timeout(WAIT, told.recv()).await.expect("told").expect("a rename");
        assert_eq!(renamed_to(&heard).as_deref(), Some("Bea back"), "nothing the borrower read is told again");
        drop((ada, server));
    }

    #[tokio::test]
    async fn an_end_while_lent_is_the_borrowers_to_tell() {
        let (server, invites) = Server::start("errands_kept_end", 1).await;
        let (ada, mut told) = kept(registered(&server.url(), ADA, &invites[0]).await);
        let lent = ada.lend().await.expect("lent");
        ada.link.ask(proto::chatkord_v1::SignOut {}).await.expect("signed out");
        let mut events = lent.events;
        let ended = timeout(WAIT, async {
            loop {
                if let Some(Heard::Ended(ended)) = events.next().await {
                    return ended;
                }
            }
        })
        .await
        .expect("the borrower read the end");
        drop(lent.back);
        assert!(
            timeout(Duration::from_millis(500), told.recv()).await.is_ok_and(|told| told.is_none()),
            "the listener told the end too"
        );
        assert!(ada.lend().await.is_none(), "the listener has ended");
        drop((ended, server));
    }

    #[tokio::test]
    async fn an_end_while_listening_is_told_once_and_ends_the_listener() {
        let (server, invites) = Server::start("errands_kept_told_end", 1).await;
        let (ada, mut told) = kept(registered(&server.url(), ADA, &invites[0]).await);
        ada.link.ask(proto::chatkord_v1::SignOut {}).await.expect("signed out");
        let heard = timeout(WAIT, told.recv()).await.expect("told").expect("the end");
        assert!(matches!(heard, Heard::Ended(_)), "{heard:?}");
        assert!(ada.lend().await.is_none(), "the listener has ended");
        drop(server);
    }
}
