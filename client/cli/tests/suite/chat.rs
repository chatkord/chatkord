//! Two members of one server, each on a socket of their own, as two windows are: who the other is, and each other's
//! messages, told live.

use crate::common;

use common::{PASSWORD, Server};
use link::{Events, Link, Trust};
use proto::chatkord_v1::{CatchUp, ChannelKind, ChannelsList, DeleteMessage, Event, History, ListPeople, event};
use std::time::Duration;
use terms::{Heard, Nonce, Secret, ServerUrl};

const ADA: &str = "ada";
const GRACE: &str = "grace";

async fn member(server: &Server, identity: &str, invite: &str) -> (Link, Events) {
    let url = ServerUrl::parse(&server.url()).expect("a loopback address");
    let (link, events) = Link::connect(&url, &Trust::system(), "chat-test", "test").await.expect("a socket");
    let password = Secret::new(PASSWORD.into());
    link.register(identity, Some(identity), &password, &Secret::new(invite.into())).await.expect("registered");
    (link, events)
}

/// The next event of the kind `pick` takes, passing over the others, within a few seconds.
async fn told<T>(events: &mut Events, pick: impl Fn(&event::Kind) -> Option<T>) -> T {
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match events.next().await {
                Some(Heard::Event(Event { kind: Some(kind), .. })) => {
                    if let Some(found) = pick(&kind) {
                        return found;
                    }
                }
                Some(_) => {}
                None => panic!("the connection ended"),
            }
        }
    });
    waited.await.expect("told in time")
}

mod server {
    use super::*;

    #[tokio::test]
    async fn two_members_see_each_other_and_each_other_s_messages_live() {
        let (server, invites) = Server::start("chat", 2).await;
        let (ada, mut ada_heard) = member(&server, ADA, &invites[0]).await;
        let (grace, _grace_heard) = member(&server, GRACE, &invites[1]).await;

        let arrived = told(&mut ada_heard, |kind| match kind {
            event::Kind::PersonArrived(arrived) => arrived.person.clone(),
            _ => None,
        })
        .await;
        assert_eq!(arrived.display_name, GRACE, "a new member is told to the others");
        let people = grace.ask(ListPeople { after: None }).await.expect("listed");
        assert!(people.entries.iter().any(|person| person.display_name == ADA), "{people:?}");

        let listed = grace.ask(ChannelsList {}).await.expect("listed");
        let general = listed
            .channels
            .iter()
            .find(|channel| channel.kind == i32::from(ChannelKind::Text) && channel.name == testserver::GENERAL)
            .and_then(|channel| terms::TimelineId::parse(&channel.timeline_id))
            .expect("the test server's text channel, by its timeline");

        let sent = grace.send_with_preview(plain(&general, "anyone up for a game?"), None).await.expect("sent");
        let heard = told(&mut ada_heard, |kind| match kind {
            event::Kind::MessageChanged(changed) => changed.message.clone(),
            _ => None,
        })
        .await;
        assert_eq!(
            (heard.message_id.as_str(), heard.body.as_deref()),
            (sent.message_id.as_str(), Some("anyone up for a game?"))
        );
        let again = grace.send_with_preview(plain(&general, "anyone up for a game?"), None).await.expect("answered");
        assert_eq!(again.message_id, sent.message_id, "a resend after a lost reply is the same send");

        let edited = grace
            .edit_with_preview(
                &terms::MessageId::parse(&sent.message_id).unwrap(),
                "anyone up for a game later?",
                &[],
                None,
            )
            .await
            .expect("edited");
        assert!(
            ada.edit_with_preview(&terms::MessageId::parse(&sent.message_id).unwrap(), "not mine", &[], None)
                .await
                .is_err(),
            "only the author edits"
        );
        let page = ada
            .ask(History { timeline_id: general.as_str().to_owned(), anchor: None, limit: 0 })
            .await
            .expect("a page");
        let newest = page.messages.last().expect("a message");
        assert_eq!(newest.body.as_deref(), Some("anyone up for a game later?"));

        grace.ask(DeleteMessage { message_id: sent.message_id.clone() }).await.expect("deleted");
        let caught = ada
            .ask(CatchUp { timeline_id: general.as_str().to_owned(), since: edited.revision })
            .await
            .expect("caught up");
        assert!(
            caught.changed.iter().any(|message| message.body.is_none()),
            "the tombstone is in the catch-up: {caught:?}"
        );
    }
}

/// A message of text alone, the same nonce every time so a resend is the same send.
fn plain<'a>(timeline: &'a terms::TimelineId, body: &'a str) -> link::Sending<'a> {
    link::Sending { timeline, thread_of: None, body, spans: &[], files: &[], reply: None, nonce: Nonce::new([1; 16]) }
}
