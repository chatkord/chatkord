//! A call as a person is in it, from the real wire: what one client does is what the other's room shows, from the
//! server's own events, and a place that ends other than by a leave says why, in the server's own word.

use crate::common;

use call::room::{Command, Join, Leaving, Report, Source, Stopped, Wants, run};
use choices::State;
use common::{PASSWORD, Server, Shared, WAIT, join_voice, listening, registered, registered_as, told, until};
use futures_util::FutureExt;
use link::{Events, Link, Trust};
use proto::chatkord_v1::VoiceJoin;
use screens::{Room, RoomEvent, Why};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terms::{MemberId, Secret, ServerUrl};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// A call under way in a task of its own, with the room as it shows it.
struct Running {
    /// Kept so the connection outlives the call: the place must be given back by the call, not by a socket closing.
    _link: Link,
    /// What the person wants of the call, changed as the window would.
    wants: watch::Sender<Wants>,
    commands: mpsc::Sender<Command>,
    leaving: Leaving,
    room: Arc<Shared<Room>>,
    /// When each report of the levels and of the connection came.
    looked: Arc<Shared<Vec<(Instant, Looked)>>>,
    /// What the call said for an operator, and when.
    said: Arc<Mutex<Vec<(Instant, String)>>>,
    task: JoinHandle<Result<(), Stopped>>,
}

/// A report a call makes on its own clock, whatever the server does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Looked {
    Levels,
    Connection,
}

fn start(link: Link, events: Events, join: Join) -> Running {
    start_wanting(link, events, join, Wants::default())
}

/// A call that starts as `first` says: muted or deafened, and each member's volume.
fn start_wanting(link: Link, mut events: Events, join: Join, first: Wants) -> Running {
    let room = Shared::new(Room::new(join.me.clone()));
    let (commands, queued) = mpsc::channel(8);
    let (wants, wanted) = watch::channel(first);
    let leaving = Leaving::default();
    let left = leaving.clone();
    let into = Arc::clone(&room);
    let looked = Shared::new(Vec::new());
    let seen = Arc::clone(&looked);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let operator = Arc::clone(&lines);
    let kept = link.clone();
    let task = tokio::spawn(async move {
        run(&link, common::engine(), &mut events, join, (wanted, queued), left, move |report| match report {
            Report::Room(event) => {
                if let RoomEvent::Levels(_) = &event {
                    seen.change(|seen| seen.push((Instant::now(), Looked::Levels)));
                }
                into.change(|room| {
                    room.on(event, Instant::now());
                });
            }
            Report::Connection(_) => seen.change(|seen| seen.push((Instant::now(), Looked::Connection))),
            Report::Said(line) => operator.lock().unwrap().push((Instant::now(), line)),
            _ => {}
        })
        .await
    });
    Running { _link: kept, wants, commands, leaving, room, looked, said: lines, task }
}

impl Running {
    /// How many reports of `kind` came after `since`.
    fn looked_since(&self, kind: Looked, since: Instant) -> usize {
        self.looked.lock().unwrap().iter().filter(|(at, looked)| *looked == kind && *at > since).count()
    }

    /// Waits for the call's next `count` reports of `kind`: a window of `count` of its own beats, a look at the
    /// connection every `LOOK_EVERY` and the levels every `level_every`, in place of a time.
    async fn passes(&self, kind: Looked, count: usize) {
        let first = self.looked.lock().unwrap().iter().filter(|(_, looked)| *looked == kind).count();
        let within = WAIT + screens::LOOK_EVERY * u32::try_from(count).unwrap_or(u32::MAX);
        let seen = |looked: &Vec<(Instant, Looked)>| {
            (looked.iter().filter(|(_, looked)| *looked == kind).count() >= first + count).then_some(())
        };
        self.looked.wait(within, seen).await.expect("the call kept reporting");
    }

    async fn ended(self) -> (Result<(), Stopped>, Arc<Shared<Room>>) {
        let result = timeout(WAIT, self.task).await.expect("the call ended").expect("the task ran");
        (result, self.room)
    }
}

fn tone(channel_id: String) -> Join {
    Join { source: Source::Tone, ..listening(channel_id, Duration::from_millis(100)) }
}

mod server {
    use super::*;

