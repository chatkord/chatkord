//! How far a face and its voice drift apart between two machines: the receiving member, signed in to a server whose
//! voice channel another machine's window is in a call in, takes that member's camera and voice; a person claps in front of the camera, and
//! each clap is found twice, as the loudest change between two pictures and as the start of its sound. The voice is
//! played to a virtual speaker and recorded on this process's clock, the pictures timed as they arrive decoded, so the
//! offset is each sound's start less its picture's: positive, the voice comes late. A measurement, run on request.
//!
//! What it does not count: a picture's wait for the screen's next refresh (at most one, 8.3 ms on a 120 Hz panel), and
//! the speaker's own latency past the sound server. The clap's picture is the change at its peak, so a camera's
//! exposure adds up to one picture's period of doubt.

use crate::common;

use call::room::Report;
use common::onsets::{LOUD, RATE_HZ, median, onsets};
use common::running::start_hearing_on;
use link::{Link, Trust};
use proto::chatkord_v1::ChannelsList;
use proto::video::Video;
use screens::{RoomEvent, Track};
use std::time::{Duration, Instant};
use terms::{MemberId, Secret, ServerUrl};
use testserver::sound::Lab;
use tier::skipped;

/// The server's address.
const URL: &str = "CHATKORD_FACE_URL";
/// The account this member signs in as, and its password, read from the environment alone.
const NAME: &str = "CHATKORD_FACE_NAME";
const PASSWORD: &str = "CHATKORD_FACE_PASSWORD";
/// The voice channel the other machine's call is in, by its name.
const CHANNEL: &str = "CHATKORD_FACE_CHANNEL";
/// How long the claps are listened for.
const LISTEN: Duration = Duration::from_secs(40);
/// How far from a sound's start its picture is looked for.
const NEAR: Duration = Duration::from_millis(500);
/// How often the recording's length is set against the clock.
const ANCHOR_EVERY: Duration = Duration::from_millis(10);

/// A picture's arrival and how much its luma changed from the one before, as the mean absolute difference of every
/// eighth sample.
fn change(before: &[u8], now: &[u8]) -> f64 {
    let pairs = before.iter().zip(now).step_by(8);
    let (sum, count) = pairs.fold((0_u64, 0_u64), |(sum, count), (a, b)| (sum + u64::from(a.abs_diff(*b)), count + 1));
    if count == 0 { 0.0 } else { sum as f64 / count as f64 }
}

/// The offset of each clap, in milliseconds: each sound's start (`heard`, in seconds on the shared clock) less the
/// time of the picture that changed most within [`NEAR`] of it, among `pictures` (time, change). A sound with no
/// picture near it is left out.
fn offsets(heard: &[f64], pictures: &[(f64, f64)]) -> Vec<f64> {
    let near = NEAR.as_secs_f64();
    heard
        .iter()
        .filter_map(|&sound| {
            let peak =
                pictures.iter().filter(|(at, _)| (at - sound).abs() <= near).max_by(|(_, a), (_, b)| a.total_cmp(b))?;
            Some((sound - peak.0) * 1000.0)
        })
        .collect()
}

