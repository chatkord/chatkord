//! The whole path over the real wire: two clients join a voice channel and carry a tone through the `chatkord-server`
//! binary, a process of its own because its worker and the engine cannot be linked into one.

use crate::common;

use call::room::Report;
use cli::{Account, Act, Command, run};
use common::looks::Looks;
use common::running;
use common::{Person, Server, hears_a_tone, join_voice, registered_as, told, voice_of};
use link::Trust;
use platform::Platform;
use proto::chatkord_v1::TrackKind;
use proto::chatkord_v1::event;
use screens::RoomEvent;
use std::sync::Arc;
use std::time::Duration;
use terms::{Address, Heard, MemberId};
use tier::skipped;
use tokio::sync::oneshot;
use tokio::time::timeout;
use webrtc::VoiceRefused;

const RATE_HZ: u32 = 48_000;

/// How long a microphone may take to be let go of, or taken again, after muting or unmuting.
const UNMUTE_BOUND: Duration = Duration::from_millis(1000);

/// Feeds a share's sound a stereo tone, 10 ms at a time as the mix hands it over, for 30 s; false if it stopped
/// taking it.
fn feed_a_stereo_tone(feeder: webrtc::SoundFeeder) -> tokio::task::JoinHandle<bool> {
    tokio::spawn(async move {
        let mut clock = common::chunk_clock(Duration::from_millis(10));
        for chunk in 0..3_000_u32 {
            let samples: Vec<i16> = (0..RATE_HZ / 100)
                .flat_map(|i| {
                    let t = f64::from(chunk * RATE_HZ / 100 + i) / f64::from(RATE_HZ);
                    let side = |hz: f64| (8_000.0 * (std::f64::consts::TAU * hz * t).sin()) as i16;
                    [side(440.0), side(660.0)]
                })
                .collect();
            if !feeder.push(&samples).await {
                return false;
            }
            clock.tick().await;
        }
        true
    })
}

/// The loudest sample, as a share of full scale, that our chain mixes `user_id`'s sound of `kind` at over a second
/// that starts once a change has reached the mix, past the jitter buffer.
async fn mixed(hearing: &webrtc::Hearing, user_id: &str, kind: TrackKind) -> f64 {
    let mut looks = Looks::levels();
    looks.over(Duration::from_millis(600)).await;
    hearing.mixed(user_id, kind).expect("the sound is taken");
    looks.over(Duration::from_secs(1)).await;
    f64::from(hearing.mixed(user_id, kind).expect("the sound is still taken")) / f64::from(i16::MAX)
}

/// What became of the device `of` reads, asked for on `voice`'s engine, once the chain's thread has answered the
/// ask: opened, or why not. An ask only asks, so nothing is read of a device before this.
async fn opened(
    voice: &webrtc::Voice,
    of: fn(&webrtc::Sounding) -> webrtc::Standing,
) -> Result<(), webrtc::DevicesRefused> {
    let devices = voice.devices()?;
    let mut told = devices.sounding();
    let answered = |now: &webrtc::Sounding| matches!(of(now), webrtc::Standing::Open | webrtc::Standing::Refused(_));
    let settled = timeout(common::WAIT, told.wait_for(answered)).await.expect("the chain answers an ask");
    match of(&settled.expect("the chain lives")) {
        webrtc::Standing::Refused(why) => Err(why),
        webrtc::Standing::Open
        | webrtc::Standing::Opening
        | webrtc::Standing::Closed
        | webrtc::Standing::Unanswered
        | webrtc::Standing::Hung => Ok(()),
    }
}

fn speaker(sounding: &webrtc::Sounding) -> webrtc::Standing {
    sounding.speaker
}

fn microphone(sounding: &webrtc::Sounding) -> webrtc::Standing {
    sounding.microphone
}

/// How often the microphone's capture is looked at: the resolution its time to open is measured at.
const CAPTURE_LOOKS: Duration = Duration::from_millis(10);

