//! A share's sound on Windows: WASAPI's process loopback, of one process tree or of every process but ours, never a
//! device's whole output, so the call is never heard in it; where the loopback is missing there is no sound.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient,
    IAudioClient, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVE_FORMAT_PCM, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{
    BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl,
};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, Ref, implement};

use super::{CHANNELS, CHUNK, RATE, SoundError};
use crate::voice::Health;

mod apps;
pub use apps::App;
pub(crate) use apps::Listen;
pub use apps::holds_us;

/// Which processes a share hears. This process and its children are never heard, whatever is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hears {
    /// No sound, which [`super::Capture::open`] refuses.
    Nothing,
    /// A window's program: the sound of the first of `roots` and its children (Windows hears one tree).
    Tree {
        /// Process ids, this one's skipped.
        roots: Vec<u32>,
        /// The program's app id, which Windows' loopback does not take: it hears by process alone.
        app_id: String,
    },
    /// Every process's but this one's tree.
    AllButOurs,
}

/// How long the capture's thread waits for sound before it looks whether it should stop.
const WAIT: Duration = Duration::from_millis(100);
/// The buffer Windows keeps for the capture, in its 100 ns units.
#[expect(clippy::cast_possible_truncation, reason = "200 ms is 2 000 000 of them")]
const BUFFER: i64 = (Duration::from_millis(200).as_nanos() / 100) as i64;
/// Zeros for a packet Windows marks silent, handed over a chunk at a time.
const QUIET: [u8; CHUNK * size_of::<i16>()] = [0; CHUNK * size_of::<i16>()];

/// The loopback being heard. Dropping it stops the capture and waits for its thread.
pub(super) struct Stream {
    stop: Arc<AtomicBool>,
    /// Set when the capture's thread ended by itself: the endpoint went (`AUDCLNT_E_DEVICE_INVALIDATED`) or a read
    /// failed.
    ended: Arc<AtomicBool>,
    /// How long after its capture the last packet was handed over, in microseconds.
    late: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Stream {
    /// Hears what `hears` names, refusing [`Hears::Nothing`], each packet handed to `hear` on the capture's thread.
    /// Blocks until the loopback has started or refused.
    pub(super) fn open(hears: &Hears, hear: impl FnMut(&[u8]) + Send + 'static) -> Result<Self, SoundError> {
        // SAFETY: reads this process's own id.
        let ours = unsafe { GetCurrentProcessId() };
        let (target, mode) = match hears {
            Hears::Nothing => return Err(SoundError::Stream),
            Hears::Tree { roots, .. } => {
                let root = apps::root_not_holding_us(roots).ok_or(SoundError::Stream)?;
                (root, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
            }
            Hears::AllButOurs => (ours, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE),
        };
        Self::start(AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: target, ProcessLoopbackMode: mode }, hear)
    }

    /// Starts the loopback `params` names on a thread of its own.
    fn start(
        params: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
        hear: impl FnMut(&[u8]) + Send + 'static,
    ) -> Result<Self, SoundError> {
        let stop = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let late = Arc::new(AtomicU64::new(0));
        let (started, start) = mpsc::sync_channel(1);
        let (stopping, ending, lateness) = (stop.clone(), ended.clone(), late.clone());
        let thread = std::thread::Builder::new()
            .name("chatkord-share-sound".into())
            .spawn(move || {
                run(params, hear, &stopping, &lateness, &started);
                ending.store(true, Ordering::Release);
            })
            .map_err(|_| SoundError::Stream)?;
        match start.recv() {
            Ok(Ok(())) => Ok(Self { stop, ended, late, thread: Some(thread) }),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => Err(SoundError::Stream),
        }
    }

    /// Running until its thread ends by itself.
    pub(super) fn health(&self) -> Health {
        if self.ended.load(Ordering::Acquire) { Health::Stopped } else { Health::Running }
    }

    /// How long after its capture the last packet was handed over: Windows stamps each on the performance counter.
    pub(super) fn delay(&self) -> Duration {
        Duration::from_micros(self.late.load(Ordering::Relaxed))
    }
}

/// The capture's thread: COM, the loopback opened and said, then every packet chunked until told to stop.
fn run(
    params: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    mut hear: impl FnMut(&[u8]),
    stop: &AtomicBool,
    late: &AtomicU64,
    started: &mpsc::SyncSender<Result<(), SoundError>>,
) {
    // SAFETY: this thread's own COM apartment, left below on every path.
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        let _ = started.send(Err(SoundError::Stream));
        return;
    }
    match Loopback::open(params) {
        Ok(loopback) => {
            let _ = started.send(Ok(()));
            // This thread carries a call's sound into the ring: ahead of ordinary work, as the voice's threads are.
            let _ahead = crate::voice::realtime();
            loopback.hear(&mut hear, stop, late);
        }
        Err(why) => {
            let _ = started.send(Err(why));
        }
    }
    // SAFETY: paired with the initialisation above; the loopback's interfaces were released when it dropped.
    unsafe { CoUninitialize() };
}

