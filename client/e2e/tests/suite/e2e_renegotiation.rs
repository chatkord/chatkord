//! A call that goes on for a while, over the real server: streams that come and go keep the receive offer bounded, a
//! member who leaves takes only their own stream, and a sender whose loop stalls is heard again when it resumes.

use crate::common;

use common::looks::Looks;
use common::{Server, hears_a_tone, registered, registered_as, told, voice_of};
use link::Link;
use proto::chatkord_v1::{TrackKind, VoiceJoin, VoiceLeave, VoiceState, event};
use std::time::Duration;
use tokio::time::timeout;
use webrtc::Voice;

/// The most sections the receive offer holds for one stream taken and dropped again and again: the resting first,
/// the one taken now and the one closed last.
const BOUND: usize = 3;
const MICROPHONE: TrackKind = TrackKind::Microphone;
/// A start and an end each round, at the four changes a second the server lets a member make.
const ROUND: Duration = Duration::from_millis(500);

async fn join(link: &Link, booth: &str) -> VoiceState {
    let Ok(state) = link.ask(VoiceJoin { channel_id: booth.into(), ..Default::default() }).await else {
        panic!("not a voice state")
    };
    state
}

/// The level `user_id` is heard at over the next stretch, the loudest of several asks.
async fn level_of(hearing: &Voice, user_id: &str) -> u8 {
    let (mut looks, mut loudest) = (Looks::levels(), 0);
    for _ in 0..5 {
        looks.next().await;
        let heard = hearing.levels().into_iter().find(|(who, _)| who == user_id).map_or(0, |(_, level)| level);
        loudest = loudest.max(heard);
    }
    loudest
}

/// Waits until `user_id` is heard as a tone.
async fn hears(hearing: &Voice, user_id: &str) {
    timeout(common::WAIT, async { while level_of(hearing, user_id).await <= 5 {} })
        .await
        .unwrap_or_else(|_| panic!("{user_id} was not heard in time"));
}

fn producer_closed(user_id: &str) -> impl Fn(&event::Kind) -> bool + '_ {
    move |kind| matches!(kind, event::Kind::ProducerClosed(closed) if closed.user_id == user_id && closed.kind == i32::from(MICROPHONE))
}

fn producer_joined(user_id: &str) -> impl Fn(&event::Kind) -> bool + '_ {
    move |kind| matches!(kind, event::Kind::ProducerJoined(joined) if joined.user_id == user_id && joined.kind == i32::from(MICROPHONE))
}

mod server {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_microphone_that_starts_and_ends_twenty_times_keeps_the_listeners_offer_bounded() {
        let (server, invites) = Server::start("e2e_renegotiation", 2).await;
        let (ada, _ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada_id = ada_id.as_str().to_owned();
        let (ada_voice, _ada_hearing) = voice_of(&join(&ada, &booth).await);
        let (bo, mut bo_events, _) = registered(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut hearing) = voice_of(&join(&bo, &booth).await);

        let mut rounds = common::looks::paced(ROUND);
        for round in 0..20 {
            rounds.tick().await;
            let speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
            told(&mut bo_events, producer_joined(&ada_id)).await;
            hearing.take(&bo, &ada_id, MICROPHONE).await.unwrap_or_else(|refused| panic!("round {round}: {refused:?}"));
            hears_a_tone(&bo_voice).await;
            speaking.stop(&ada).await.expect("the track ends");
            told(&mut bo_events, producer_closed(&ada_id)).await;
            hearing.forget(&ada_id, MICROPHONE);
            assert!(hearing.receive_sections() <= BOUND, "round {round}: {} sections", hearing.receive_sections());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_member_who_leaves_takes_only_their_stream_and_their_section_is_used_again() {
        let (server, invites) = Server::start("e2e_renegotiation", 3).await;
        let (ada, _ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada_id = ada_id.as_str().to_owned();
        let (ada_voice, _ada_hearing) = voice_of(&join(&ada, &booth).await);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let (cy, _cy_events, _, cy_id) = registered_as(&server.url(), "cy", &invites[2]).await;
        let cy_id = cy_id.as_str().to_owned();
        let (bo, mut bo_events, _) = registered(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut hearing) = voice_of(&join(&bo, &booth).await);
        hearing.take(&bo, &ada_id, MICROPHONE).await.expect("bo takes ada");

        let mut most = 0;
        for round in 0..3 {
            let (cy_voice, _cy_hearing) = voice_of(&join(&cy, &booth).await);
            let cy_speaking = cy_voice.speak_tone(&cy).await.expect("cy speaks");
            told(&mut bo_events, producer_joined(&cy_id)).await;
            hearing.take(&bo, &cy_id, MICROPHONE).await.unwrap_or_else(|refused| panic!("round {round}: {refused:?}"));
            hears(&bo_voice, &cy_id).await;

            // Cy leaves the call: the server closes cy's stream, and only that one.
            drop(cy_speaking);
            cy.ask(VoiceLeave {}).await.expect("cy leaves");
            drop(cy_voice);
            told(&mut bo_events, producer_closed(&cy_id)).await;
            hearing.forget(&cy_id, MICROPHONE);
            hears(&bo_voice, &ada_id).await;
            assert_eq!(hearing.taken(MICROPHONE), std::slice::from_ref(&ada_id), "round {round}");
            most = most.max(hearing.receive_sections());
        }
        assert!(most <= BOUND, "{most} sections after three departures");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_sender_whose_loop_stalls_leaves_the_others_heard_and_is_heard_again_when_it_resumes() {
        let (server, invites) = Server::start("e2e_renegotiation", 3).await;
        let (ada, _ada_events, booth, ada_id) = registered_as(&server.url(), "ada", &invites[0]).await;
        let ada_id = ada_id.as_str().to_owned();
        let (ada_voice, _ada_hearing) = voice_of(&join(&ada, &booth).await);
        let mut ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let (cy, _cy_events, _, cy_id) = registered_as(&server.url(), "cy", &invites[2]).await;
        let cy_id = cy_id.as_str().to_owned();
        let (cy_voice, _cy_hearing) = voice_of(&join(&cy, &booth).await);
        let _cy_speaking = cy_voice.speak_tone(&cy).await.expect("cy speaks");
        let (bo, _bo_events, _) = registered(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut hearing) = voice_of(&join(&bo, &booth).await);
        for user_id in [&ada_id, &cy_id] {
            hearing.take(&bo, user_id, MICROPHONE).await.expect("bo takes both");
            hears(&bo_voice, user_id).await;
        }
        let sections = hearing.receive_sections();

        ada_speaking.stall(true);
        timeout(common::WAIT, async { while level_of(&bo_voice, &ada_id).await > 5 {} })
            .await
            .expect("ada's stall is heard as silence");
        hears(&bo_voice, &cy_id).await;
        Looks::levels().over(Duration::from_secs(3)).await;
        hears(&bo_voice, &cy_id).await;

        ada_speaking.stall(false);
        hears(&bo_voice, &ada_id).await;
        let mut taken = hearing.taken(MICROPHONE);
        taken.sort();
        let mut both = vec![ada_id, cy_id];
        both.sort();
        assert_eq!(taken, both, "nobody was taken again");
        assert_eq!(hearing.receive_sections(), sections, "the offer did not change");
    }
}