    /// A server too old to name the account in `LoginAck` leaves the call unable to tell its own tile, so its own
    /// microphone is nobody's; it must not make a tile of nothing (an empty id, an empty name).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_that_does_not_know_its_own_id_adds_no_nameless_person() {
        let (server, invites) = Server::start("e2e_call_unnamed", 1).await;
        let (ada, ada_events, booth) = registered(&server.url(), "ada", &invites[0]).await;
        let ada_call = start(ada, ada_events, Join { me: None, ..tone(booth) });
        // Long enough for the engine to report the level of her own microphone, which would ring a tile of no name.
        until(&ada_call.room, "the room, with Ada in it", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;
        ada_call.passes(Looked::Connection, 3).await;
        let tiles = ada_call.room.lock().unwrap().tiles(Instant::now());
        assert_eq!(tiles.len(), 1, "one person in the room, not two: {tiles:?}");
        assert!(tiles.iter().all(|t| !t.name.is_empty() && t.user_id.is_some()), "{tiles:?}");
    }

    /// A server that announces its media by a name the machine resolves to both loopbacks, with a listener on one: the
    /// call reaches it by every address the name has, not by the one family the engine would have picked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_to_a_server_announced_as_localhost_connects() {
        let (server, invites) = Server::start_announcing("e2e_call_localhost", 1, "localhost").await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth) });
        until(&ada_call.room, "Ada's own tile connected over the name", |tiles| {
            tiles.iter().any(|t| t.own && t.connection == screens::Connection::Connected)
        })
        .await;
    }

    /// A server whose media is announced where nothing listens (IPv6's loopback, its listener on IPv4's): the call
    /// ends once its connection has stayed unconnected for the bound, saying where its media was sent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_whose_media_never_connects_ends_at_its_bound_saying_where_it_was_sent() {
        let (server, invites) = Server::start_announcing("e2e_call_unreached", 1, "::1").await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let begun = Instant::now();
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth) });
        let bound = Duration::from_secs(proto::envelope::MEDIA_CONNECT_SECONDS);
        let ended = timeout(bound + WAIT, ada_call.task).await.expect("the call ended").expect("the task ran");
        let took = begun.elapsed();
        assert!(
            matches!(&ended, Err(Stopped::Ended(screens::End::Failed(words::Failure::VoiceUnreached { host, .. }))) if host == "::1"),
            "{ended:?}"
        );
        assert!(took >= bound && took < bound + Duration::from_secs(3), "ended {took:?} after the join");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn what_a_member_does_about_hearing_and_being_heard_is_what_the_others_see_within_a_second() {
        let (server, invites) = Server::start("e2e_call", 2).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth.clone()) });
        let bea_call = start(bea, bea_events, listening(booth, Duration::from_millis(100)));

        // The server never announces a person's own stream to them: Ada's own tile comes first, sending, and rings from
        // the level of the microphone she sends.
        until(&ada_call.room, "Ada's own tile first, sending and heard", |tiles| {
            tiles.first().is_some_and(|own| own.own() && own.name == "ADA" && !own.muted && own.speaking)
        })
        .await;

        until(&bea_call.room, "Ada heard from her tone", |tiles| {
            tiles.iter().any(|t| t.name == "ADA" && t.speaking && !t.muted && !t.deafened)
        })
        .await;

        let asked = Instant::now();
        ada_call.wants.send_modify(|wants| wants.mute(true));
        until(&bea_call.room, "Ada muted", |tiles| tiles.iter().any(|t| t.name == "ADA" && t.muted && !t.deafened))
            .await;
        let took = asked.elapsed();
        assert!(took < Duration::from_secs(1), "Bea saw Ada muted after {took:?}");
        until(&bea_call.room, "Ada no longer heard while muted", |tiles| {
            tiles.iter().any(|t| t.name == "ADA" && !t.speaking)
        })
        .await;

        ada_call.wants.send_modify(|wants| wants.deafen(true));
        until(&bea_call.room, "Ada deafened", |tiles| tiles.iter().any(|t| t.name == "ADA" && t.deafened)).await;

        // Undoing both brings her back to be heard.
        ada_call.wants.send_modify(|wants| wants.deafen(false));
        ada_call.wants.send_modify(|wants| wants.mute(false));
        until(&bea_call.room, "Ada heard again", |tiles| {
            tiles.iter().any(|t| t.name == "ADA" && t.speaking && !t.muted && !t.deafened)
        })
        .await;

        // Leaving gives the place back: Bea sees her go, and the call ends as asked.
        ada_call.leaving.leave();
        let (left, ada_room) = ada_call.ended().await;
        left.expect("left as asked");
        assert!(ada_room.lock().unwrap().ended().is_none(), "a leave is not an ending to be told");
        until(&bea_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;
        bea_call.leaving.leave();
        bea_call.ended().await.0.expect("left as asked");
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_joined_muted_announces_no_stream_until_the_first_unmute() {
        let (server, invites) = Server::start("e2e_call_muted", 2).await;
        let (ada, ada_events, booth) = registered(&server.url(), "ada", &invites[0]).await;
        // Cy watches the wire from inside the channel: the streams the server announces are what a call sent.
        let (_cy, mut cy_events, _) = join_voice(&server.url(), "cy", &invites[1]).await;
        let muted = Wants::starting(State { mic_muted: true, deafened: false }, choices::Volumes::default());
        let ada_call = start_wanting(ada, ada_events, tone(booth), muted);

        let announced = |events: &mut link::Events| {
            let mut seen = false;
            // What is already queued, without waiting.
            while let Some(Some(heard)) = events.next().now_or_never() {
                if let terms::Heard::Event(event) = heard
                    && matches!(event.kind, Some(proto::chatkord_v1::event::Kind::ProducerJoined(_)))
                {
                    seen = true;
                }
            }
            seen
        };
        until(&ada_call.room, "Ada in the room", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;
        ada_call.passes(Looked::Connection, 2).await;
        assert!(!announced(&mut cy_events), "a call joined muted announced a stream");

        ada_call.wants.send_modify(|wants| wants.mute(false));
        let found = timeout(
            WAIT,
            told(&mut cy_events, |kind| matches!(kind, proto::chatkord_v1::event::Kind::ProducerJoined(_))),
        )
        .await;
        assert!(found.is_ok(), "no stream was announced after the first unmute");
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_leave_behind_a_full_command_queue_still_leaves() {
        let (server, invites) = Server::start("e2e_call_full_leave", 2).await;
        let (ada, mut ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let bea_call = start(bea, bea_events, listening(booth.clone(), Duration::from_millis(100)));
        const QUEUE: usize = 8;
        // Filled before the call reads any of it, so a leave sent as one more command would be dropped.
        let (commands, queued) = mpsc::channel(QUEUE);
        for at in 0..QUEUE {
            assert!(at < QUEUE);
            commands.try_send(Command::DevicesChanged).unwrap();
        }
        assert!(commands.try_send(Command::DevicesChanged).is_err(), "the queue is full");
        let (_wants, wanted) = watch::channel(Wants::default());
        let leaving = Leaving::default();
        leaving.leave();
        let join = Join { me: Some(ada_id), ..tone(booth) };
        let kept = ada.clone();
        let task = tokio::spawn(async move {
            run(&ada, common::engine(), &mut ada_events, join, (wanted, queued), leaving, |_| {}).await
        });
        timeout(WAIT, task).await.expect("the call ended").expect("the task ran").expect("left as asked");
        until(&bea_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;
        drop((kept, commands, server));
    }

    /// What the person wants is a state the call reads whole, so nothing of it waits behind acts in a queue: with the
    /// command queue full before the call reads any of it, a mute, a deafen and the last of a run of volumes all hold.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn what_is_wanted_behind_a_full_command_queue_still_holds() {
        let (server, invites) = Server::start("e2e_call_full_wants", 2).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let bea_id = MemberId::parse("bea").unwrap();
        let bea_call = start(bea, bea_events, listening(booth.clone(), Duration::from_millis(100)));
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth) });
        // At once, while the call is still joining and reads nothing.
        while ada_call.commands.try_send(Command::DevicesChanged).is_ok() {}
        for percent in [90, 60, 30] {
            let volume = choices::Volume::new(percent).unwrap();
            ada_call.wants.send_modify(|wants| wants.play(&bea_id, choices::Audio::Voice, volume));
        }
        ada_call.wants.send_modify(|wants| wants.mute(true));
        ada_call.wants.send_modify(|wants| wants.deafen(true));
        until(&bea_call.room, "Ada muted and deafened", |tiles| {
            tiles.iter().any(|t| t.name == "ADA" && t.muted && t.deafened)
        })
        .await;
        assert_eq!(ada_call.wants.borrow().clone(), {
            let mut held = Wants::default();
            held.play(&bea_id, choices::Audio::Voice, choices::Volume::new(30).unwrap());
            held.mute(true);
            held.deafen(true);
            held
        });
        // And a change made while the call runs is carried out as it is made.
        ada_call.wants.send_modify(|wants| wants.deafen(false));
        until(&bea_call.room, "Ada hears again, still muted", |tiles| {
            tiles.iter().any(|t| t.name == "ADA" && t.muted && !t.deafened)
        })
        .await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_closed_command_channel_is_a_leave_that_gives_the_place_back() {
        let (server, invites) = Server::start("e2e_call_closed", 2).await;
        let (ada, ada_events, booth) = registered(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, tone(booth.clone()));
        let bea_call = start(bea, bea_events, listening(booth, Duration::from_millis(100)));
        until(&bea_call.room, "Ada there", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;

        let Running { commands, task, _link, .. } = ada_call;
        drop(commands);
        timeout(WAIT, task).await.unwrap().unwrap().expect("left");
        until(&bea_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_taken_over_by_the_same_account_elsewhere_ends_as_moved_and_leaves_the_new_place_alone() {
        let (server, invites) = Server::start("e2e_call_moved", 2).await;
        let (ada, ada_events, booth) = registered(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, tone(booth.clone()));
        let bea_call = start(bea, bea_events, listening(booth.clone(), Duration::from_millis(100)));
        until(&bea_call.room, "Ada there", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;

        // Ada signs in again on another device and joins: the seat moves.
        let url = ServerUrl::parse(&server.url()).unwrap();
        let (other, _other_events) = Link::connect(&url, &Trust::system(), "e2e", "test").await.unwrap();
        other.sign_in_with_password("ada", &Secret::new(PASSWORD.into())).await.expect("signed in again");
        other.ask(VoiceJoin { channel_id: booth, ..Default::default() }).await.expect("joined");

        let (moved, ada_room) = ada_call.ended().await;
        assert!(matches!(moved, Err(Stopped::Ended(_))), "the call ended: {moved:?}");
        assert_eq!(ada_room.lock().unwrap().ended_why(), Some(Why::Moved));
        assert_eq!(
            ada_room.lock().unwrap().ended(),
            Some(&screens::End::Closed(Some(proto::chatkord_v1::CloseReason::Moved)))
        );
        // The new place is still there for Bea to see.
        bea_call.passes(Looked::Levels, 5).await;
        assert!(bea_call.room.lock().unwrap().tiles(Instant::now()).iter().any(|t| t.name == "ADA"));
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_in_a_channel_that_is_deleted_ends_as_deleted() {
        let (server, invites) = Server::start("e2e_call_deleted", 1).await;
        let (ada, ada_events, booth) = registered(&server.url(), "ada", &invites[0]).await;
        let ada_call = start(ada, ada_events, tone(booth.clone()));
        let (cy, _cy_events, _) = registered(&server.url(), "cy", &server.owner_code()).await;
        until(&ada_call.room, "Ada there", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;
        cy.ask(proto::chatkord_v1::ChannelDelete { channel_id: booth.to_string() }).await.expect("deleted");

        let (ended, room) = ada_call.ended().await;
        assert!(matches!(ended, Err(Stopped::Ended(_))), "the call ended: {ended:?}");
        assert_eq!(room.lock().unwrap().ended_why(), Some(Why::Deleted));
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_admin_s_mute_silences_the_member_whatever_their_own_mute_and_the_lift_leaves_it_as_they_set_it() {
        let (server, invites) = Server::start("e2e_call_held", 1).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (cy, cy_events, _, cy_id) = registered_as(&server.url(), "cy", &server.owner_code()).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id.clone()), ..tone(booth.clone()) });
        // An administrator moderates over the session's own connection, as the window does, in the call or not.
        let moderator = cy.clone();
        let cy_call = start(cy, cy_events, Join { me: Some(cy_id), ..listening(booth, Duration::from_millis(100)) });
        let heard_by_cy = |ada_id: &MemberId| {
            let id = ada_id.clone();
            move |tiles: &[screens::Tile]| tiles.iter().any(|t| t.user_id.as_ref() == Some(&id) && !t.muted)
        };
        until(&cy_call.room, "Ada sending", heard_by_cy(&ada_id)).await;

        moderator
            .ask(proto::chatkord_v1::VoiceMuteMember { user_id: ada_id.as_str().to_owned(), muted: true })
            .await
            .expect("muted for everyone");
        let held = |ada_id: &MemberId| {
            let id = ada_id.clone();
            move |tiles: &[screens::Tile]| tiles.iter().any(|t| t.user_id.as_ref() == Some(&id) && t.held && t.muted)
        };
        until(&ada_call.room, "Ada told she is held", held(&ada_id)).await;
        until(&cy_call.room, "the room told Ada is held", held(&ada_id)).await;

        // Her own mute is hers while held: her unmute is asked and kept, and the hold still silences her.
        ada_call.wants.send_modify(|wants| wants.mute(false));
        let own_off = cy_call
            .room
            .wait(WAIT, |room| (room.held(&ada_id) && !room.state_of(&ada_id).mic_muted).then_some(()))
            .await;
        assert!(own_off.is_some(), "the room is told her own mute is off and the hold stands");
        cy_call.passes(Looked::Levels, 6).await;
        let tiles = cy_call.room.lock().unwrap().tiles(std::time::Instant::now());
        assert!(held(&ada_id)(&tiles), "held, she is not heard");

        // Lifted, her own mute is as she set it: she is heard at once.
        moderator
            .ask(proto::chatkord_v1::VoiceMuteMember { user_id: ada_id.as_str().to_owned(), muted: false })
            .await
            .expect("lifted");
        until(&cy_call.room, "Ada heard again", heard_by_cy(&ada_id)).await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_member_an_admin_took_out_ends_as_taken_out_and_may_join_again_at_once() {
        let (server, invites) = Server::start("e2e_call_taken_out", 1).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (cy, cy_events, _, cy_id) = registered_as(&server.url(), "cy", &server.owner_code()).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id.clone()), ..tone(booth.clone()) });
        let moderator = cy.clone();
        let cy_call =
            start(cy, cy_events, Join { me: Some(cy_id), ..listening(booth.clone(), Duration::from_millis(100)) });
        until(&cy_call.room, "Ada there", |tiles| tiles.iter().any(|t| t.name == "ADA")).await;

        moderator
            .ask(proto::chatkord_v1::VoiceTakeOut { user_id: ada_id.as_str().to_owned() })
            .await
            .expect("taken out");
        let (ended, room) = ada_call.ended().await;
        assert!(matches!(ended, Err(Stopped::Ended(_))), "the call ended: {ended:?}");
        assert_eq!(room.lock().unwrap().ended_why(), Some(Why::TakenOut));
        assert_eq!(
            room.lock().unwrap().ended(),
            Some(&screens::End::Closed(Some(proto::chatkord_v1::CloseReason::TakenOut)))
        );
        until(&cy_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;

        // Nothing bars her: a new place is hers at once.
        let url = ServerUrl::parse(&server.url()).unwrap();
        let (again, _events) = Link::connect(&url, &Trust::system(), "e2e", "test").await.unwrap();
        again.sign_in_with_password("ada", &Secret::new(PASSWORD.into())).await.expect("signed in again");
        let joined = again.ask(VoiceJoin { channel_id: booth, ..Default::default() }).await;
        assert!(joined.is_ok(), "{joined:?}");
        drop(server);
    }

    /// Longer than libwebrtc tries a route before it gives up, and shorter than the server keeps a place whose route
    /// is gone (its consent check, 30 s, then its grace, 30 s).
    const CUT: Duration = Duration::from_secs(40);

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_whose_media_is_cut_past_the_engines_give_up_is_heard_again_once_it_comes_back() {
        let (server, relay, invites) = Server::start_relayed("e2e_call_mended", 2).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _, bea_id) = registered_as(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth.clone()) });
        let bea_call =
            start(bea, bea_events, Join { me: Some(bea_id), ..listening(booth, Duration::from_millis(100)) });
        let heard = |tiles: &[screens::Tile]| tiles.iter().any(|t| t.name == "ADA" && t.speaking);
        until(&bea_call.room, "Ada heard from her tone", heard).await;

        relay.cut();
        let cut = Instant::now();
        let mending =
            |tiles: &[screens::Tile]| tiles.iter().any(|t| t.own && t.connection == screens::Connection::Reconnecting);
        until(&bea_call.room, "Bea's own tile says it is made again", mending).await;
        bea_call
            .passes(Looked::Connection, usize::try_from(CUT.saturating_sub(cut.elapsed()).as_secs()).unwrap_or(0))
            .await;
        assert!(!heard(&bea_call.room.lock().unwrap().tiles(Instant::now())), "nothing is heard through a cut");
        relay.mend();
        let mended = Instant::now();
        until(&bea_call.room, "Ada heard again", heard).await;
        let back = mended.elapsed();
        until(&bea_call.room, "Bea's own tile connected again", |tiles| !mending(tiles)).await;
        for (who, call) in [("ada", &ada_call), ("bea", &bea_call)] {
            for (at, line) in call.said.lock().unwrap().iter().filter(|(at, _)| *at > cut) {
                println!("{who} {:>5.1} s after the cut: {line}", at.duration_since(cut).as_secs_f64());
            }
        }
        println!(
            "cut {:.1} s, heard again {:.1} s after it was mended",
            mended.duration_since(cut).as_secs_f64(),
            back.as_secs_f64()
        );
        assert!(!ada_call.task.is_finished() && !bea_call.task.is_finished(), "both are still in the call");
        ada_call.leaving.leave();
        bea_call.leaving.leave();
        ada_call.ended().await.0.expect("left as asked");
        bea_call.ended().await.0.expect("left as asked");
        drop(server);
    }

    /// A call's end is bounded: the server told and everything let go within `LEAVE_WITHIN`, or the call is over all
    /// the same. One whose server answers nothing used to wait out the leave's ask, a quarter of a minute.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_leave_whose_server_answers_nothing_is_over_within_its_deadline() {
        let (server, invites) = Server::start("e2e_call_leave_bound", 2).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth.clone()) });
        let bea_call = start(bea, bea_events, listening(booth, Duration::from_millis(100)));
        until(&bea_call.room, "Ada heard from her tone", |tiles| tiles.iter().any(|t| t.name == "ADA" && t.speaking))
            .await;

        server.pause();
        let asked = Instant::now();
        ada_call.leaving.leave();
        let ended = timeout(call::room::LEAVE_WITHIN + Duration::from_secs(2), ada_call.task).await;
        let took = asked.elapsed();
        server.resume();
        ended.unwrap_or_else(|_| panic!("the call was not over {took:?} after the leave")).unwrap().expect("left");
        assert!(took >= call::room::LEAVE_WITHIN, "over after {took:?}, before the server could have answered");
        // The server, running again, reads what was sent before anything was taken down: Ada is gone.
        until(&bea_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_whose_server_stops_answering_still_reports_its_levels_and_its_connection_and_then_leaves() {
        let (server, invites) = Server::start("e2e_call_unanswered", 2).await;
        let (ada, ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let (bea, bea_events, _) = registered(&server.url(), "bea", &invites[1]).await;
        let ada_call = start(ada, ada_events, Join { me: Some(ada_id), ..tone(booth.clone()) });
        let bea_call = start(bea, bea_events, listening(booth, Duration::from_millis(100)));
        until(&bea_call.room, "Ada heard from her tone", |tiles| tiles.iter().any(|t| t.name == "ADA" && t.speaking))
            .await;
        ada_call.passes(Looked::Connection, 1).await;

        // The server stops where it is: the state Ada asks for is never answered while it is stopped.
        server.pause();
        let asked = Instant::now();
        ada_call.wants.send_modify(|wants| wants.mute(true));
        ada_call.wants.send_modify(|wants| wants.deafen(true));
        bea_call.passes(Looked::Connection, 2).await;
        let (levels, looks) =
            (ada_call.looked_since(Looked::Levels, asked), ada_call.looked_since(Looked::Connection, asked));
        assert!(levels >= 5, "{levels} reports of the levels in 2 s while the ask hung, one due every 100 ms");
        assert!(looks >= 1, "{looks} looks at the connection in 2 s while the ask hung");

        // A leave is seen while the ask still hangs: the loop stops reporting at once.
        ada_call.leaving.leave();
        let left = Instant::now();
        bea_call.passes(Looked::Levels, 6).await;
        let after = ada_call.looked_since(Looked::Levels, left + Duration::from_millis(300));
        assert_eq!(after, 0, "the call went on reporting its levels after the leave");
        server.resume();
        let (left, _) = ada_call.ended().await;
        left.expect("left as asked");
        until(&bea_call.room, "Ada gone", |tiles| tiles.iter().all(|t| t.name != "ADA")).await;
        bea_call.leaving.leave();
        bea_call.ended().await.0.expect("left as asked");
        drop(server);
    }
}