/// A started loopback capture and the event Windows signals when sound waits.
struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    ready: HANDLE,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        // SAFETY: the client was started by `open` and is ours alone.
        let _ = unsafe { self.client.Stop() };
        // SAFETY: the event was made by `open` and is ours alone.
        let _ = unsafe { CloseHandle(self.ready) };
    }
}

impl Loopback {
    /// Activates the loopback for `params` and starts it in our format. A Windows without process loopback refuses
    /// the activation, which is said as this system having none.
    fn open(mut params: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS) -> Result<Self, SoundError> {
        let client = activate(&mut params).ok_or(SoundError::Unavailable)?;
        #[expect(clippy::cast_possible_truncation, reason = "two channels, and the PCM tag is 1")]
        let (channels, tag) = (CHANNELS as u16, WAVE_FORMAT_PCM as u16);
        let block = channels * 2;
        let format = WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: channels,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let flags =
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;
        // SAFETY: the format lives across the call, which copies it.
        unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER, 0, &raw const format, None) }
            .map_err(|_| SoundError::Stream)?;
        // SAFETY: an unnamed event, closed by `Drop` or below.
        let ready = unsafe { CreateEventW(None, false, false, None) }.map_err(|_| SoundError::Stream)?;
        // SAFETY: the event is made before it is handed over.
        let opened = unsafe { client.SetEventHandle(ready) }
            .and_then(|()| {
                // SAFETY: an initialised client.
                unsafe { client.GetService::<IAudioCaptureClient>() }
            })
            .and_then(|capture| {
                // SAFETY: an initialised client with its event set.
                unsafe { client.Start() }.map(|()| capture)
            });
        match opened {
            Ok(capture) => Ok(Self { client, capture, ready }),
            Err(_) => {
                // SAFETY: the event made above, handed to nothing that outlives this.
                let _ = unsafe { CloseHandle(ready) };
                Err(SoundError::Stream)
            }
        }
    }

    /// Hands every packet's bytes to `hear`, a silent one as zeros, until `stop` is set or the capture fails; says in
    /// `late` how long after its capture each was handed over.
    fn hear(&self, hear: &mut impl FnMut(&[u8]), stop: &AtomicBool, late: &AtomicU64) {
        let block = CHANNELS * 2;
        #[expect(clippy::cast_possible_truncation, reason = "100 ms")]
        let wait = WAIT.as_millis() as u32;
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: the event made by `open`, alive while `self` is.
            if unsafe { WaitForSingleObject(self.ready, wait) } != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                let (mut data, mut frames, mut flags, mut captured) = (std::ptr::null_mut(), 0u32, 0u32, 0u64);
                // SAFETY: the capture client of a started stream; a packet taken is released below before the next.
                let taken = unsafe {
                    self.capture.GetBuffer(
                        &raw mut data,
                        &raw mut frames,
                        &raw mut flags,
                        None,
                        Some(&raw mut captured),
                    )
                };
                if taken.is_err() {
                    return;
                }
                if frames == 0 {
                    break;
                }
                let length = frames as usize * block;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    for piece in (0..length).step_by(QUIET.len()) {
                        hear(&QUIET[..QUIET.len().min(length - piece)]);
                    }
                } else {
                    // SAFETY: Windows lent `frames` whole frames of our format at `data` until the release below.
                    hear(unsafe { std::slice::from_raw_parts(data, length) });
                }
                if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0
                    && let Some(at) = i64::try_from(captured).ok().and_then(clock::instant_of)
                {
                    late.store(u64::try_from(at.elapsed().as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
                }
                // SAFETY: releases exactly the packet taken above.
                if unsafe { self.capture.ReleaseBuffer(frames) }.is_err() {
                    return;
                }
            }
        }
    }
}

