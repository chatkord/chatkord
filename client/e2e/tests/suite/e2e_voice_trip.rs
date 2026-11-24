//! How long a voice takes from one member's microphone to another's speaker, and what the two members and the sound
//! server spend on it: a measurement whose numbers go in the commit that changes the voice's path, and a lab test that
//! nine in ten bursts, each after half a second of exact silence, reach the speaker.
//! With `CHATKORD_TRIP_CAMERA` set, the member who speaks also sends a camera of made-up pictures at 720p30, encoded
//! as a call's camera is, and the other watches it, so the voice shares its connection with a camera's.
//! The speaker runs in a process of its own, as a member does, so no echo canceller hears its own voice played back.

use crate::common;

use common::onsets::{RATE_HZ, median, onsets};
use common::{Server, join_voice, voice_of};
use proto::chatkord_v1::TrackKind;
use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use testserver::sound::Lab;
use tier::skipped;
use webrtc::virtual_sound::listed;

const EVERY: Duration = Duration::from_millis(500);
const BURST: Duration = Duration::from_millis(30);
const STEADY: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(3);
/// How often the receiver's mix is read for a burst: finer than a burst and the quiet between two.
const MIX_LOOKS: Duration = Duration::from_millis(5);

const URL: &str = "CHATKORD_TRIP_URL";
const INVITE: &str = "CHATKORD_TRIP_INVITE";
const MICROPHONE: &str = "CHATKORD_TRIP_MICROPHONE";
const CAMERA: &str = "CHATKORD_TRIP_CAMERA";
/// The made-up camera's picture and rate: a call's camera as it starts.
const CAMERA_MODE: (u32, u32, u32) = (1280, 720, 30);

/// Sends a camera of made-up pictures over `voice` until the returned flag is set: a gradient that moves a step each
/// picture, encoded on the first encoder that takes pictures in memory, at a camera's own rates.
async fn made_up_camera(
    voice: &webrtc::Voice,
    link: &link::Link,
) -> (webrtc::SentVideo, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (width, height, rate) = CAMERA_MODE;
    let (sent, feeder) = voice.camera(link, webrtc::Codec::H264, width, height).await.expect("a camera track");
    let envelope = net::Envelope::for_camera(width, height, rate, None);
    sent.set_rates(envelope.min, envelope.start, envelope.max).expect("its rates");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopping = std::sync::Arc::clone(&stop);
    std::thread::spawn(move || {
        let settings = media::Settings {
            width: width as usize,
            height: height as usize,
            frames_per_second: rate,
            bits_per_second: u32::try_from(envelope.start).unwrap_or(u32::MAX),
            key_frame_every: None,
        };
        let report = media::survey();
        let mut session = report
            .memory_order(media::Codec::H264)
            .find_map(|encoder| media::Session::open(encoder, settings).ok())
            .expect("an encoder takes pictures in memory");
        let (w, h) = (width as usize, height as usize);
        let mut made = 0_usize;
        let begun = Instant::now();
        while !stopping.load(Ordering::Relaxed) {
            let mut data = vec![128_u8; w * h * 3 / 2];
            for (at, luma) in data[..w * h].iter_mut().enumerate() {
                *luma = u8::try_from((at % w + made) % 220 + 16).unwrap();
            }
            let picture = media::Nv12::new(w, h, data).unwrap();
            // A viewer who joins later asks for a key frame to start from, as the call's camera answers.
            if feeder.key_frame_wanted() {
                session.request_key_frame();
            }
            for unit in session.encode(&picture).expect("encoded") {
                feeder.push(&unit.data, unit.key_frame, width, height, unit.captured);
            }
            made += 1;
            let next = begun + Duration::from_secs(1) / rate * u32::try_from(made).unwrap();
            crate::clock::frame_clock(next.saturating_duration_since(Instant::now()));
        }
    });
    (sent, stop)
}

/// The time from each burst said into the microphone (the left channel) to its first sound at the speaker (the
/// right), in milliseconds, for every burst heard within one burst period.
fn trips(stereo: &[i16]) -> Vec<f64> {
    let said = onsets(stereo.iter().step_by(2).copied());
    let heard = onsets(stereo.iter().skip(1).step_by(2).copied());
    let period = RATE_HZ * EVERY.as_millis() as usize / 1000;
    said.iter()
        .filter_map(|&at| heard.iter().find(|&&then| then > at && then - at < period).map(|then| then - at))
        .map(|samples| samples as f64 * 1000.0 / RATE_HZ as f64)
        .collect()
}

/// The places, among the bursts said, of those not heard within one burst period, each with the loudest sample the
/// speaker played in that period.
fn missed(stereo: &[i16]) -> Vec<(usize, i16)> {
    let said = onsets(stereo.iter().step_by(2).copied());
    let heard = onsets(stereo.iter().skip(1).step_by(2).copied());
    let period = RATE_HZ * EVERY.as_millis() as usize / 1000;
    said.iter()
        .enumerate()
        .filter(|(_, at)| !heard.iter().any(|then| *then > **at && *then - **at < period))
        .map(|(place, at)| {
            let played = stereo.iter().skip(1).step_by(2).skip(*at).take(period);
            (place, played.map(|sample| sample.saturating_abs()).max().unwrap_or(0))
        })
        .collect()
}

