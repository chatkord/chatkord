//! A share's sound hears the chosen applications and nothing else, follows one that restarts, and leaves nothing
//! behind. The applications are `pw-cat` players linked to no device, so the machine stays silent.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use pw::{App, Hears, Mix};
use testserver::Undo;
use webrtc as _;

const LOUD: f64 = 8000.0;
/// The rate the tones are written and heard at.
const RATE: u32 = 48_000;
/// Half a second of both channels: as much as each look listens to.
const LOOK: usize = RATE as usize;

use tier::skipped;

/// Twenty seconds of a stereo tone at `hz`, as a WAV file.
fn tone(dir: &Path, hz: u32) -> PathBuf {
    let path = dir.join(format!("tone-{hz}.wav"));
    let frames = RATE * 20;
    let mut wav = Vec::new();
    let bytes = frames * 4;
    for part in [&b"RIFF"[..], &(36 + bytes).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes()] {
        wav.extend_from_slice(part);
    }
    for part in [&2u16.to_le_bytes()[..], &RATE.to_le_bytes(), &(RATE * 4).to_le_bytes(), &4u16.to_le_bytes()] {
        wav.extend_from_slice(part);
    }
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for i in 0..frames {
        let sample = (LOUD * (TAU * f64::from(hz) * f64::from(i) / f64::from(RATE)).sin()) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    // Tests running at once write the same file: each writes its own and renames it into place, so a player opening
    // it never reads one half written (pw-cat then quits at once and nothing is heard).
    static WRITES: AtomicUsize = AtomicUsize::new(0);
    let own = dir.join(format!("tone-{hz}.wav.{}.{}", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&own, wav).unwrap();
    std::fs::rename(&own, &path).unwrap();
    path
}

/// A player linked to no device, stopped when dropped or when the test crashes: a child of this test, or apart from
/// it under the user's service manager, so it is no process of ours.
enum Player {
    Child(Child, Option<Undo>),
    Apart { unit: String, _stop: Undo },
}

fn pw_cat_args(name: &str, file: &Path) -> Vec<String> {
    let props = format!("{{ application.name = {name} node.name = {name} }}");
    ["pw-cat", "--playback", "--target", "0", "-P", &props]
        .map(str::to_owned)
        .into_iter()
        .chain([file.display().to_string()])
        .collect()
}

impl Player {
    fn start(name: &str, file: &Path) -> Option<Self> {
        let args = pw_cat_args(name, file);
        let mut child =
            Command::new(&args[0]).args(&args[1..]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
        match Undo::new("kill", &[&child.id().to_string()]) {
            Some(undo) => Some(Self::Child(child, Some(undo))),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }

    /// A player the user's service manager starts, as any application on the desktop is.
    fn apart(name: &str, file: &Path) -> Option<Self> {
        let unit = format!("{name}.service");
        let undo = Undo::new("systemctl", &["--user", "stop", &unit])?;
        let started = Command::new("systemd-run")
            .args(["--user", "--quiet", "--collect", "--unit", &unit])
            .args(pw_cat_args(name, file))
            .status()
            .ok()?;
        started.success().then_some(Self::Apart { unit, _stop: undo })
    }

    fn pid(&self) -> u32 {
        match self {
            Self::Child(child, _) => child.id(),
            Self::Apart { unit, .. } => {
                let shown = Command::new("systemctl").args(["--user", "show", "-P", "MainPID", unit]).output().unwrap();
                String::from_utf8_lossy(&shown.stdout).trim().parse().unwrap()
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // A unit is stopped by its `Undo`; a child is ended here, so its id is not killed again once it is reused.
        if let Self::Child(child, undo) = self {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(undo) = undo.take() {
                undo.forget();
            }
        }
    }
}

fn app(name: &str) -> App {
    App { name: name.to_owned(), binary: Some("pw-cat".to_owned()) }
}

fn chosen(name: &str) -> Hears {
    Hears::Apps([app(name)].into())
}

/// Waits until neither `hz` nor `other` is heard: an unlinked stream delivers no chunk at all.
fn silent(chunks: &Receiver<Vec<u8>>, hz: f64, other: f64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let heard = listen(chunks);
        if strength(&heard, hz) < LOUD / 20.0 && strength(&heard, other) < LOUD / 20.0 {
            return true;
        }
    }
    false
}

/// How strong `hz` is in the left channel of interleaved `samples`, in sample units.
fn strength(samples: &[i16], hz: f64) -> f64 {
    let left: Vec<f64> = samples.iter().step_by(2).map(|&s| f64::from(s)).collect();
    let (mut re, mut im) = (0.0, 0.0);
    for (i, sample) in left.iter().enumerate() {
        let phase = TAU * hz * i as f64 / f64::from(RATE);
        re += sample * phase.cos();
        im += sample * phase.sin();
    }
    2.0 * re.hypot(im) / left.len().max(1) as f64
}

/// Drops what arrived so far, then keeps the next half second, as samples.
fn listen(chunks: &Receiver<Vec<u8>>) -> Vec<i16> {
    while chunks.try_recv().is_ok() {}
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut heard = Vec::new();
    while heard.len() < LOOK && Instant::now() < deadline {
        if let Ok(bytes) = chunks.recv_timeout(Duration::from_millis(100)) {
            heard.extend(bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)));
        }
    }
    heard
}

/// Waits until what is heard holds `hz` and not `other`, as the links settle; what the last look heard, if never.
fn hears(chunks: &Receiver<Vec<u8>>, hz: f64, other: f64) -> Result<(), String> {
    // A player started under the whole gate's load has been seen to need more than 5 s to be heard.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = String::from("nothing");
    while Instant::now() < deadline {
        let heard = listen(chunks);
        let (wanted, unwanted) = (strength(&heard, hz), strength(&heard, other));
        if wanted > LOUD / 2.0 && unwanted < LOUD / 20.0 {
            return Ok(());
        }
        last = format!("{} samples, {hz} Hz at {wanted:.0}, {other} Hz at {unwanted:.0}", heard.len());
    }
    Err(last)
}

fn ours_left_in_the_graph() -> usize {
    let listed = Command::new("pw-link").arg("--links").output().unwrap();
    let pid = std::process::id().to_string();
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| line.contains("chatkord-share-") && line.contains(&pid))
        .count()
}

/// The id of this test's share capture node in the graph, from `pw-cli`.
fn our_capture_node() -> Option<String> {
    let listed = Command::new("pw-cli").args(["list-objects", "Node"]).output().ok()?;
    let pid = std::process::id().to_string();
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    let mut id = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("id ") {
            id = rest.split(',').next().map(str::to_owned);
        }
        if line.contains("node.name") && line.contains(&format!("chatkord-share-{pid}-")) {
            return id;
        }
    }
    None
}

mod lab {
    use super::*;

    /// How late a share's sound is when it is pushed to the engine, which stamps it then: the graph's delay to our
    /// stream, read from PipeWire's own clock, and the chunk's wait for its 10 ms, at most: the feed is woken as each
    /// chunk is made.
    /// Stamped late, the sound plays that much after its picture, which people notice past about 125 ms (ITU-R
    /// BT.1359; sound early, past 45 ms).
    #[test]
    fn a_shares_sound_is_pushed_within_the_lip_sync_bound_of_its_capture() {
        const CHUNKING: Duration = Duration::from_millis(10);
        const SOUND_LATE: Duration = Duration::from_millis(125);
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let file = tone(&dir, 440);
        let name = format!("chatkord-mix-late-{}", std::process::id());
        let Some(_player) = Player::apart(&name, &file) else {
            return skipped("no user service manager to start a player apart from this test");
        };
        let (tell, chunks) = mpsc::channel();
        let mix = match Mix::open(chosen(&name), RATE, move |bytes| {
            let _ = tell.send(bytes.to_vec());
        }) {
            Ok(mix) => mix,
            Err(why) => return skipped(why.to_string()),
        };
        hears(&chunks, 440.0, 660.0).expect("heard");
        let mut graph = Duration::ZERO;
        for _ in 0..20 {
            graph = graph.max(mix.timing(RATE).expect("the stream says its time").delay);
            std::thread::sleep(Duration::from_millis(100));
        }
        let quantum = chunks.try_iter().map(|bytes| bytes.len() / 4).max().unwrap_or(0);
        let late = graph + CHUNKING;
        eprintln!("graph delay at most {graph:?}, quantum {quantum} frames: pushed at most {late:?} after capture");
        assert!(late < SOUND_LATE, "a picture stamped at capture leads its sound by less than people notice");
    }

    #[test]
    fn a_mix_whose_node_the_sound_server_takes_down_says_it_ended() {
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let file = tone(&dir, 440);
        let name = format!("chatkord-mix-ended-{}", std::process::id());
        let Some(_player) = Player::apart(&name, &file) else {
            return skipped("no user service manager to start a player apart from this test");
        };
        let (tell, chunks) = mpsc::channel();
        let mix = match Mix::open(chosen(&name), RATE, move |bytes| {
            let _ = tell.send(bytes.to_vec());
        }) {
            Ok(mix) => mix,
            Err(why) => return skipped(why.to_string()),
        };
        hears(&chunks, 440.0, 660.0).expect("heard before");
        assert_eq!(mix.state(), pw::State::Streaming);
        let node = our_capture_node().expect("our capture node is listed");
        assert!(Command::new("pw-cli").args(["destroy", &node]).status().unwrap().success(), "taken down");
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(mix.state(), pw::State::Streaming | pw::State::Paused) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            matches!(mix.state(), pw::State::Error | pw::State::Unconnected),
            "ended, so its owner opens it again: {:?}",
            mix.state()
        );
    }

    #[test]
    fn the_chosen_applications_are_heard_followed_and_let_go() {
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let (a, b) = (tone(&dir, 440), tone(&dir, 660));
        let (name_a, name_b) =
            (format!("chatkord-mix-a-{}", std::process::id()), format!("chatkord-mix-b-{}", std::process::id()));
        let (Some(player_a), Some(_player_b)) = (Player::apart(&name_a, &a), Player::apart(&name_b, &b)) else {
            return skipped("no user service manager to start players apart from this test");
        };

        let (tell, chunks) = mpsc::channel();
        let mix = match Mix::open(chosen(&name_a), RATE, move |bytes| {
            let _ = tell.send(bytes.to_vec());
        }) {
            Ok(mix) => mix,
            Err(why) => return skipped(why.to_string()),
        };
        hears(&chunks, 440.0, 660.0).expect("the chosen application is heard, and the other is not");

        mix.choose(chosen(&name_b));
        hears(&chunks, 660.0, 440.0).expect("a new choice is heard instead");

        mix.choose(chosen(&name_a));
        drop(player_a);
        let _player_a = Player::apart(&name_a, &a).unwrap();
        hears(&chunks, 440.0, 660.0).expect("an application that starts again is followed");

        assert!(ours_left_in_the_graph() > 0, "the share's links are in the graph while it runs");
        drop(mix);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(ours_left_in_the_graph(), 0, "nothing of the share is left once it is dropped");
    }

    #[test]
    fn a_window_is_heard_through_its_owner_and_a_screen_hears_all_but_this_process() {
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let id = std::process::id();
        let (ours, theirs) = (format!("chatkord-ours-{id}"), format!("chatkord-theirs-{id}"));
        // Tones of its own: other tests' players run beside it, and a whole screen hears them too.
        let Some(child) = Player::start(&ours, &tone(&dir, 330)) else {
            return skipped("pw-cat would not start");
        };
        let Some(apart) = Player::apart(&theirs, &tone(&dir, 770)) else {
            return skipped("no user service manager to start a player apart from this test");
        };

        let (tell, chunks) = mpsc::channel();
        let window = Hears::Tree { roots: vec![apart.pid()], app_id: "org.example.none".to_owned() };
        let mix = match Mix::open(window, RATE, move |bytes| {
            let _ = tell.send(bytes.to_vec());
        }) {
            Ok(mix) => mix,
            Err(why) => return skipped(why.to_string()),
        };
        hears(&chunks, 770.0, 330.0).expect("the window's owner is heard, and nothing else");

        mix.choose(Hears::AllButOurs);
        hears(&chunks, 770.0, 330.0).expect("a screen hears every application but this process's own children");

        mix.choose(Hears::Nothing);
        assert!(silent(&chunks, 770.0, 330.0), "turned off, nothing is heard");
        mix.choose(Hears::AllButOurs);
        hears(&chunks, 770.0, 330.0).expect("and turned on again, it is heard again");
        drop((mix, child));
    }

    /// Links asked for and taken down before the daemon answered for them: what the daemon then says of them reaches
    /// nothing of ours, which is gone.
    #[test]
    fn links_taken_down_before_the_daemon_answers_leave_nothing_to_tell() {
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let name = format!("chatkord-flip-{}", std::process::id());
        let Some(_player) = Player::apart(&name, &tone(&dir, 880)) else {
            return skipped("no user service manager to start a player apart from this test");
        };
        let (tell, chunks) = mpsc::channel();
        let mix = match Mix::open(chosen(&name), RATE, move |bytes| {
            let _ = tell.send(bytes.to_vec());
        }) {
            Ok(mix) => mix,
            Err(why) => return skipped(why.to_string()),
        };
        hears(&chunks, 880.0, 0.0).expect("the player is heard, so each choice below asks for its links");
        for _ in 0..100 {
            mix.choose(Hears::Nothing);
            mix.choose(chosen(&name));
        }
        hears(&chunks, 880.0, 0.0).expect("the last choice is heard");
        drop(mix);
    }

    #[test]
    fn the_applications_playing_are_listed() {
        if Command::new("pw-cat").arg("--version").output().is_err() {
            return skipped("no pw-cat to play a tone");
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let name = format!("chatkord-apps-{}", std::process::id());
        let (tell, lists) = mpsc::channel();
        let apps = match pw::Apps::watch(move |apps| {
            let _ = tell.send(apps);
        }) {
            Ok(apps) => apps,
            Err(why) => return skipped(why.to_string()),
        };
        // Apart from this test: a child of this process is never offered.
        let Some(player) = Player::apart(&name, &tone(&dir, 550)) else {
            return skipped("no user service manager to start a player apart from this test");
        };
        let listed = |want: bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while let Ok(apps) = lists.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                if apps.contains(&app(&name)) == want {
                    return true;
                }
            }
            false
        };
        assert!(listed(true), "an application that plays is listed");
        drop(player);
        assert!(listed(false), "and is gone from the list once it stops");
        drop(apps);
    }
}
