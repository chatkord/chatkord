//! A microphone and a speaker that exist only for a test, made through the sound server and unloaded after it: a
//! device with nothing real behind it.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// The rate the virtual microphone is fed at, and the length of each frame written into it.
const RATE_HZ: u32 = 48_000;
const FRAME_MS: u32 = 10;

/// The 10 ms frame `index` of a sine at `hz` whose loudest sample is `peak`.
fn sine(hz: f64, peak: f64, index: u32) -> Vec<i16> {
    let samples = RATE_HZ / 1000 * FRAME_MS;
    let first = u64::from(index) * u64::from(samples);
    #[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss, reason = "a sine within i16, by sample")]
    (0..u64::from(samples))
        .map(|n| (peak * (std::f64::consts::TAU * hz * (first + n) as f64 / f64::from(RATE_HZ)).sin()) as i16)
        .collect()
}

/// The microphone's pipe, opened to write without waiting: the sound server stops reading a source nothing records
/// from, and a writer that waited then would never see the test end.
fn writer(pipe: &std::path::Path) -> Option<std::fs::File> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Linux's `O_NONBLOCK`.
        open.custom_flags(0o4000);
    }
    open.open(pipe).ok()
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The sound server's device list, held by one test at a time on the machine: a device chosen by its place in the
/// module's list is another device once one before it goes, so the tests that make and unload devices take turns.
pub struct Lab {
    _turn: tier::Turn,
}

impl Lab {
    /// Waits for the machine's turn; none when the lock file cannot be opened.
    pub fn take() -> Option<Self> {
        Some(Self { _turn: tier::turn("virtual_sound")? })
    }

    /// A microphone and a speaker for the test named `test`, while this turn lasts.
    pub fn make(&self, test: &str) -> Option<Virtual<'_>> {
        Virtual::make(self, test)
    }

    /// The desktop's notification sounds muted while this lasts, as KDE's Do Not Disturb mutes them while an app is
    /// full screen, and put back as they were after; none when the session manager's settings cannot be changed.
    pub fn mute_notifications(&self) -> Option<NotificationsMuted<'_>> {
        let before = route_setting(NOTIFICATIONS)?;
        route_settings(&["0", NOTIFICATIONS, MUTED, JSON])?;
        Some(NotificationsMuted { _lab: self, before })
    }
}

/// WirePlumber's remembered settings for every stream of the notification role, the key the desktop's mute writes.
const NOTIFICATIONS: &str = "restore.stream.Output/Audio.media.role:Notification";
const JSON: &str = "Spa:String:JSON";
const MUTED: &str = r#"{ "mute": true }"#;

fn route_settings(args: &[&str]) -> Option<String> {
    let out = Command::new("pw-metadata").args(["-n", "route-settings"]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The value the session manager keeps under `key`, `Some(None)` when it keeps none.
fn route_setting(key: &str) -> Option<Option<String>> {
    let listed = route_settings(&["0", key])?;
    Some(listed.lines().find_map(|line| {
        let value = line.split_once(" value:'")?.1;
        Some(value.rsplit_once("' type:")?.0.to_owned())
    }))
}

/// The desktop's notification sounds muted, until dropped: see [`Lab::mute_notifications`].
pub struct NotificationsMuted<'lab> {
    _lab: &'lab Lab,
    before: Option<String>,
}

impl Drop for NotificationsMuted<'_> {
    fn drop(&mut self) {
        // The desktop may have set its own since (a full screen left): that one stands.
        if route_setting(NOTIFICATIONS).flatten().is_none_or(|now| now != MUTED) {
            return;
        }
        let _ = match &self.before {
            Some(value) => route_settings(&["0", NOTIFICATIONS, value, JSON]),
            None => route_settings(&["-d", "0", NOTIFICATIONS]),
        };
    }
}

