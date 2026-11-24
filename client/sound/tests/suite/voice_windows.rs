//! A call's microphone and speaker, and the window's sounds, on WASAPI, on this machine's own endpoints: each listed
//! by its endpoint id and named from its property store, opened by that id, the microphone handing over whole frames,
//! the speaker kept to a sender's clock, a sound played whole. What is played is heard through the loopback of the
//! process that plays it, a child of this test's binary, since a loopback never hears its own process. Nothing here
//! prints a device's name or id.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sound::share::{Capture, Hears, RATE};
use sound::voice::{FRAME, Health, Microphone, Speaker, Stereo, VoiceError, Voiced};
use tier::skipped;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// The speaker the child plays on, by endpoint id, and for how many seconds.
const PLAY_ON: &str = "CHATKORD_TEST_PLAY_ON";
const PLAY_FOR: &str = "CHATKORD_TEST_PLAY_FOR";
/// A quiet 440 Hz tone: heard on the loopback whatever the endpoint's volume, not loud in the room.
const HZ: f64 = 440.0;
const LEVEL: f64 = 3_000.0;
/// How far a speaker's delay may move over ten minutes while its rate follows the sender: one 10 ms packet, twice.
const DRIFT: Duration = Duration::from_millis(20);

/// 10 ms of the tone in both channels, the frame `index` of it.
fn tone(index: u32) -> Stereo {
    let mut frame = [0; 2 * FRAME];
    for (n, pair) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let t = (f64::from(index) * FRAME as f64 + n as f64) / f64::from(RATE);
        #[expect(clippy::cast_possible_truncation, reason = "a sine within i16")]
        let sample = (LEVEL * (std::f64::consts::TAU * HZ * t).sin()) as i16;
        *pair = [sample, sample];
    }
    frame
}

/// Sends `speaker` the tone on a clock of this thread's for `seconds`, keeping it as the chain would.
fn send(speaker: &mut Speaker, voiced: &mut Voiced, seconds: u32) {
    let started = Instant::now();
    for index in 0..seconds * 100 {
        voiced.play(&tone(index));
        speaker.keep(Instant::now());
        let due = started + Duration::from_millis(10 * u64::from(index + 1));
        thread::sleep(due.saturating_duration_since(Instant::now()));
    }
}

/// How strong the tone is in the left channel of interleaved `samples`, in sample units.
fn strength(samples: &[i16]) -> f64 {
    let left: Vec<f64> = samples.iter().step_by(2).map(|&sample| f64::from(sample)).collect();
    let (mut re, mut im) = (0.0, 0.0);
    for (i, sample) in left.iter().enumerate() {
        let phase = std::f64::consts::TAU * HZ * i as f64 / f64::from(RATE);
        re += sample * phase.cos();
        im += sample * phase.sin();
    }
    2.0 * re.hypot(im) / left.len().max(1) as f64
}

/// A quarter second of 660 Hz at half of full scale, as Linux's `pw::play` test plays.
const SOUND: std::ops::Range<u32> = 0..RATE / 4;

fn sound() -> std::sync::Arc<[f32]> {
    #[expect(clippy::cast_precision_loss, reason = "a quarter second of samples")]
    SOUND.map(|n| 0.5 * (std::f32::consts::TAU * 660.0 * n as f32 / RATE as f32).sin()).collect()
}