/// How long it took the microphone to be (or not be) captured, if within `within`.
async fn capturing_within(speaking: &webrtc::Speaking, capturing: bool, within: Duration) -> Option<Duration> {
    let began = std::time::Instant::now();
    Looks::every(CAPTURE_LOOKS).until(within, || (speaking.is_capturing() == capturing).then(|| began.elapsed())).await
}

/// Whether `report` says a member other than `me` is heard at more than a quiet hum: the share of full scale the call
/// measured over its last stretch, from 0 to 100.
fn tone_heard(report: &Report, me: &MemberId) -> bool {
    matches!(report, Report::Room(RoomEvent::Levels(levels)) if levels.iter().any(|(who, level)| who != me && *level > 3))
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_member_who_plays_the_call_to_the_speakers_still_has_each_stream_measured() {
        let (server, invites) = Server::start("e2e", 2).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (mut bo_voice, mut bo_hearing) = voice_of(&state);
        if let Err(why) = bo_voice.play() {
            skipped(format!("{why:?}"));
            return;
        }
        if let Err(why) = opened(&bo_voice, speaker).await {
            skipped(format!("{why:?}"));
            return;
        }
        assert!(bo_voice.is_playing(), "the speaker is being played to");
        let ada_id = state.streams[0].user_id.clone();
        bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");

        // The tone is audible on this device's speaker for as long as it takes to be measured.
        let heard = hears_a_tone(&bo_voice).await;
        assert!(heard > 5, "the stream was measured while it was played, level {heard}");
        assert!(bo_voice.is_playing(), "and it is still being played");
        drop(bo_voice);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_deafened_member_keeps_every_stream_measured_and_the_speaker_running() {
        let (server, invites) = Server::start("e2e", 2).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (mut bo_voice, mut bo_hearing) = voice_of(&state);
        if let Err(why) = bo_voice.play() {
            skipped(format!("{why:?}"));
            return;
        }
        if let Err(why) = opened(&bo_voice, speaker).await {
            skipped(format!("{why:?}"));
            return;
        }
        bo_voice.set_deafened(true);
        let ada_id = state.streams[0].user_id.clone();
        bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");

        let heard = hears_a_tone(&bo_voice).await;
        assert!(heard > 5, "the stream is still measured while deafened, level {heard}");
        bo_voice.set_deafened(false);
        assert!(bo_voice.is_playing(), "the speaker never stopped: a deafen is the mix's gain");
    }

    /// Muting and unmuting within the budget a person feels, timed with nothing else running beside it.
    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_member_speaks_with_the_microphone_and_muting_does_not_end_the_stream() {
            let (server, invites) = Server::start("e2e", 2).await;
            let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
            let (ada_voice, _ada_hearing) = voice_of(&joined);
            let mut speaking = match ada_voice.speak(&ada).await {
                Ok(speaking) => speaking,
                Err(VoiceRefused::Sound(why)) => {
                    skipped(format!("{why:?}"));
                    return;
                }
                Err(other) => panic!("the microphone was not sent: {other:?}"),
            };
            if let Err(why) = opened(&ada_voice, microphone).await {
                skipped(format!("{why:?}"));
                return;
            }

            assert!(speaking.is_capturing(), "the microphone is open, not only a track made from it");
            let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
            let (bo_voice, mut bo_hearing) = voice_of(&state);
            let ada_id = state.streams[0].user_id.clone();
            bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada's microphone");
            // Whatever the microphone picks up, the stream arrives and is measured; its level is not asserted.
            let listed = |voice: &webrtc::Voice| voice.levels().iter().any(|(user, _)| *user == ada_id);
            let mut looks = Looks::levels();
            looks.until(common::WAIT, || listed(&bo_voice).then_some(())).await.expect("the stream is measured");

            // The engine itself lets go of the device while the only track reading from it is muted, which is what a
            // person sees in the desktop's microphone indicator, and takes it again on unmuting.
            speaking.mute();
            capturing_within(&speaking, false, UNMUTE_BOUND).await.expect("a muted microphone is let go of");
            // Kept muted a while, as a person does, before unmuting.
            looks.over(Duration::from_millis(500)).await;
            speaking.unmute().expect("unmuting");
            let took = capturing_within(&speaking, true, UNMUTE_BOUND)
                .await
                .expect("unmuting takes the microphone again within the bound");
            eprintln!("unmuting took the microphone again in {took:?}");
            assert!(listed(&bo_voice), "a muted microphone stays in the call");
            speaking.stop(&ada).await.expect("the track ends");
        }
    }

    /// The engine starts recording as any sound starts going out, so a share's sound could reopen a muted microphone
    /// (and move a Bluetooth headset to its call profile).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_shares_sound_starting_leaves_a_muted_microphone_closed() {
        let (server, invites) = Server::start("e2e", 1).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let mut speaking = match ada_voice.speak(&ada).await {
            Ok(speaking) => speaking,
            Err(VoiceRefused::Sound(why)) => {
                skipped(format!("{why:?}"));
                return;
            }
            Err(other) => panic!("the microphone was not sent: {other:?}"),
        };
        if let Err(why) = opened(&ada_voice, microphone).await {
            skipped(format!("{why:?}"));
            return;
        }
        speaking.mute();
        capturing_within(&speaking, false, common::WAIT).await.expect("a muted microphone is let go of");

        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        let feeder = sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts");
        let silence = vec![0_i16; (RATE_HZ / 100 * 2) as usize];
        let mut clock = common::chunk_clock(Duration::from_millis(10));
        for _ in 0..50 {
            assert!(feeder.push(&silence).await, "the share's sound takes its chunks");
            clock.tick().await;
        }
        assert!(!speaking.is_capturing(), "the share's sound must not open the muted microphone");

        speaking.unmute().expect("unmuting");
        capturing_within(&speaking, true, common::WAIT).await.expect("unmuting takes the microphone again");
        sharing.stop(&ada).await.expect("the screen ends");
        speaking.stop(&ada).await.expect("the track ends");
    }

    /// Calls run one after another on one engine: a microphone that ended must not be opened by a later share's sound.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_shares_sound_after_the_microphone_ended_opens_no_microphone() {
        let (server, invites) = Server::start("e2e", 1).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        match ada_voice.speak(&ada).await {
            Ok(speaking) => {
                if let Err(why) = opened(&ada_voice, microphone).await {
                    skipped(format!("{why:?}"));
                    return;
                }
                speaking.stop(&ada).await.expect("the track ends");
            }
            Err(VoiceRefused::Sound(why)) => {
                skipped(format!("{why:?}"));
                return;
            }
            Err(other) => panic!("the microphone was not sent: {other:?}"),
        }
        let devices = ada_voice.devices().expect("a hold");
        assert!(!devices.is_capturing(), "the microphone ended with its track");

        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        let feeder = sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts");
        let silence = vec![0_i16; (RATE_HZ / 100 * 2) as usize];
        let mut clock = common::chunk_clock(Duration::from_millis(10));
        for _ in 0..50 {
            assert!(feeder.push(&silence).await, "the share's sound takes its chunks");
            clock.tick().await;
        }
        assert!(!devices.is_capturing(), "the share's sound must not open the microphone");
        sharing.stop(&ada).await.expect("the screen ends");
    }
}