/// A process's user and system time, in clock ticks.
fn ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let Some((_, after)) = stat.rsplit_once(')') else { return 0 };
    let fields: Vec<&str> = after.split_whitespace().collect();
    // Counted from the state after the name: utime is the 12th, stime the 13th.
    let field = |n: usize| fields.get(n).and_then(|field| field.parse::<u64>().ok()).unwrap_or(0);
    field(11) + field(12)
}

fn clock_ticks() -> f64 {
    let out = Command::new("getconf").arg("CLK_TCK").output().expect("getconf");
    String::from_utf8_lossy(&out.stdout).trim().parse().expect("a number")
}

/// This user's sound server processes, by name and id.
fn sound_server() -> Vec<(String, u32)> {
    let uid = std::fs::read_to_string("/proc/self/loginuid").unwrap_or_default();
    let entries = std::fs::read_dir("/proc").into_iter().flatten().flatten();
    entries
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?.trim().to_string();
            let own = std::fs::read_to_string(entry.path().join("loginuid")).ok()? == uid;
            (own && ["pipewire", "pipewire-pulse", "wireplumber"].contains(&comm.as_str())).then_some((comm, pid))
        })
        .collect()
}

/// Records the microphone's node into the left channel and what the speaker plays into the right, on one stream, so
/// both are on one clock.
fn record(file: &Path, microphone: &str, speaker: &str) -> Child {
    let name = format!("chatkord_trip_{}", std::process::id());
    let child = Command::new("pw-record")
        .args(["--target", "0", "-P", &format!("{{ node.name = {name} }}")])
        .args(["--rate", "48000", "--channels", "2", "--format", "s16"])
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("pw-record");
    let deadline = Instant::now() + Duration::from_secs(5);
    for (from, to) in
        [(format!("{microphone}:capture_MONO"), "input_FL"), (format!("{speaker}:monitor_MONO"), "input_FR")]
    {
        while !Command::new("pw-link").args([&from, &format!("{name}:{to}")]).status().is_ok_and(|s| s.success()) {
            assert!(Instant::now() < deadline, "{from} is linked to the recorder");
            crate::common::retry_beat(Duration::from_millis(50));
        }
    }
    child
}

/// The interleaved samples of a 16-bit WAV, read to its end.
fn samples(wav: &[u8]) -> Vec<i16> {
    let Some(start) = wav.windows(4).position(|window| window == b"data") else { return Vec::new() };
    wav[(start + 8).min(wav.len())..].as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect()
}

mod lab {
    use super::*;

    /// The member who speaks, started by the test below in a process of its own; run by hand without its
    /// variables, it does nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "started by the measurement below, never on its own"]
    async fn the_member_who_speaks_into_the_virtual_microphone() {
        let (Ok(url), Ok(invite), Ok(microphone)) =
            (std::env::var(URL), std::env::var(INVITE), std::env::var(MICROPHONE))
        else {
            return;
        };
        let (ada, _events, joined) = join_voice(&url, "ada", &invite).await;
        let (voice, _hearing) = voice_of(&joined);
        let devices = voice.devices().expect("the sound module");
        let id = listed(&devices.inputs(), &microphone).expect("the virtual microphone is listed");
        devices.choose_input(Some(&id)).expect("the virtual microphone is chosen");
        let _speaking = voice.speak(&ada).await.expect("the microphone is sent");
        let _camera = if std::env::var(CAMERA).is_ok() { Some(made_up_camera(&voice, &ada).await) } else { None };
        println!("speaking");
        // Until the measurement that started it is done with it: its end closes this input.
        let _ = tokio::task::spawn_blocking(|| std::io::copy(&mut std::io::stdin(), &mut std::io::sink())).await;
    }

    // Hearing nine bursts in ten is this machine keeping up in real time: the test runs with nothing beside it.
    mod timing {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_voice_from_one_microphone_to_another_members_speaker() {
            let Some(lab) = Lab::take() else { return skipped("no lock for the virtual sound devices") };
            let Some(mut sound) = lab.make("trip") else { return skipped("the sound server makes no virtual devices") };
            let (server, invites) = Server::start("e2e", 2).await;
            let per_second = clock_ticks();

            let mut ada = Command::new(std::env::current_exe().expect("this test"))
                .args([
                    "--exact",
                    "e2e_voice_trip::lab::the_member_who_speaks_into_the_virtual_microphone",
                    "--ignored",
                    "--nocapture",
                ])
                .env(URL, server.url())
                .env(INVITE, &invites[0])
                .env(MICROPHONE, &sound.microphone)
                .envs(std::env::var(CAMERA).ok().map(|on| (CAMERA, on)))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("the test binary again");
            let mut said = std::io::BufReader::new(ada.stdout.take().expect("its output")).lines();
            while said.next().expect("ada says it speaks").expect("a line") != "speaking" {}