/// This crate's ignored test `player`, playing on the speaker `id` for `seconds`, as a process of its own.
fn player(player: &str, id: &str, seconds: u32) -> std::process::Child {
    Command::new(std::env::current_exe().expect("this test"))
        .args(["--exact", player, "--ignored", "--nocapture"])
        .env(PLAY_ON, id)
        .env(PLAY_FOR, seconds.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the test binary again")
}

/// What the player said once it was done: frames dropped and times it ran out.
fn losses(player: std::process::Child) -> (u64, u64) {
    let out = player.wait_with_output().expect("the player ended");
    assert!(out.status.success(), "the player failed");
    let said = BufReader::new(&out.stdout[..]).lines().map_while(Result::ok).find(|line| line.starts_with("played "));
    let said = said.expect("the player said what it played");
    let mut words = said.split(' ').skip(1).map(|word| word.parse().expect("a count"));
    (words.next().expect("dropped"), words.next().expect("ran out"))
}

/// The longest a player process may take to be heard, far above what it takes on a loaded machine.
const PLAYER_STARTS: Duration = Duration::from_secs(20);

/// How long `sound` waited for its player, and the samples it hears for `span` from the first, passing over the first
/// `settle` of them: what a test measures starts at the player's first sound, however long it took to start.
fn heard_from_first(sound: &mut Capture, settle: Duration, span: Duration) -> (Duration, Vec<i16>) {
    let waited = Instant::now();
    let first = loop {
        sound.keep(Instant::now());
        if let Some(chunk) = sound.chunks().next() {
            break chunk;
        }
        assert!(waited.elapsed() < PLAYER_STARTS, "the player was heard within {PLAYER_STARTS:?}");
        thread::sleep(Duration::from_millis(5));
    };
    let (started, mut heard) = (Instant::now(), Vec::new());
    if settle.is_zero() {
        heard.extend_from_slice(&first);
    }
    while started.elapsed() < span {
        sound.keep(Instant::now());
        for chunk in sound.chunks().by_ref() {
            if started.elapsed() > settle {
                heard.extend_from_slice(&chunk);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    (started.duration_since(waited), heard)
}

/// Thirty seconds of the tone as a WAV file, for a player that is not ours.
fn tone_file() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("chatkord-voice-tone-{}.wav", std::process::id()));
    let bytes = RATE * 30 * 4;
    let mut wav = Vec::new();
    for part in [&b"RIFF"[..], &(36 + bytes).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes()] {
        wav.extend_from_slice(part);
    }
    for part in [&2u16.to_le_bytes()[..], &RATE.to_le_bytes(), &(RATE * 4).to_le_bytes(), &4u16.to_le_bytes()] {
        wav.extend_from_slice(part);
    }
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for index in 0..30 * 100 {
        for sample in tone(index) {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }
    std::fs::write(&path, wav).expect("the tone is written");
    path
}

/// Another application playing `file` on the default output as any player does, ended when dropped.
struct Application(std::process::Child);

impl Application {
    fn play(file: &std::path::Path) -> Self {
        let play = format!("(New-Object Media.SoundPlayer '{}').PlaySync()", file.display());
        let child = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &play]).spawn();
        Self(child.expect("a player starts"))
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Whether Windows is set to leave other applications as they are during a call: `UserDuckingPreference` 3 (Sound,
/// Communications, "Do nothing"); absent, it lowers them by 80 %.
fn lowers_nothing() -> bool {
    let asked = Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Multimedia\Audio", "/v", "UserDuckingPreference"])
        .output();
    asked.is_ok_and(|asked| String::from_utf8_lossy(&asked.stdout).contains("0x3"))
}

/// The endpoints Windows lists, asked of the owner thread as everything else is.
fn listing() -> Result<sound::voice::Listing, VoiceError> {
    sound::Sound::shared().listing_blocking()
}

/// The default endpoint `pick` takes from the listing, or why there is none.
fn default(pick: impl FnOnce(sound::voice::Listing) -> Option<String>) -> Result<String, String> {
    tier::lab();
    let listing = listing().map_err(|why| why.to_string())?;
    pick(listing).ok_or_else(|| "this machine has no such default endpoint".to_string())
}

mod lab {
    use super::*;

    /// The speaker the tests below play on from a process of their own; run by hand without its variables, it says
    /// so and plays nothing.
    #[test]
    #[ignore = "started by the speaker tests below, never on its own"]
    fn the_player_the_speaker_tests_start() {
        let (Ok(id), Ok(seconds)) = (std::env::var(PLAY_ON), std::env::var(PLAY_FOR)) else {
            return skipped("the player runs only as the speaker tests start it");
        };
        // An empty id is the default, which the speaker follows as it moves.
        let device = Some(id.as_str()).filter(|id| !id.is_empty());
        let (mut speaker, mut voiced) = Speaker::open(device, Instant::now()).expect("the speaker opens");
        send(&mut speaker, &mut voiced, seconds.parse().expect("seconds"));
        assert_eq!(speaker.health(), Health::Running, "the speaker ran to the end");
        let (dropped, underruns) = speaker.losses();
        println!("played {dropped} {underruns}");
    }

    /// The window's sound the test below hears, played from a process of its own; run by hand without its variables,
    /// it says so and plays nothing.
    #[test]
    #[ignore = "started by the sound test below, never on its own"]
    fn the_sound_the_sound_test_starts() {
        let (Ok(id), Ok(seconds)) = (std::env::var(PLAY_ON), std::env::var(PLAY_FOR)) else {
            return skipped("the sound runs only as the sound test starts it");
        };
        let seconds: u32 = seconds.parse().expect("seconds");
        let stage = sound::speaker::Stage::enter().expect("the sound system opens");
        let mut sounds = sound::speaker::Chimes::new(&stage);
        // A loopback hands over a process's sound only while it plays: a speaker playing silence beside the sound
        // carries its last packets through, as the call's own speaker does while a sound plays.
        let (mut silence, mut voiced) =
            Speaker::open(Some(&id), Instant::now()).expect("the speaker opens by its endpoint id");
        let carrying = thread::spawn(move || {
            let started = Instant::now();
            for index in 0..(seconds + 1) * 100 {
                voiced.play(&[0; 2 * FRAME]);
                silence.keep(Instant::now());
                let due = started + Duration::from_millis(10 * u64::from(index + 1));
                thread::sleep(due.saturating_duration_since(Instant::now()));
            }
        });
        // Played a second in, once the test hears this process.
        thread::sleep(Duration::from_secs(1));
        sounds.play(sound(), Some(&id)).expect("the sound starts on the speaker asked for");
        carrying.join().expect("the silence played");
        println!("played 0 0");
    }

    #[test]
    fn every_endpoint_is_listed_by_its_id_and_named_from_its_property_store() {
        tier::lab();
        let listing = match listing() {
            Ok(listing) => listing,
            Err(why) => return skipped(why),
        };
        let devices: Vec<_> = listing.microphones.iter().chain(&listing.speakers).collect();
        if devices.is_empty() {
            return skipped("this machine lists no endpoint");
        }
        for device in &devices {
            assert!(device.id.starts_with("{0.0."), "an endpoint id, as MMDevice gives it");
            assert!(!device.label.is_empty() && device.label != device.id, "a name read from the property store");
        }
        for (default, listed) in
            [(&listing.default_microphone, &listing.microphones), (&listing.default_speaker, &listing.speakers)]
        {
            if let Some(default) = default {
                assert!(listed.iter().any(|device| &device.id == default), "the communications default is listed");
            }
        }
        println!("{} microphones, {} speakers", listing.microphones.len(), listing.speakers.len());
    }

    #[test]
    fn the_microphone_opened_by_its_endpoint_id_hands_over_whole_frames_and_wakes_its_reader() {
        let id = match default(|listing| listing.default_microphone) {
            Ok(id) => id,
            Err(why) => return skipped(why),
        };
        let (mut microphone, mut heard) = match Microphone::open(Some(&id), Some(thread::current())) {
            Ok(opened) => opened,
            Err(why) => return skipped(why.to_string()),
        };
        let (started, mut frames) = (Instant::now(), 0_u32);
        while started.elapsed() < Duration::from_secs(2) {
            thread::park_timeout(Duration::from_millis(100));
            microphone.keep(Instant::now());
            while heard.frame().is_some() {
                frames += 1;
            }
        }
        assert_eq!(microphone.health(), Health::Running);
        assert_eq!(microphone.dropped(), 0, "a reader that keeps up loses nothing");
        assert!(frames >= 150, "two seconds bring about 200 frames of 10 ms: {frames}");
        let delay = heard.delay();
        println!("{frames} frames in 2 s, delay {delay:?}");
        // The stream's own latency read 0 in shared mode; the packets' capture times say how late they come.
        assert!(delay > Duration::ZERO && delay < Duration::from_millis(200), "a delay measured: {delay:?}");
    }

    /// Windows lowers other applications while a communications stream plays (Sound, Communications); a call opts
    /// its streams out, as Discord's does, so what another application plays keeps its level beside it.
    #[test]
    fn a_call_on_the_default_devices_leaves_another_applications_sound_as_loud_as_it_was() {
        if let Err(why) = default(|listing| listing.default_speaker) {
            return skipped(why);
        }
        if lowers_nothing() {
            return skipped("Windows is set to lower nothing during a call (Sound, Communications)");
        }
        let file = tone_file();
        let application = Application::play(&file);
        let mut sound = Capture::open(Hears::Tree { roots: vec![application.0.id()], app_id: String::new() })
            .expect("the application's loopback opens");
        let alone = strength(&heard_from_first(&mut sound, Duration::from_millis(500), Duration::from_millis(1_500)).1);
        let microphone = Microphone::open(None, None).expect("the call's microphone opens on the default");
        let (mut speaker, mut voiced) =
            Speaker::open(None, Instant::now()).expect("the call's speaker opens on the default");
        let (started, mut beside) = (Instant::now(), Vec::new());
        // Windows lowers the others as the call's streams start, fading over a few hundred milliseconds.
        while started.elapsed() < Duration::from_secs(2) {
            voiced.play(&[0; 2 * FRAME]);
            speaker.keep(Instant::now());
            sound.keep(Instant::now());
            for chunk in sound.chunks().by_ref() {
                if started.elapsed() > Duration::from_secs(1) {
                    beside.extend_from_slice(&chunk);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        let beside = strength(&beside);
        drop((microphone, speaker, voiced, sound, application));
        let _ = std::fs::remove_file(&file);
        println!("the application's tone at {alone:.0} alone and {beside:.0} beside a call, of {LEVEL}");
        assert!(alone > LEVEL / 2.0, "the application was heard alone: {alone:.0}");
        assert!(beside > alone * 0.9, "and as loud beside a call: {beside:.0} of {alone:.0}");
    }

    /// A speaker that moves to the device chosen says so in the log, never naming it: a call gone silent then tells
    /// a speaker that never followed from one that followed and played nothing.
    #[test]
    fn a_speaker_that_moves_to_the_device_chosen_says_so_in_the_log() {
        let id = match default(|listing| listing.default_speaker) {
            Ok(id) => id,
            Err(why) => return skipped(why),
        };
        let ring = report::Ring::new(std::time::SystemTime::now);
        let moved = |ring: &report::Ring| {
            ring.lines().into_iter().find(|line| line.as_str().contains("a kept stream moved to the device chosen"))
        };
        let logging = tracing_subscriber::registry().with(ring.layer()).set_default();
        let (mut speaker, mut voiced) = Speaker::open(None, Instant::now()).expect("the speaker opens on the default");
        speaker.choose(Some(&id)).expect("the default speaker opens by its endpoint id");
        let started = Instant::now();
        while moved(&ring).is_none() && started.elapsed() < Duration::from_secs(2) {
            voiced.play(&[0; 2 * FRAME]);
            speaker.keep(Instant::now());
            thread::sleep(Duration::from_millis(10));
        }
        drop((speaker, voiced, logging));
        let line = moved(&ring).expect("the move was logged");
        let line = line.as_str();
        assert!(["end=speaker", "from=default", "to=chosen"].iter().all(|said| line.contains(said)), "{line}");
        assert!(!line.contains(&id), "the endpoint's id is never logged: {line}");
    }

    #[test]
    fn a_sound_is_heard_whole_on_the_speaker_asked_for() {
        let id = match default(|listing| listing.default_speaker) {
            Ok(id) => id,
            Err(why) => return skipped(why),
        };
        let child = player("voice_windows::lab::the_sound_the_sound_test_starts", &id, 2);
        let mut sound = Capture::open(Hears::Tree { roots: vec![child.id()], app_id: String::new() })
            .expect("the player's loopback opens");
        // The player plays its sound a second in, then a second of silence after it.
        let (waited, heard) = heard_from_first(&mut sound, Duration::ZERO, Duration::from_secs(3));
        drop(sound);
        let _ = losses(child);
        // In frames of the left channel: from the first loud sample to the last.
        let loud = |sample: &i16| sample.unsigned_abs() > 300;
        let left: Vec<i16> = heard.iter().step_by(2).copied().collect();
        let span = left.iter().position(loud).zip(left.iter().rposition(loud));
        let heard_for = span.map_or(0, |(first, last)| last + 1 - first);
        let whole = SOUND.len();
        println!("first heard after {waited:?}; heard for {heard_for} samples of {whole}");
        assert!(heard_for.abs_diff(whole) < whole / 50, "the sound came out whole: {heard_for} of {whole}");
    }

    /// The speaker's pace is the machine's: its sender is a thread of a process of its own, which a loaded machine
    /// runs late, and frames sent late run the speaker out.
    mod timing {
        use super::*;

        #[test]
        fn the_speaker_opened_by_its_endpoint_id_plays_a_senders_clock_heard_on_its_loopback() {
            let id = match default(|listing| listing.default_speaker) {
                Ok(id) => id,
                Err(why) => return skipped(why),
            };
            let child = player("voice_windows::lab::the_player_the_speaker_tests_start", &id, 4);
            let mut sound = Capture::open(Hears::Tree { roots: vec![child.id()], app_id: String::new() })
                .expect("the player's loopback opens");
            // The first half second is the player's speaker filling; the player plays four.
            let (waited, heard) =
                heard_from_first(&mut sound, Duration::from_millis(500), Duration::from_millis(3_750));
            drop(sound);
            let (dropped, underruns) = losses(child);
            // A quarter second at a time: the speaker follows the sender's clock, a fraction of a percent off the
            // device's, so the tone's phase drifts over seconds and a longer window would smear it.
            let windows: Vec<f64> = heard.chunks(RATE as usize / 2).map(strength).collect();
            let weakest = windows.iter().copied().fold(f64::INFINITY, f64::min);
            println!("first heard after {waited:?}; dropped {dropped}, ran out {underruns} times");
            println!("the tone at least {weakest:.0} of {LEVEL}");
            assert_eq!(dropped, 0, "frames sent at the speaker's pace never fill its ring");
            assert!(underruns <= 1, "it runs out at most once, before its margin has first filled: {underruns}");
            assert!(windows.len() >= 12, "three seconds were heard: {} quarter seconds", windows.len());
            assert!(
                weakest > LEVEL / 2.0,
                "the tone played on the speaker asked for, in every quarter second: {weakest:.0}"
            );
        }
    }

    /// The communications default's speaker heard for a minute, a second at a time, while its devices are changed by
    /// hand or by script around it: a second it falls silent in says when the speaker stopped.
    #[test]
    #[ignore = "a measurement, run on request"]
    fn the_default_speaker_is_heard_through_a_minute_of_device_changes() {
        if let Err(why) = default(|listing| listing.default_speaker) {
            return skipped(why);
        }
        let child = player("voice_windows::lab::the_player_the_speaker_tests_start", "", 60);
        let mut sound = Capture::open(Hears::Tree { roots: vec![child.id()], app_id: String::new() })
            .expect("the player's loopback opens");
        let (started, mut second, mut heard) = (Instant::now(), 0, Vec::new());
        while started.elapsed() < Duration::from_secs(62) {
            sound.keep(Instant::now());
            for chunk in sound.chunks().by_ref() {
                heard.extend_from_slice(&chunk);
            }
            if started.elapsed() >= Duration::from_secs(second + 1) {
                println!("second {second}: the tone at {:.0} of {LEVEL}", strength(&heard));
                heard.clear();
                second += 1;
            }
            thread::sleep(Duration::from_millis(5));
        }
        drop(sound);
        let (dropped, underruns) = losses(child);
        println!("dropped {dropped}, ran out {underruns} times");
    }

    #[test]
    #[ignore = "a measurement, run on request"]
    fn the_speaker_keeps_a_senders_clock_for_ten_minutes_without_running_out() {
        let id = match default(|listing| listing.default_speaker) {
            Ok(id) => id,
            Err(why) => return skipped(why),
        };
        let (mut speaker, mut voiced) = match Speaker::open(Some(&id), Instant::now()) {
            Ok(opened) => opened,
            Err(why) => return skipped(why.to_string()),
        };
        let mut first = None;
        for minute in 1..=10 {
            send(&mut speaker, &mut voiced, 60);
            let (dropped, underruns) = speaker.losses();
            println!("minute {minute}: dropped {dropped}, ran out {underruns} times, delay {:?}", voiced.delay());
            first.get_or_insert(voiced.delay());
        }
        assert_eq!(speaker.health(), Health::Running);
        let (dropped, underruns) = speaker.losses();
        let delay = voiced.delay();
        println!("ten minutes: dropped {dropped}, ran out {underruns} times, delay {delay:?}");
        assert_eq!(dropped, 0, "the rate follows the sender, so its ring never fills");
        assert!(underruns <= 1, "nor runs dry past its first fill: {underruns}");
        // The ring holds minutes of drift, so a rate left alone shows first as delay: 50 ms in ten minutes on Machine A.
        let drift = delay.abs_diff(first.unwrap_or_default());
        assert!(drift <= DRIFT, "nor drifts from the sender: {drift:?} from its first minute");
    }
}
