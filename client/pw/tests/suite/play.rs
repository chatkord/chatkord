//! A sound played through the sound server reaches the speaker asked for, whole, and says when it has drained.

use std::sync::Arc;
use std::time::{Duration, Instant};

use testserver::sound::Lab;
use tier::skipped;

const RATE: u32 = 48_000;
const WAIT: Duration = Duration::from_secs(10);

/// A quarter second of 660 Hz at half of full scale.
fn tone() -> Arc<[f32]> {
    #[expect(clippy::cast_precision_loss, reason = "a quarter second of samples")]
    (0..RATE / 4).map(|n| 0.5 * (std::f32::consts::TAU * 660.0 * n as f32 / RATE as f32).sin()).collect()
}

mod lab {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_sound_is_heard_on_the_speaker_asked_for_and_drains() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("play") else { return skipped("no sound server to make a speaker on") };
        let play = match pw::Play::open() {
            Ok(play) => play,
            Err(why) => return skipped(why.to_string()),
        };
        let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
        let playing = play.start(tone(), RATE, Some(&sound.sink)).expect("a sound starts on a speaker there is");
        let started = Instant::now();
        while !playing.drained() {
            assert!(started.elapsed() < WAIT, "a quarter second drains within {WAIT:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let drained = started.elapsed();
        drop(playing);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let heard = recording.finish();
        let loud = |sample: &&i16| sample.unsigned_abs() > 300;
        let span = heard.iter().position(|sample| loud(&sample)).zip(heard.iter().rposition(|sample| loud(&sample)));
        let heard_for = span.map_or(0, |(first, last)| last + 1 - first);
        println!("drained after {drained:?}, heard for {heard_for} samples of {}", RATE / 4);
        assert!(drained >= Duration::from_millis(250), "not drained before it could have played: {drained:?}");
        let whole = (RATE / 4) as usize;
        assert!(
            heard_for.abs_diff(whole) < whole / 50,
            "the tone came out of the speaker asked for, whole: {heard_for}"
        );
    }
}