            let (bo, _events, state) = join_voice(&server.url(), "bo", &invites[1]).await;
            let (mut voice, mut hearing) = voice_of(&state);
            let devices = voice.devices().expect("the sound module");
            let speaker = listed(&devices.outputs(), &sound.speaker).expect("the virtual speaker is listed");
            devices.choose_output(Some(&speaker)).expect("the virtual speaker is chosen");
            voice.play().expect("the speaker plays");
            let ada_id = state.streams.first().expect("ada's stream").user_id.clone();
            hearing.take(&bo, &ada_id, TrackKind::Microphone).await.expect("bo takes ada");
            let pictures = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            if std::env::var(CAMERA).is_ok() {
                let counted = std::sync::Arc::clone(&pictures);
                let seen = move |_: webrtc::Picture<'_>| {
                    counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                };
                hearing.watch(&bo, &ada_id, proto::video::Video::Camera, seen).await.expect("bo watches ada's camera");
            }

            let file = std::env::temp_dir().join(format!("chatkord_trip_{}.wav", std::process::id()));
            let recorder = record(&file, &sound.source, &sound.sink);
            sound.pulse(EVERY, BURST);
            let mut looks = crate::common::looks::Looks::every(MIX_LOOKS);
            looks.over(SETTLE).await;
            let (servers, began) = (sound_server(), Instant::now());
            let before: Vec<u64> = servers.iter().map(|(_, pid)| ticks(*pid)).collect();
            let (bo_before, ada_before) = (ticks(std::process::id()), ticks(ada.id()));
            // Where the bursts are lost: the receiver's mix, read every look, says whether each reached it.
            let (mut loud, mut mixed, mut quiet_since) = (false, 0_usize, Instant::now());
            while began.elapsed() < STEADY {
                looks.next().await;
                let level = voice.levels().into_iter().find(|(user, _)| *user == ada_id).map_or(0, |(_, level)| level);
                if level >= 6 {
                    if !loud && quiet_since.elapsed() >= Duration::from_millis(200) {
                        mixed += 1;
                    }
                    loud = true;
                } else {
                    if loud {
                        quiet_since = Instant::now();
                    }
                    loud = false;
                }
            }
            eprintln!("bursts reaching bo's mix over {STEADY:?}: {mixed}");
            let seconds = began.elapsed().as_secs_f64();
            let share = |spent: u64| spent as f64 / per_second / seconds * 100.0;
            eprintln!("ada, speaking: {:.1} % of a core", share(ticks(ada.id()) - ada_before));
            eprintln!("bo, hearing and playing: {:.1} % of a core", share(ticks(std::process::id()) - bo_before));
            for ((name, pid), before) in servers.iter().zip(before) {
                eprintln!("{name}: {:.1} % of a core", share(ticks(*pid) - before));
            }

            let _ = Command::new("kill").args(["-INT", &recorder.id().to_string()]).status();
            let _ = recorder.wait_with_output();
            let _ = ada.kill();
            let _ = ada.wait();
            let recorded = samples(&std::fs::read(&file).unwrap_or_default());
            let _ = std::fs::remove_file(&file);
            let trips = trips(&recorded);
            let said = onsets(recorded.iter().step_by(2).copied()).len();
            assert!(said > 0, "the bursts said into the microphone were recorded");
            let missed = missed(&recorded);
            assert!(
                trips.len() * 10 >= said * 9,
                "at least nine in ten of {said} bursts were heard, not {}; missed, by place: {missed:?}",
                trips.len()
            );
            let (low, high) = trips.iter().fold((f64::MAX, 0.0_f64), |(low, high), &t| (low.min(t), high.max(t)));
            eprintln!(
                "round trip over {} bursts: median {:.1} ms, from {low:.1} to {high:.1} ms",
                trips.len(),
                median(trips.clone())
            );
            if std::env::var(CAMERA).is_ok() {
                let shown = pictures.load(std::sync::atomic::Ordering::Relaxed);
                eprintln!("with a camera on: bo decoded {shown} of ada's pictures");
                assert!(shown > 0, "the camera reached bo, so the voice shared its connection with it");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_is_found_where_it_starts_and_its_trip_is_the_gap_to_the_other_channel() {
        let mut stereo = vec![0_i16; 2 * RATE_HZ];
        for n in 0..480 {
            stereo[2 * (12_000 + n)] = 9_000;
            stereo[2 * (12_000 + 2_400 + n) + 1] = 9_000;
        }
        assert_eq!(onsets(stereo.iter().step_by(2).copied()), vec![12_000]);
        assert_eq!(trips(&stereo), vec![50.0], "2400 samples later is 50 ms");
    }
}