/// A microphone fed from a pipe and a speaker that can be recorded, made through the sound server for one test
/// and unloaded after it. Names carry the process id so tests that run together do not share one.
pub struct Virtual<'lab> {
    _lab: &'lab Lab,
    /// The microphone's description, which a device list finds it by.
    pub microphone: String,
    /// The speaker's description, which a device list finds it by.
    pub speaker: String,
    /// The speaker's name at the sound server (`node.name` on PipeWire), which a stream asks for it by.
    pub sink: String,
    /// The microphone's name at the sound server, the same way.
    pub source: String,
    modules: Vec<String>,
    pipe: PathBuf,
    speaking: Arc<AtomicBool>,
    writer: Option<thread::JoinHandle<()>>,
}

impl<'lab> Virtual<'lab> {
    fn make(lab: &'lab Lab, test: &str) -> Option<Self> {
        let id = format!("{}_{test}", std::process::id());
        let pipe = std::env::temp_dir().join(format!("chatkord_probe_{id}.pipe"));
        let _ = std::fs::remove_file(&pipe);
        let (microphone, speaker) = (format!("ChatkordProbeMic{id}"), format!("ChatkordProbeSpeaker{id}"));
        let (sink, source) = (format!("chatkord_probe_speaker_{id}"), format!("chatkord_probe_mic_{id}"));
        let mut modules = Vec::new();
        for args in [
            vec![
                "load-module".to_string(),
                "module-pipe-source".to_string(),
                format!("source_name={source}"),
                format!("file={}", pipe.display()),
                "format=s16le".to_string(),
                "rate=48000".to_string(),
                "channels=1".to_string(),
                format!("source_properties=device.description={microphone}"),
            ],
            vec![
                "load-module".to_string(),
                "module-null-sink".to_string(),
                format!("sink_name={sink}"),
                "channels=1".to_string(),
                format!("sink_properties=device.description={speaker}"),
            ],
        ] {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            match pactl(&args) {
                Some(module) => modules.push(module),
                None => {
                    let made = Self {
                        _lab: lab,
                        microphone,
                        speaker,
                        sink,
                        source,
                        modules,
                        pipe,
                        speaking: Arc::default(),
                        writer: None,
                    };
                    drop(made);
                    return None;
                }
            }
        }
        Some(Self {
            _lab: lab,
            microphone,
            speaker,
            sink,
            source,
            modules,
            pipe,
            speaking: Arc::default(),
            writer: None,
        })
    }

    /// Sends a tone into the microphone until the test ends.
    pub fn speak(&mut self) {
        self.feed(|index| sine(440.0, 8_000.0, index));
    }

    /// Sends a burst of tone `length` long every `every` into the microphone, silence between, until the test ends:
    /// a sound whose start a recording can find.
    pub fn pulse(&mut self, every: Duration, length: Duration) {
        let frame = u128::from(FRAME_MS);
        let (every, length) = ((every.as_millis() / frame).max(1), length.as_millis() / frame);
        self.feed(move |index| {
            let peak = if u128::from(index) % every < length { 12_000.0 } else { 0.0 };
            sine(1_000.0, peak, index)
        });
    }

    /// Writes the 10 ms frame `index` that `frame` makes into the microphone, one each 10 ms, until the test ends.
    fn feed(&mut self, frame: impl Fn(u32) -> Vec<i16> + Send + 'static) {
        let (pipe, speaking) = (self.pipe.clone(), Arc::clone(&self.speaking));
        speaking.store(true, Ordering::SeqCst);
        self.writer = Some(thread::spawn(move || {
            let mut file = writer(&pipe);
            let mut index = 0;
            while speaking.load(Ordering::SeqCst) {
                let bytes: Vec<u8> = frame(index).iter().flat_map(|s| s.to_le_bytes()).collect();
                // A frame is less than a pipe's atomic write, so it goes whole or, the pipe full, not at all.
                match file.as_mut().map(|open| open.write(&bytes)) {
                    Some(Ok(_)) => {}
                    Some(Err(why)) if why.kind() == std::io::ErrorKind::WouldBlock => {}
                    Some(Err(_)) | None => break,
                }
                index = index.wrapping_add(1);
                thread::sleep(Duration::from_millis(u64::from(FRAME_MS)));
            }
        }));
    }