/// The audio client of the process loopback `params` names, once Windows has activated it; none where it would not.
fn activate(params: &mut AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS) -> Option<IAudioClient> {
    let mut activation = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 { ProcessLoopbackParams: *params },
    };
    #[expect(clippy::cast_possible_truncation, reason = "a Win32 struct's size, far under u32::MAX")]
    let size = size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
    let blob = BLOB { cbSize: size, pBlobData: (&raw mut activation).cast() };
    // Never dropped: clearing it would free the blob, which is ours, on the stack.
    let variant = std::mem::ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                Anonymous: PROPVARIANT_0_0_0 { blob },
                ..Default::default()
            }),
        },
    });
    let (done, finished) = mpsc::sync_channel(1);
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(done).into();
    // SAFETY: the parameters and the variant that points at them outlive the activation, which is waited for below.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&raw const *variant),
            &handler,
        )
    }
    .ok()?;
    finished.recv().ok()?;
    let (mut result, mut activated) = (windows::core::HRESULT(0), None);
    // SAFETY: the operation has completed, as its handler said.
    unsafe { operation.GetActivateResult(&raw mut result, &raw mut activated) }.ok()?;
    result.ok().ok()?;
    activated?.cast().ok()
}

/// Says that an activation completed. Agile, as Windows calls it from a thread of its own.
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(mpsc::SyncSender<()>);

