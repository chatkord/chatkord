//! A call's devices as their owner keeps them: the microphone's frames arrive whole, the speaker plays what it is
//! sent on a clock of the sender's, and a device chosen anew takes over once it runs, the sound going on meanwhile.

use std::thread;
use std::time::{Duration, Instant};

use sound::voice::{FRAME, Health, Microphone, Speaker, Stereo, Voiced};
use testserver::sound::Lab;
use tier::skipped;

const WAIT: Duration = Duration::from_secs(10);

/// 10 ms of a 440 Hz sine at a third of full scale, in both channels, the frame `index` of it.
fn tone(index: u32) -> Stereo {
    let mut frame = [0; 2 * FRAME];
    for (n, pair) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let t = (f64::from(index) * FRAME as f64 + n as f64) / 48_000.0;
        #[expect(clippy::cast_possible_truncation, reason = "a sine within i16")]
        let sample = (10_000.0 * (std::f64::consts::TAU * 440.0 * t).sin()) as i16;
        *pair = [sample, sample];
    }
    frame
}

/// Sends `speaker` a tone on a clock of this thread's for `seconds`, keeping it as the chain would.
fn send(speaker: &mut Speaker, voiced: &mut Voiced, from: u32, seconds: u32) -> u32 {
    let started = Instant::now();
    let frames = seconds * 100;
    for index in from..from + frames {
        voiced.play(&tone(index));
        speaker.keep(Instant::now());
        let due = started + Duration::from_millis(10 * u64::from(index - from + 1));
        thread::sleep(due.saturating_duration_since(Instant::now()));
    }
    from + frames
}

mod lab {
    use super::*;

    #[test]
    fn the_microphone_hands_over_its_sound_in_whole_frames_and_wakes_its_reader() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(mut sound) = lab.make("media_mic") else { return skipped("no sound server to make devices on") };
        let (mut microphone, mut frames) = match Microphone::open(Some(&sound.source), Some(thread::current())) {
            Ok(opened) => opened,
            Err(why) => return skipped(why.to_string()),
        };
        sound.speak();
        let (started, mut loudest, mut count) = (Instant::now(), 0_i16, 0);
        while loudest < 6_000 {
            assert!(started.elapsed() < WAIT, "the tone arrives within {WAIT:?}");
            thread::park_timeout(Duration::from_millis(100));
            microphone.keep(Instant::now());
            while let Some(frame) = frames.frame() {
                count += 1;
                loudest = loudest.max(frame.iter().map(|sample| sample.saturating_abs()).max().unwrap_or(0));
            }
        }
        assert_eq!(microphone.health(), Health::Running);
        assert_eq!(microphone.dropped(), 0, "a reader that keeps up loses nothing");
        println!("{count} frames, delay {:?}", frames.delay());
    }

    #[test]
    fn the_speaker_plays_a_sender_s_clock_without_running_out_or_over() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let Some(sound) = lab.make("media_speaker") else { return skipped("no sound server to make devices on") };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let Some(recording) = runtime.block_on(sound.record(WAIT)) else {
            return skipped("the speaker cannot be recorded");
        };
        let (mut speaker, mut voiced) = match Speaker::open(Some(&sound.sink), Instant::now()) {
            Ok(opened) => opened,
            Err(why) => return skipped(why.to_string()),
        };
        send(&mut speaker, &mut voiced, 0, 6);
        assert_eq!(speaker.health(), Health::Running);
        let (dropped, underruns) = speaker.losses();
        println!("dropped {dropped}, ran out {underruns} times, delay {:?}", voiced.delay());
        assert_eq!(dropped, 0, "frames sent at the speaker's pace never fill its ring");
        assert!(underruns <= 1, "it runs out at most once, before its margin has first filled: {underruns}");
        let heard = recording.stop();
        assert!(heard > 6_000, "the tone was played on the speaker asked for, loudest {heard}");
    }

    #[test]
    fn a_speaker_chosen_anew_takes_over_once_it_runs_and_the_sound_goes_on() {
        let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
        let (Some(first), Some(second)) = (lab.make("media_first"), lab.make("media_second")) else {
            return skipped("no sound server to make devices on");
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (Some(on_first), Some(on_second)) =
            (runtime.block_on(first.record(WAIT)), runtime.block_on(second.record(WAIT)))
        else {
            return skipped("the speakers cannot be recorded");
        };
        let (mut speaker, mut voiced) = match Speaker::open(Some(&first.sink), Instant::now()) {
            Ok(opened) => opened,
            Err(why) => return skipped(why.to_string()),
        };
        let sent = send(&mut speaker, &mut voiced, 0, 2);
        speaker.choose(Some(&second.sink)).expect("the second speaker opens");
        let switched = on_first.taken();
        send(&mut speaker, &mut voiced, sent, 2);
        let (dropped, _) = speaker.losses();
        assert_eq!(dropped, 0);
        let heard_first = on_first.finish();
        let after = heard_first.get(switched + 48_000 / 2..).unwrap_or_default();
        assert!(heard_first.iter().any(|sample| *sample > 6_000), "the first speaker played until the switch");
        assert!(after.iter().all(|sample| sample.saturating_abs() < 300), "and nothing half a second after it");
        assert!(on_second.stop() > 6_000, "the second took over");
    }
}