    /// Records what the speaker is sent, once the recorder is taking samples (within `within`), so a sound asked
    /// for after this is not missed while the recorder connects.
    pub async fn record(&self, within: Duration) -> Option<Recording> {
        let file = std::env::temp_dir().join(format!("{}.wav", self.sink));
        let _ = std::fs::remove_file(&file);
        let child = Command::new("pw-record")
            // Without the property the recorder takes the default microphone, not what the speaker is sent.
            .args(["-P", "{ stream.capture.sink=true }"])
            .args(["--target", &self.sink, "--rate", "48000", "--channels", "1"])
            .args(["--format", "s16"])
            .arg(&file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let recording = Recording { child: Some(child), file };
        let deadline = tokio::time::Instant::now() + within;
        // pw-record grows its file while it runs, and once connected writes the speaker's silence within ~50 ms.
        while samples(&std::fs::read(&recording.file).unwrap_or_default()).is_empty() {
            if tokio::time::Instant::now() >= deadline {
                recording.stop();
                return None;
            }
            tokio::time::sleep(POLL).await;
        }
        Some(recording)
    }
}

impl Drop for Virtual<'_> {
    fn drop(&mut self) {
        self.speaking.store(false, Ordering::SeqCst);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        for module in self.modules.iter().rev() {
            let _ = pactl(&["unload-module", module]);
        }
        let _ = std::fs::remove_file(&self.pipe);
    }
}

/// What a virtual speaker is sent, recorded until [`Recording::stop`] or until it is let go, which ends the recorder
/// either way.
pub struct Recording {
    child: Option<Child>,
    file: PathBuf,
}

impl Recording {
    /// The loudest sample recorded so far; the recorder keeps running.
    pub fn peak(&self) -> i16 {
        loudest(&std::fs::read(&self.file).unwrap_or_default())
    }

    /// Waits for a sample louder than `over`, for at most `within`: the loudest sample, `Err` when none was.
    pub async fn heard(&self, over: i16, within: Duration) -> Result<i16, i16> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let peak = self.peak();
            if peak > over {
                return Ok(peak);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(peak);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// How many samples are recorded so far, the place in [`Recording::finish`]'s samples a sound asked for now
    /// starts after.
    pub fn taken(&self) -> usize {
        samples(&std::fs::read(&self.file).unwrap_or_default()).len()
    }

    /// The loudest sample recorded; see [`Recording::finish`].
    pub fn stop(self) -> i16 {
        self.finish().iter().map(|sample| sample.saturating_abs()).max().unwrap_or(0)
    }

    /// Every sample recorded.
    pub fn finish(mut self) -> Vec<i16> {
        self.end();
        let bytes = std::fs::read(&self.file).unwrap_or_default();
        samples(&bytes).iter().map(|pair| i16::from_le_bytes(*pair)).collect()
    }

    /// The recorder is interrupted, not killed, so that it writes out what it holds, and waited for, once.
    fn end(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("kill").args(["-INT", &child.id().to_string()]).status();
            let _ = child.wait();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.end();
        let _ = std::fs::remove_file(&self.file);
    }
}

/// How often a recording is read while it is waited on.
const POLL: Duration = Duration::from_millis(50);

/// The samples of a 16-bit WAV, read to its end: a recording still running has not yet written its length.
fn samples(wav: &[u8]) -> &[[u8; 2]] {
    let Some(start) = wav.windows(4).position(|window| window == b"data") else { return &[] };
    wav[(start + 8).min(wav.len())..].as_chunks::<2>().0
}

fn loudest(wav: &[u8]) -> i16 {
    samples(wav).iter().map(|pair| i16::from_le_bytes(*pair).saturating_abs()).max().unwrap_or(0)
}