impl IAgileObject_Impl for Activated_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _: Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.try_send(());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::CHUNK;
    use std::f64::consts::TAU;
    use std::time::Instant;
    use windows::Win32::Media::Audio::{
        IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

    /// The tone's pitch, apart from anything else a machine plays, and its level: quiet, about -50 dBFS.
    const HZ: f64 = 1234.0;
    const LEVEL: f64 = 100.0;
    /// The pitch of a player started apart, not this process's, so a test running beside another tells whose it hears.
    const APART_HZ: f64 = 987.0;

    use tier::skipped;

    /// Plays the tone from this process on the default output until `stop` is set.
    fn play(stop: &AtomicBool, playing: &mpsc::SyncSender<bool>) {
        // SAFETY: this thread's own apartment; every interface is released before it is left.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let Ok((client, writer, size)) = render() else {
            let _ = playing.send(false);
            // SAFETY: paired with the initialisation above; `render` released what it made.
            unsafe { CoUninitialize() };
            return;
        };
        let _ = playing.send(true);
        let mut at = 0u64;
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: a started client of this apartment.
            let room = size - unsafe { client.GetCurrentPadding() }.unwrap_or(size);
            let lent = (room > 0).then(|| {
                // SAFETY: a started client's writer, asked for no more than the room it has.
                unsafe { writer.GetBuffer(room) }
            });
            if let Some(Ok(buffer)) = lent {
                // SAFETY: Windows lent `room` frames of two 16-bit samples at `buffer` until the release below.
                let samples = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<i16>(), room as usize * 2) };
                for frame in samples.as_chunks_mut::<2>().0 {
                    #[expect(clippy::cast_possible_truncation, reason = "an amplitude of 100")]
                    let sample = (LEVEL * (TAU * HZ * at as f64 / f64::from(RATE)).sin()) as i16;
                    frame.fill(sample);
                    at += 1;
                }
                // SAFETY: releases exactly the room taken above.
                let _ = unsafe { writer.ReleaseBuffer(room, 0) };
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // SAFETY: the client started by `render`.
        let _ = unsafe { client.Stop() };
        drop((writer, client));
        // SAFETY: paired with the initialisation above, after every interface is released.
        unsafe { CoUninitialize() };
    }

    /// The default output, started in two-channel 16-bit PCM, its writer and its buffer's size in frames.
    fn render() -> windows::core::Result<(IAudioClient, IAudioRenderClient, u32)> {
        // SAFETY: the caller's thread is in an apartment.
        let devices: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
        // SAFETY: a live enumerator.
        let device = unsafe { devices.GetDefaultAudioEndpoint(eRender, eConsole) }?;
        // SAFETY: a live device.
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }?;
        #[expect(clippy::cast_possible_truncation, reason = "the PCM tag is 1")]
        let tag = WAVE_FORMAT_PCM as u16;
        let format = WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // SAFETY: the format lives across the call, which copies it.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                BUFFER,
                0,
                &raw const format,
                None,
            )
        }?;
        // SAFETY: an initialised client.
        let size = unsafe { client.GetBufferSize() }?;
        // SAFETY: an initialised client.
        let writer = unsafe { client.GetService::<IAudioRenderClient>() }?;
        // SAFETY: an initialised client.
        unsafe { client.Start() }?;
        Ok((client, writer, size))
    }

    /// How loud `chunks` are, in sample units.
    fn loudness(chunks: &[[i16; CHUNK]]) -> f64 {
        let squares: f64 = chunks.iter().flatten().map(|&sample| f64::from(sample).powi(2)).sum();
        (squares / (chunks.len().max(1) * CHUNK) as f64).sqrt()
    }

    /// How strong the tone is in the left channel of `chunks`, in sample units.
    fn strength(chunks: &[[i16; CHUNK]], hz: f64) -> f64 {
        let left: Vec<f64> = chunks.iter().flat_map(|chunk| chunk.iter().step_by(2)).map(|&s| f64::from(s)).collect();
        let (mut re, mut im) = (0.0, 0.0);
        for (i, sample) in left.iter().enumerate() {
            let phase = TAU * hz * i as f64 / f64::from(RATE);
            re += sample * phase.cos();
            im += sample * phase.sin();
        }
        2.0 * re.hypot(im) / left.len().max(1) as f64
    }

    /// A second of what `params` hears, after the first half second has settled.
    fn heard(params: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS) -> Vec<[i16; CHUNK]> {
        let (hear, mut chunks) = super::super::hand_off();
        let sound = Stream::start(params, hear).expect("the loopback opens");
        let (settle, end) = (Instant::now() + Duration::from_millis(500), Instant::now() + Duration::from_millis(1500));
        let mut kept = Vec::new();
        while Instant::now() < end {
            for chunk in chunks.by_ref() {
                if Instant::now() > settle {
                    kept.push(chunk);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(sound);
        kept
    }

    /// Thirty seconds of a player's tone, at [`APART_HZ`] in both channels, as a WAV file.
    fn tone_file() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("chatkord-tone-{}.wav", std::process::id()));
        let frames = RATE * 30;
        let bytes = frames * 4;
        let mut wav = Vec::new();
        for part in [&b"RIFF"[..], &(36 + bytes).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes()]
        {
            wav.extend_from_slice(part);
        }
        for part in [&2u16.to_le_bytes()[..], &RATE.to_le_bytes(), &(RATE * 4).to_le_bytes(), &4u16.to_le_bytes()] {
            wav.extend_from_slice(part);
        }
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        for at in 0..frames {
            #[expect(clippy::cast_possible_truncation, reason = "an amplitude of 100")]
            let sample = (LEVEL * (TAU * APART_HZ * f64::from(at) / f64::from(RATE)).sin()) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, wav).expect("the tone is written");
        path
    }

    /// A player of a file that Windows' management service starts, as any application on the desktop is, so it is
    /// no process of this test's tree. Ended when dropped, while its thirty seconds keep its id its own.
    struct Apart(u32);

    impl Apart {
        fn start(file: &std::path::Path) -> Option<Self> {
            let play = format!(
                "powershell -NoProfile -NonInteractive -Command (New-Object Media.SoundPlayer '{}').PlaySync()",
                file.display()
            );
            let create = format!(
                "(Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine='{}'}}).ProcessId",
                play.replace('\'', "''")
            );
            let made = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &create])
                .output()
                .ok()?;
            String::from_utf8_lossy(&made.stdout).trim().parse().ok().map(Self)
        }
    }

    impl Drop for Apart {
        fn drop(&mut self) {
            let _ = std::process::Command::new("taskkill").args(["/PID", &self.0.to_string(), "/F"]).output();
        }
    }

    /// The longest a player started apart may take to play, far above what it takes; the tone file lasts thirty.
    const PLAYER_STARTS: Duration = Duration::from_secs(20);

    /// Whether this machine has an output to play on.
    fn has_output() -> bool {
        // SAFETY: this thread's own apartment, left below.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // SAFETY: the thread is in an apartment.
        let devices: windows::core::Result<IMMDeviceEnumerator> =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) };
        // SAFETY: a live enumerator.
        let found = devices.and_then(|devices| unsafe { devices.GetDefaultAudioEndpoint(eRender, eConsole) }).is_ok();
        // SAFETY: paired with the initialisation above, the interfaces released.
        unsafe { CoUninitialize() };
        found
    }

    mod lab {
        use super::*;

        #[test]
        fn a_screens_sound_never_holds_this_processs_own_and_a_window_of_ours_would() {
            // SAFETY: reads this process's own id.
            let ours = unsafe { GetCurrentProcessId() };
            let tree = |mode| AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: ours, ProcessLoopbackMode: mode };
            // Another application's sound would hide whether ours is in it.
            let others = loudness(&heard(tree(PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)));
            if others > LEVEL / 20.0 {
                return skipped(format!("other applications are playing sound (level {others:.0})"));
            }
            let stop = Arc::new(AtomicBool::new(false));
            let (playing, started) = mpsc::sync_channel(1);
            let player = {
                let stop = stop.clone();
                std::thread::spawn(move || play(&stop, &playing))
            };
            if !started.recv().unwrap_or(false) {
                return skipped("no sound output to play a tone on");
            }
            let included = strength(&heard(tree(PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)), HZ);
            let excluded = strength(&heard(tree(PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)), HZ);
            stop.store(true, Ordering::Relaxed);
            player.join().unwrap();
            assert!(included > LEVEL / 2.0, "this process's tone is heard when its tree is asked for: {included:.1}");
            assert!(excluded < LEVEL / 20.0, "and never in all but its tree, as a screen hears: {excluded:.1}");
        }

        #[test]
        fn an_application_playing_apart_from_this_process_is_listed_and_heard_by_its_program() {
            if !has_output() {
                return skipped("no sound output to play a tone on");
            }
            let file = tone_file();
            let Some(player) = Apart::start(&file) else {
                return skipped("Windows' management service would not start a player apart from this test");
            };
            let host = crate::host::Host::enter(crate::host::Served::default()).expect("the sound system opens");
            let deadline = Instant::now() + PLAYER_STARTS;
            let listed = loop {
                let apps = host.apps(Instant::now()).expect("the applications are listed").heard();
                if let Some(app) = apps.into_iter().find(|app| app.root == player.0) {
                    break Some(app);
                }
                if Instant::now() > deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let app = listed.expect("a player started apart is listed by its own process");
            assert!(app.answers_to("powershell.exe"), "by its program: {app:?}");
            let tree = AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: app.root,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            };
            let included = strength(&heard(tree), APART_HZ);
            drop(player);
            let _ = std::fs::remove_file(&file);
            assert!(included > LEVEL / 2.0, "the application's tone is heard through its tree: {included:.1}");
        }

        /// How late a share's sound is handed over after its capture, as Linux's `pw::mix` test asks of PipeWire:
        /// the engine stamps it when pushed, so sound handed over late plays that much after its picture, which
        /// people notice past about 125 ms (ITU-R BT.1359); the chunk's 10 ms comes on top, its feed woken as it is
        /// made.
        #[test]
        fn a_shares_sound_is_handed_over_within_the_lip_sync_bound_of_its_capture() {
            const CHUNKING: Duration = Duration::from_millis(10);
            const SOUND_LATE: Duration = Duration::from_millis(125);
            if !has_output() {
                return skipped("no sound output to play a tone on");
            }
            let file = tone_file();
            let Some(player) = Apart::start(&file) else {
                return skipped("Windows' management service would not start a player apart from this test");
            };
            let tree = Hears::Tree { roots: vec![player.0], app_id: String::new() };
            let heard_so_far = Arc::new(AtomicU64::new(0));
            let counted = Arc::clone(&heard_so_far);
            // Only the player's own sound counts: the zeros the feed sends while it waits for it do not.
            let sound = crate::share::Capture::open(tree, move |chunk| {
                if chunk.iter().any(|sample| *sample != 0) {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
            })
            .expect("the player's loopback opens");
            // What is measured starts at the player's first sound, however long the machine took to start it.
            let waited = Instant::now();
            while heard_so_far.load(Ordering::Relaxed) == 0 {
                assert!(waited.elapsed() < PLAYER_STARTS, "the player was heard within {PLAYER_STARTS:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
            let start = waited.elapsed();
            let (started, mut late) = (Instant::now(), Duration::ZERO);
            while started.elapsed() < Duration::from_secs(3) {
                // The first half second is the player's buffer filling.
                if started.elapsed() > Duration::from_millis(500) {
                    late = late.max(sound.delay());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let heard = heard_so_far.load(Ordering::Relaxed);
            drop(sound);
            drop(player);
            let _ = std::fs::remove_file(&file);
            let pushed = late + CHUNKING;
            eprintln!("first heard {start:?} after the loopback opened; {heard} chunks in 3 s");
            eprintln!("handed over at most {late:?} after capture: pushed at most {pushed:?} after it");
            assert!(heard >= 270, "the player was heard for its 300 chunks of 10 ms, but for a few: {heard} chunks");
            assert!(late > Duration::ZERO, "each packet's capture time was read");
            assert!(pushed < SOUND_LATE, "a picture stamped at capture leads its sound by less than people notice");
        }
    }
}
