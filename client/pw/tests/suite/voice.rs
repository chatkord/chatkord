//! A call's microphone and speaker are the devices asked for by name, whatever else comes or goes: each stream is
//! linked to that node, its sound arrives or plays, and a named device that goes stops its stream.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use testserver::sound::Lab;
use tier::skipped;

const RATE: u32 = 48_000;
const WAIT: Duration = Duration::from_secs(10);

/// Whether the graph links a port of `from` to a port of `to`, as `pw-link` lists the links.
fn linked(from: &str, to: &str) -> bool {
    let out = Command::new("pw-link").arg("-l").output().map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
    let mut source = None;
    for line in out.unwrap_or_default().lines() {
        if let Some(port) = line.trim_start().strip_prefix("|-> ") {
            if source.is_some_and(|source: &str| source.starts_with(&format!("{from}:")))
                && port.starts_with(&format!("{to}:"))
            {
                return true;
            }
        } else if !line.trim_start().starts_with("|<-") {
            source = Some(line.trim());
        }
    }
    false
}

async fn until(what: &str, holds: impl Fn() -> bool) {
    let started = Instant::now();
    while !holds() {
        assert!(started.elapsed() < WAIT, "{what} within {WAIT:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_microphone_asked_for_by_name_is_the_one_heard() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(mut sound) = lab.make("voice_mic") else { return skipped("no sound server to make devices on") };
        let voice = match pw::Voice::open() {
            Ok(voice) => voice,
            Err(why) => return skipped(why.to_string()),
        };
        let (peak, samples) = (Arc::new(AtomicI32::new(0)), Arc::new(AtomicU32::new(0)));
        let (hearing, counting) = (Arc::clone(&peak), Arc::clone(&samples));
        let microphone = voice
            .microphone(Some(&sound.source), RATE, move |heard| {
                let loudest = heard.iter().map(|sample| i32::from(*sample).abs()).max().unwrap_or(0);
                hearing.fetch_max(loudest, Ordering::Relaxed);
                counting.fetch_add(u32::try_from(heard.len()).unwrap_or(0), Ordering::Relaxed);
            })
            .expect("the microphone opens");
        sound.speak();
        until("the tone is heard", || peak.load(Ordering::Relaxed) > 6_000).await;
        assert_eq!(microphone.state(), pw::State::Streaming);
        assert!(linked(&sound.source, "chatkord-microphone"), "the stream is linked to the node asked for");
        let timing = microphone.timing(RATE).expect("a streaming microphone has a time");
        println!("heard {} samples, delay {:?}", samples.load(Ordering::Relaxed), timing.delay);
        assert!(timing.delay < Duration::from_millis(200), "the delay is a graph's, not a stall: {timing:?}");
        assert!(!voice.lost());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_speaker_asked_for_by_name_plays_and_follows_a_rate() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("voice_speaker") else { return skipped("no sound server to make devices on") };
        let voice = match pw::Voice::open() {
            Ok(voice) => voice,
            Err(why) => return skipped(why.to_string()),
        };
        let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
        let mut at = 0_u32;
        let speaker = voice
            .speaker(Some(&sound.sink), RATE, move |samples| {
                for pair in samples.as_chunks_mut::<2>().0 {
                    let t = f64::from(at) / f64::from(RATE);
                    #[expect(clippy::cast_possible_truncation, reason = "a sine at a third of full scale")]
                    let sample = (10_000.0 * (std::f64::consts::TAU * 440.0 * t).sin()) as i16;
                    *pair = [sample, sample];
                    at = at.wrapping_add(1);
                }
            })
            .expect("the speaker opens");
        until("the speaker streams", || speaker.state() == pw::State::Streaming).await;
        assert!(linked("chatkord-speaker", &sound.sink), "the stream is linked to the node asked for");
        recording.heard(6_000, WAIT).await.expect("the tone reached the speaker asked for");
        speaker.set_rate(1.001).expect("a streaming speaker takes a rate");
        let timing = speaker.timing(RATE).expect("a streaming speaker has a time");
        println!("delay {:?}", timing.delay);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(speaker.state(), pw::State::Streaming, "a new rate leaves it playing");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_named_speaker_that_goes_stops_its_stream_rather_than_moving_it() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("voice_gone") else { return skipped("no sound server to make devices on") };
        let voice = match pw::Voice::open() {
            Ok(voice) => voice,
            Err(why) => return skipped(why.to_string()),
        };
        let speaker = voice.speaker(Some(&sound.sink), RATE, |samples| samples.fill(0)).expect("the speaker opens");
        until("the speaker streams", || speaker.state() == pw::State::Streaming).await;
        let sink = sound.sink.clone();
        drop(sound);
        until("the stream stops", || speaker.state() != pw::State::Streaming).await;
        println!("gone, the stream is {:?}", speaker.state());
        assert!(!linked("chatkord-speaker", &sink));
        assert!(!voice.lost(), "a device going leaves the connection");
    }
}
