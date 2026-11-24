//! The room the window shows, from the real wire: a sender heard while the tone plays, muted when it stops and gone
//! when they leave, each from the server's events and the engine's loudness, never a timer; a new channel too.

use crate::common;

use call::room::{Leaving, Report, run};
use common::{Server, Shared, WAIT, engine, join_voice, registered, until, voice_of, within};
use proto::chatkord_v1::{ChannelKind, ChannelsList, VoiceLeave, event};
use screens::{Health, Quality, Room};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Runs the window's loop as Bea in the voice channel her registration joins, collecting what the connection reports,
/// until `holds` is true of one of the reports or ten seconds pass; then leaves.
async fn health_until(
    server: &Server,
    name: &str,
    invite: &str,
    with: impl AsyncFnOnce(&link::Link),
    holds: impl Fn(&Health) -> bool,
) -> Option<Health> {
    let (bea, mut bea_events, booth) = registered(&server.url(), name, invite).await;
    with(&bea).await;
    let seen = Shared::new(Vec::<Health>::new());
    let into = seen.clone();
    let (_commands, queued) = mpsc::channel(8);
    let stop = Leaving::default();
    let listening = run(
        &bea,
        engine(),
        &mut bea_events,
        common::listening(booth, Duration::from_millis(100)),
        (tokio::sync::watch::channel(call::room::Wants::default()).1, queued),
        stop.clone(),
        move |report| {
            if let Report::Connection(health) = report {
                into.change(|seen| seen.push(health));
            }
        },
    );
    let watching = async {
        let found = seen.wait(Duration::from_secs(10), |seen| seen.iter().copied().find(|health| holds(health))).await;
        stop.leave();
        found
    };
    let (ended, found) = tokio::join!(listening, watching);
    ended.expect("the loop ended when it was told to");
    found
}

mod server {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_room_shows_someone_heard_then_muted_then_gone_from_the_servers_events() {
        let (server, invites) = Server::start("e2e_room", 2).await;
        // Ada joins and sends a tone; Bea registers and then joins by running the window's loop.
        let (ada, _ada_events, joined) = within("Ada joins", join_voice(&server.url(), "ada", &invites[0])).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let speaking = within("Ada's tone starts", ada_voice.speak_tone(&ada)).await.expect("ada speaks");
        let (bea, mut bea_events, booth) = within("Bea registers", registered(&server.url(), "bea", &invites[1])).await;

        let room = Shared::new(Room::new(None));
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let (_commands, queued) = mpsc::channel(8);
        let stop = Leaving::default();
        let channels = Shared::new(Vec::<String>::new());
        let (into_room, into_log, into_channels) = (room.clone(), said.clone(), channels.clone());
        let listening = run(
            &bea,
            engine(),
            &mut bea_events,
            common::listening(booth, Duration::from_millis(100)),
            (tokio::sync::watch::channel(call::room::Wants::default()).1, queued),
            stop.clone(),
            move |report| match report {
                Report::Room(event) => drop(into_room.change(|room| room.on(event, Instant::now()))),
                Report::Said(line) => into_log.lock().unwrap().push(line),
                Report::Left(Ok(())) => {}
                Report::Left(Err(said)) | Report::Notice(_, said) => {
                    into_log.lock().unwrap().push(words::Words::english().said(&said))
                }
                Report::Heard(heard) => {
                    if let Some(event::Kind::ChannelAdded(added)) = heard.kind {
                        into_channels.change(|names| names.extend(added.channel.map(|channel| channel.name)));
                    }
                }
                Report::Behind
                | Report::Answered(_)
                | Report::Connection(_)
                | Report::Sound(_)
                | Report::Share(_)
                | Report::Watch(_)
                | Report::Camera(_)
                | Report::OwnWatched { .. } => {}
            },
        );
        let watching = async {
            until(&room, "both people in the room", |tiles| {
                tiles.iter().any(|t| t.name == "ADA") && tiles.iter().any(|t| t.name == "BEA")
            })
            .await;
            until(&room, "Ada heard while her tone plays", |tiles| {
                tiles.iter().any(|t| t.name == "ADA" && t.speaking && !t.muted)
            })
            .await;
            assert!(
                !room.lock().unwrap().tiles(Instant::now()).iter().any(|t| t.name == "BEA" && t.speaking),
                "Bea sends nothing"
            );

            // Her tone ends, the stream closed rather than muted (a muted track keeps sending): the server says so,
            // and the model shows her muted and not heard.
            within("Ada's tone stops", speaking.stop(&ada)).await.unwrap();
            until(&room, "Ada muted after her stream closed", |tiles| {
                tiles.iter().any(|t| t.name == "ADA" && t.muted && !t.speaking)
            })
            .await;

            // She leaves: the server says so and her tile goes.
            within("Ada leaves", ada.ask(VoiceLeave {})).await.unwrap();
            until(&room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;

            // Someone who may manage channels makes one: the loop that holds the connection passes the news on.
            let (cy, _cy_events, _) =
                within("Cy registers", registered(&server.url(), "cy", &server.owner_code())).await;
            let listed = within("Cy lists the channels", cy.ask(ChannelsList {})).await.unwrap();
            let category = listed.categories.into_iter().next().expect("a category").category_id;
            let create = proto::chatkord_v1::ChannelCreate {
                kind: ChannelKind::Voice.into(),
                name: "Movie night".into(),
                category_id: category,
                access: None,
                role_id: None,
            };
            let made = cy.ask(create);
            within("Cy makes a channel", made).await.expect("made");
            let heard = channels.wait(WAIT, |names| names.iter().any(|name| name == "Movie night").then_some(())).await;
            assert!(heard.is_some(), "the call never passed on the new channel: {:?}", channels.lock().unwrap());
            stop.leave();
        };
        tokio::pin!(listening);
        // The loop runs the whole test; once told to leave, it ends within the bound.
        let ended = tokio::select! {
            _ = &mut listening => panic!("the listen loop ended before it was told to leave"),
            () = watching => within("the listen loop ends once told to leave", &mut listening).await,
        };
        ended.expect("the listen loop ended when it was told to");
        assert!(room.lock().unwrap().ended().is_none(), "the room did not end: this client chose to stop");
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_that_sends_and_hears_nothing_still_has_a_round_trip_and_a_verdict_from_the_link() {
        let (server, invites) = Server::start("e2e_health_alone", 1).await;
        let health =
            health_until(&server, "bea", &invites[0], async |_| {}, |health| health.round_trip.is_some()).await;
        let health = health.expect("no round trip in ten seconds: the engine has none on a call with no media");
        assert_eq!(health.quality, Some(Quality::Good), "loopback answers at once");
        assert_eq!((health.sending.packets, health.receiving.packets, health.lost), (0, 0, 0));
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn what_a_tone_sender_sends_is_a_rate_the_listener_sees() {
        let (server, invites) = Server::start("e2e_health_rate", 2).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let speaking = voice_of(&joined).0.speak_tone(&ada).await.expect("ada speaks");
        let health = health_until(
            &server,
            "bea",
            &invites[1],
            async |_| {},
            |health| health.receiving.bits_per_second > 0 && health.receiving.packets > 0,
        )
        .await;
        let health = health.expect("no rate in ten seconds from a tone that plays");
        assert!(health.down_bytes > 0, "the transports carried it");
        assert_eq!(health.quality, Some(Quality::Good), "nothing is lost over loopback");
        speaking.stop(&ada).await.unwrap();
        drop(server);
    }
}