mod server {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_shared_in_either_codec_is_taken_in_that_codec() {
        for codec in [webrtc::Codec::H264, webrtc::Codec::Av1] {
            let (server, invites) = Server::start("e2e", 2).await;
            let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
            let (ada_voice, _ada_hearing) = voice_of(&joined);
            let (sharing, _feeder) = ada_voice.share(&ada, codec, 1280, 720).await.expect("ada shares");
            let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
            let screen = state.streams.iter().find(|s| s.kind == i32::from(TrackKind::ScreenVideo));
            let ada_id = screen.expect("ada's screen is live").user_id.clone();
            let (_bo_voice, mut bo_hearing) = voice_of(&state);
            let key_frames = bo_hearing
                .watch_encoded(&bo, &ada_id, webrtc::Video::Screen, |_| {})
                .await
                .expect("the screen is taken");
            assert_eq!(key_frames.codec(), codec, "the screen arrives in the codec it was sent in");
            sharing.stop(&ada).await.expect("the screen ends");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_tone_goes_from_one_client_through_the_real_server_to_another() {
        let (server, invites) = Server::start("e2e", 2).await;

        // Ada joins and sends a tone.
        let (ada, mut ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

        // Bo joins as the window does, is shown ada's stream, takes it and listens.
        let (bo, bo_events, booth, bo_id) = registered_as(&server.url(), "bo", &invites[1]).await;
        let bo = running::start(bo, bo_events, booth, bo_id.clone());
        common::told(&mut ada_events, |kind| matches!(kind, event::Kind::MemberJoined(_))).await;
        bo.until("the tone crossed the real server audibly", common::WAIT, |report| tone_heard(report, &bo_id)).await;

        drop(speaking);
        bo.leave().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_command_line_speaks_and_another_member_hears_it_until_it_leaves() {
        let (server, invites) = Server::start("e2e", 2).await;
        let address = Address::typed(&server.url()).expect("worth trying");

        // Bo is in the channel first, so ada's stream arrives to bo as an event.
        let (bo_link, bo_events, booth, bo_id) = registered_as(&server.url(), "bo", &invites[0]).await;
        let bo = running::start(bo_link, bo_events, booth.clone(), bo_id.clone());

        // Ada registers and speaks through the command line's own entry point.
        let said = common::Shared::new(Vec::new());
        let (stop, stopped) = oneshot::channel();
        let mut ada = Person { invite: invites[1].clone(), said: Arc::clone(&said), stop: Some(stopped) };
        let register = Command::As {
            account: Account { address: address.clone(), identity: "ada".into() },
            act: Act::Register(Some("Ada".into())),
        };
        run(register, &mut ada, &Platform::default(), &Trust::system()).await.expect("registered");
        let channel_id = booth;
        let trust = Trust::system();
        let platform = Platform::default();
        let speak = run(
            Command::As {
                account: Account { address, identity: "ada".into() },
                act: Act::Speak { channel_id: terms::ChannelId::parse(&channel_id).expect("a channel id"), tone: true },
            },
            &mut ada,
            &platform,
            &trust,
        );
        let hearing = async {
            bo.until("the command line's tone reached bo", common::WAIT, |report| tone_heard(report, &bo_id)).await;
            stop.send(()).expect("the command is still running");
        };
        let (spoke, ()) = tokio::join!(speak, hearing);
        spoke.expect("it left cleanly");
        assert!(said.lock().expect("a lock").iter().any(|line| line.contains("Left")));
        bo.leave().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_command_line_hears_a_member_who_is_already_speaking_and_one_who_starts_later() {
        let (server, invites) = Server::start("e2e", 3).await;
        let address = Address::typed(&server.url()).expect("worth trying");

        // Ada is already speaking when the listener joins.
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let channel_id = joined.channel_id.clone();

        let said = common::Shared::new(Vec::new());
        let (stop, stopped) = oneshot::channel();
        let mut listener = Person { invite: invites[1].clone(), said: Arc::clone(&said), stop: Some(stopped) };
        let register = Command::As {
            account: Account { address: address.clone(), identity: "bo".into() },
            act: Act::Register(Some("Bo".into())),
        };
        run(register, &mut listener, &Platform::default(), &Trust::system()).await.expect("registered");
        let trust = Trust::system();
        let platform = Platform::default();
        let listen = run(
            Command::As {
                account: Account { address, identity: "bo".into() },
                act: Act::Listen(terms::ChannelId::parse(&channel_id).expect("a channel id")),
            },
            &mut listener,
            &platform,
            &trust,
        );

        let loud = |said: &Vec<String>| {
            said.iter().any(|line| {
                line.rsplit_once("level ").is_some_and(|(_, level)| level.parse::<u8>().is_ok_and(|level| level > 5))
            })
        };
        let checking = async {
            said.wait(common::WAIT, |said| loud(said).then_some(())).await.expect("the listener reports a loud stream");

            // Cy starts later, and ada stops: the listener is told of both by the server.
            let (cy, _cy_events, joined) = join_voice(&server.url(), "cy", &invites[2]).await;
            let (cy_voice, _cy_hearing) = voice_of(&joined);
            let _cy_speaking = cy_voice.speak_tone(&cy).await.expect("cy speaks");
            ada_speaking.stop(&ada).await.expect("ada stops");
            let noticed = said
                .wait(common::WAIT, |said| {
                    (said.iter().filter(|line| line.starts_with("Hearing ")).count() == 2
                        && said.iter().any(|line| line.contains("stopped speaking")))
                    .then_some(())
                })
                .await;
            assert!(noticed.is_some(), "the listener took cy and noticed ada stop: {:?}", said.lock().expect("a lock"));
            stop.send(()).expect("the command is still running");
        };
        let (listened, ()) = tokio::join!(listen, checking);
        listened.expect("it left cleanly");
        let said = said.lock().expect("a lock");
        assert!(said.iter().any(|line| line.starts_with("Hearing ")), "{said:?}");
        assert!(said.iter().any(|line| line.contains("Left")));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_member_speaks_and_hears_another_at_once_over_their_two_transports() {
        let (server, invites) = Server::start("e2e", 2).await;

        // Each of the two sends a tone over the send transport and takes the other's over the receive one.
        let (ada, mut ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, mut ada_hearing) = voice_of(&joined);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut bo_hearing) = voice_of(&state);
        let _bo_speaking = bo_voice.speak_tone(&bo).await.expect("bo speaks");
        let ada_id = state.streams[0].user_id.clone();
        bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");

        let bo_id = timeout(common::WAIT, async {
            loop {
                if let Some(Heard::Event(event)) = ada_events.next().await
                    && let Some(event::Kind::ProducerJoined(joined)) = event.kind
                {
                    break joined.user_id;
                }
            }
        })
        .await
        .expect("ada is told bo's stream is live");
        ada_hearing.take(&ada, &bo_id, TrackKind::Microphone).await.expect("ada takes bo");

        let (heard_by_bo, heard_by_ada) = tokio::join!(hears_a_tone(&bo_voice), hears_a_tone(&ada_voice));
        assert!(
            heard_by_bo > 5 && heard_by_ada > 5,
            "both tones crossed: bo heard {heard_by_bo}, ada heard {heard_by_ada}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_connection_reports_a_round_trip_and_the_audio_packets_it_counted() {
        let (server, invites) = Server::start("e2e", 2).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let _ada_speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");

        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let (bo_voice, mut bo_hearing) = voice_of(&state);
        let _bo_speaking = bo_voice.speak_tone(&bo).await.expect("bo speaks");
        let ada_id = state.streams[0].user_id.clone();
        bo_hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");
        hears_a_tone(&bo_voice).await;

        let seen = Looks::every(screens::LOOK_EVERY)
            .until_answered(common::WAIT, || async {
                let now = bo_voice.connection().await.expect("the engine answers");
                (now.round_trip.is_some() && now.heard > 0 && now.sent > 0).then_some(now)
            })
            .await
            .expect("a round trip is measured and audio is counted in both directions");
        assert_eq!(seen.lost, 0, "nothing is lost over loopback");
        assert!(seen.received >= seen.heard, "every stream received counts the audio heard");
        assert!(seen.received_bytes > 0 && seen.sent_bytes > 0, "the audio's bytes are counted as its packets are");
        assert!(
            seen.up_bytes >= seen.sent_bytes && seen.down_bytes >= seen.received_bytes,
            "the transports carried it"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_screen_ends_and_starts_again_while_the_microphone_keeps_flowing_on_the_same_connection() {
        let (server, invites) = Server::start("e2e", 2).await;
        let screen = i32::from(TrackKind::ScreenVideo);
        let microphone = i32::from(TrackKind::Microphone);

        // Ada sends a tone and a screen over her one send connection.
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        assert!(
            matches!(ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await, Err(VoiceRefused::AlreadySending)),
            "one screen at a time"
        );

        let (bo, mut bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let kinds: Vec<i32> = state.streams.iter().map(|stream| stream.kind).collect();
        assert!(kinds.contains(&microphone) && kinds.contains(&screen), "bo is shown both: {kinds:?}");
        let ada_id = state.streams[0].user_id.clone();
        let (bo_voice, mut hearing) = voice_of(&state);
        hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada's microphone");
        hears_a_tone(&bo_voice).await;

        // The screen ends: bo is told, and the tone is still there.
        sharing.stop(&ada).await.expect("the screen ends");
        told(&mut bo_events, |kind| matches!(kind, event::Kind::ProducerClosed(closed) if closed.kind == screen)).await;
        hears_a_tone(&bo_voice).await;

        // It starts again in the section it left, and the tone is still there.
        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares again");
        told(&mut bo_events, |kind| matches!(kind, event::Kind::ProducerJoined(joined) if joined.kind == screen)).await;
        hears_a_tone(&bo_voice).await;

        // The other way round: the tone ends and the screen, which the server still holds, ends on its own after.
        speaking.stop(&ada).await.expect("the tone ends");
        told(&mut bo_events, |kind| matches!(kind, event::Kind::ProducerClosed(closed) if closed.kind == microphone))
            .await;
        sharing.stop(&ada).await.expect("the screen ends");
        told(&mut bo_events, |kind| matches!(kind, event::Kind::ProducerClosed(closed) if closed.kind == screen)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_shares_sound_goes_beside_its_picture_and_ends_with_it() {
        let (server, invites) = Server::start("e2e", 2).await;
        let sound = i32::from(TrackKind::ScreenAudio);

        // Ada shares a screen with a stereo tone as its sound, 10 ms at a time as the mix hands it over.
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        let feeder = sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts");
        let feeding = feed_a_stereo_tone(feeder);

        // Bo is shown the sound, takes it and receives it; it is no microphone's level.
        let (bo, mut bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id =
            state.streams.iter().find(|stream| stream.kind == sound).expect("ada's sound is live").user_id.clone();
        let (bo_voice, mut hearing) = voice_of(&state);
        hearing.take(&bo, &ada_id, TrackKind::ScreenAudio).await.expect("bo takes ada's sound");
        let heard = Looks::every(screens::LOOK_EVERY)
            .until_answered(Duration::from_secs(15), || async {
                let heard = bo_voice.connection().await.expect("the engine reports").heard;
                (heard > 100).then_some(heard)
            })
            .await
            .expect("the share's sound arrives");
        eprintln!("bo received {heard} packets of ada's share sound");
        assert!(bo_voice.levels().is_empty(), "a share's sound is not a microphone's level");

        // The share ends: its sound ends with it, and bo is told.
        sharing.stop(&ada).await.expect("the share ends");
        told(&mut bo_events, |kind| matches!(kind, event::Kind::ProducerClosed(closed) if closed.kind == sound)).await;
        assert!(hearing.forget(&ada_id, TrackKind::ScreenAudio), "the sound was being taken");
        feeding.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_members_voice_and_their_shares_sound_are_each_mixed_at_their_own_volume() {
        let (server, invites) = Server::start("e2e", 2).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let _speaking = ada_voice.speak_tone(&ada).await.expect("ada speaks");
        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        let feeding = feed_a_stereo_tone(sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts"));

        let (bo, _bo_events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
        let ada_id = state.streams[0].user_id.clone();
        let (bo_voice, mut hearing) = voice_of(&state);
        let (voice, sound) = (TrackKind::Microphone, TrackKind::ScreenAudio);
        hearing.take(&bo, &ada_id, voice).await.expect("bo takes ada's voice");
        hearing.take(&bo, &ada_id, sound).await.expect("bo takes ada's share's sound");
        let (voice_full, sound_full) = (mixed(&hearing, &ada_id, voice).await, mixed(&hearing, &ada_id, sound).await);
        assert!(voice_full > 0.05 && sound_full > 0.05, "both are mixed as sent: {voice_full}, {sound_full}");

        bo_voice.set_volume(&ada_id, voice, 0.5);
        let voice_half = mixed(&hearing, &ada_id, voice).await;
        let sound_kept = mixed(&hearing, &ada_id, sound).await;
        assert!((voice_half / voice_full - 0.5).abs() < 0.1, "the voice at half: {voice_half} of {voice_full}");
        assert!((sound_kept / sound_full - 1.0).abs() < 0.1, "the sound as it was: {sound_kept} of {sound_full}");

        bo_voice.set_volume(&ada_id, sound, 0.0);
        let sound_none = mixed(&hearing, &ada_id, sound).await;
        assert!(sound_none < sound_full / 100.0, "the sound silenced: {sound_none} of {sound_full}");
        assert!(hears_a_tone(&bo_voice).await > 5, "the voice is still measured as sent");

        hearing.release(&bo, &ada_id, voice).await;
        hearing.take(&bo, &ada_id, voice).await.expect("bo takes ada's voice again");
        let voice_again = mixed(&hearing, &ada_id, voice).await;
        assert!((voice_again / voice_full - 0.5).abs() < 0.1, "taken again at half: {voice_again} of {voice_full}");
        eprintln!("voice {voice_full}, at 50 % {voice_half}, taken again {voice_again}");
        eprintln!("sound {sound_full}, beside {sound_kept}, at 0 % {sound_none}");
        feeding.abort();
    }

    /// What a share's sound costs on the wire while it plays, while the applications play silence, and while nothing is
    /// linked (no chunk at all): the idle floor, printed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_shares_sound_costs_nothing_while_nothing_plays() {
        const SPELL: Duration = Duration::from_secs(5);
        let (server, invites) = Server::start("e2e", 1).await;
        let (ada, _ada_events, joined) = join_voice(&server.url(), "ada", &invites[0]).await;
        let (ada_voice, _ada_hearing) = voice_of(&joined);
        let (sharing, _feeder) = ada_voice.share(&ada, webrtc::Codec::H264, 1280, 720).await.expect("ada shares");
        let feeder = sharing.sound(&ada, RATE_HZ, 2).await.expect("the share's sound starts");

        // A spell of chunks made by `sample`, or of none, and what it cost: (packets, payload and wire bits a second).
        let spell = |sample: Option<fn(u32) -> i16>| {
            let (feeder, voice) = (feeder.clone(), &ada_voice);
            async move {
                let before = voice.connection().await.expect("the engine reports");
                let started = tokio::time::Instant::now();
                let mut chunk = 0_u32;
                let mut clock = common::chunk_clock(Duration::from_millis(10));
                while started.elapsed() < SPELL {
                    if let Some(sample) = sample {
                        let samples: Vec<i16> =
                            (0..RATE_HZ / 100).flat_map(|i| [sample(chunk * RATE_HZ / 100 + i); 2]).collect();
                        assert!(feeder.push(&samples).await, "the engine takes 10 ms of stereo");
                    }
                    chunk += 1;
                    clock.tick().await;
                }
                let after = voice.connection().await.expect("the engine reports");
                let seconds = started.elapsed().as_secs_f64();
                let per = |bytes: u64| (bytes as f64 * 8.0 / seconds).round() as u64;
                (
                    after.sent - before.sent,
                    per(after.sent_bytes - before.sent_bytes),
                    per(after.up_bytes - before.up_bytes),
                )
            }
        };
        let tone =
            spell(Some(|i| (8_000.0 * (std::f64::consts::TAU * 440.0 * f64::from(i) / 48_000.0).sin()) as i16)).await;
        let silence = spell(Some(|_| 0)).await;
        let nothing = spell(None).await;
        for (what, (packets, payload, wire)) in [("a tone", tone), ("silence", silence), ("no chunks", nothing)] {
            eprintln!("{what}: {packets} packets in {SPELL:?}, {payload} bit/s of payload, {wire} bit/s on the wire");
        }
        assert!(tone.0 > 100, "the tone is sent: {} packets", tone.0);
        // Zeros go as Opus DTX: still sent, at a packet every 400 ms once the encoder rests.
        assert!(silence.0 > 0 && silence.0 < tone.0 / 4, "silence is sent at DTX's floor: {} packets", silence.0);
        assert!(nothing.0 < 10, "nothing is sent while no chunk comes: {} packets", nothing.0);
        sharing.stop(&ada).await.expect("the share ends");
    }
}