/// Whose camera, not `me`'s, a report says is on: one that starts, or one already on when the room was joined.
fn camera_of(report: &Report, me: &MemberId) -> Option<MemberId> {
    match report {
        Report::Room(RoomEvent::Started { track: Track::Camera, user_id }) if user_id != me => Some(user_id.clone()),
        Report::Room(RoomEvent::Joined { streams, .. }) => streams
            .iter()
            .find(|(user_id, track)| *track == Track::Camera && user_id != me)
            .map(|(user_id, _)| user_id.clone()),
        _ => None,
    }
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "a measurement with a person clapping, run on request"]
    async fn each_clap_s_sound_against_its_picture() {
        let (Ok(url), Ok(name), Ok(password), Ok(channel)) =
            (std::env::var(URL), std::env::var(NAME), std::env::var(PASSWORD), std::env::var(CHANNEL))
        else {
            return skipped("no server named to listen on");
        };
        let Some(lab) = Lab::take() else { return skipped("no lock for the virtual sound devices") };
        let Some(sound) = lab.make("face") else { return skipped("the sound server makes no virtual devices") };
        let devices = common::engine().devices().expect("the sound module");
        let speaker = webrtc::virtual_sound::listed(&devices.outputs(), &sound.speaker).expect("the virtual speaker");
        drop(devices);

        let url = ServerUrl::parse(&url).expect("a server's address");
        let (link, events) = Link::connect(&url, &Trust::system(), "e2e", "face").await.expect("connected");
        let ack = link.sign_in_with_password(&name, &Secret::new(password)).await.expect("signed in");
        let Ok(channels) = link.ask(ChannelsList {}).await else { panic!("no channels") };
        let found = channels.channels.iter().find(|each| each.name == channel).expect("the channel named");
        let me = MemberId::parse(&ack.user_id).expect("the server names the account");
        let channel_id = found.channel_id.clone();
        let picked = choices::Picked { label: speaker.clone(), id: speaker };
        let running = start_hearing_on(link, events, channel_id, me.clone(), Some(picked));

        running
            .until("another member's camera", Duration::from_secs(60), |report| camera_of(report, &me).is_some())
            .await;
        let from =
            running.reports.lock().unwrap().iter().find_map(|report| camera_of(report, &me)).expect("a camera on");
        let slot = running.picture(&from, Video::Camera);
        // Pictures in memory, so each one's luma is read here.
        slot.refuse_imports();
        let (woke, mut woken) = tokio::sync::mpsc::unbounded_channel();
        slot.wake_with(move || {
            let _ = woke.send(());
        });
        let mut anchor = tokio::time::interval(ANCHOR_EVERY);

        let recording = sound.record(Duration::from_secs(5)).await.expect("the speaker is recorded");
        let began = Instant::now();
        println!("listening for {LISTEN:?}: clap in front of the other machine's camera, two seconds apart");
        let mut anchors: Vec<(f64, usize)> = Vec::new();
        let mut pictures: Vec<(f64, f64)> = Vec::new();
        let mut before: Option<Vec<u8>> = None;
        let until = tokio::time::Instant::from_std(began + LISTEN);
        // Woken by each picture the slot is given and at each anchor's time, until the listening is over.
        loop {
            let woke = async {
                tokio::select! {
                    _ = woken.recv() => {}
                    _ = anchor.tick() => {}
                }
            };
            if tokio::time::timeout_at(until, woke).await.is_err() {
                break;
            }
            if let Some((_, luma, _)) = common::luma_in(&slot) {
                let at = began.elapsed().as_secs_f64();
                if let Some(before) = &before {
                    pictures.push((at, change(before, &luma)));
                }
                before = Some(luma);
            }
            let now = began.elapsed().as_secs_f64();
            if anchors.last().is_none_or(|(at, _)| now - at >= ANCHOR_EVERY.as_secs_f64()) {
                anchors.push((now, recording.taken()));
            }
        }
        let samples = recording.finish();
        // A sample's time is the earliest any anchor allows: the recorder writes late, never early.
        let start = anchors.iter().map(|(at, taken)| at - *taken as f64 / RATE_HZ as f64).fold(f64::MAX, f64::min);
        let heard: Vec<f64> =
            onsets(samples.iter().copied()).iter().map(|at| start + *at as f64 / RATE_HZ as f64).collect();
        let offsets = offsets(&heard, &pictures);
        running.leave().await;
        assert!(!pictures.is_empty(), "pictures arrived in memory");
        let loudest = samples.iter().map(|sample| sample.saturating_abs()).max().unwrap_or(0);
        assert!(
            !offsets.is_empty(),
            "a clap was heard and seen: {} sounds (the loudest sample {loudest}, a sound past {LOUD}), {} pictures",
            heard.len(),
            pictures.len()
        );
        let (low, high) =
            offsets.iter().fold((f64::MAX, f64::MIN), |(low, high), &each| (low.min(each), high.max(each)));
        println!(
            "face to voice over {} claps: median {:.1} ms (voice later when positive), from {low:.1} to {high:.1} ms",
            offsets.len(),
            median(offsets.clone())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clap_is_its_sound_less_its_most_changed_picture_near_it() {
        let pictures = [(0.95, 2.0), (1.0, 40.0), (1.05, 3.0), (3.0, 1.0), (4.9, 35.0)];
        let found = offsets(&[1.12, 5.0, 8.0], &pictures);
        assert_eq!(found.len(), 2, "the third sound has no picture near it");
        assert!((found[0] - 120.0).abs() < 1e-6 && (found[1] - 100.0).abs() < 1e-6, "{found:?}");
    }

    #[test]
    fn a_change_is_the_mean_difference_of_every_eighth_sample() {
        let before = vec![10_u8; 64];
        let mut now = before.clone();
        now[0] = 90;
        assert!((change(&before, &now) - 10.0).abs() < 1e-9, "one of eight samples moved by 80");
        assert!(change(&[], &[]).abs() < f64::EPSILON);
    }

    #[test]
    fn a_camera_already_on_at_the_join_is_found_as_one_that_starts_is_and_never_one_s_own() {
        let member = |id: &str| MemberId::parse(id).unwrap();
        let (ada_id, bea_id) = (member("ada"), member("bea"));
        let joined = |streams: Vec<(MemberId, Track)>| Report::Room(RoomEvent::Joined { members: Vec::new(), streams });
        let ada = joined(vec![(ada_id.clone(), Track::Microphone), (ada_id.clone(), Track::Camera)]);
        assert_eq!(camera_of(&ada, &bea_id), Some(ada_id.clone()), "on before the join");
        let started = Report::Room(RoomEvent::Started { user_id: ada_id.clone(), track: Track::Camera });
        assert_eq!(camera_of(&started, &bea_id), Some(ada_id.clone()), "turned on after it");
        assert_eq!(camera_of(&ada, &ada_id), None, "one's own");
        assert_eq!(camera_of(&joined(vec![(ada_id, Track::Microphone)]), &bea_id), None, "a voice alone");
    }
}
