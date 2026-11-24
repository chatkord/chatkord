//! A call's microphone and speaker on WASAPI, shared mode, by endpoint id; none is the communications default,
//! followed when it changes. Each stream runs on a thread of its own, in its own COM apartment.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_RATEADJUST,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole, IAudioCaptureClient,
    IAudioClient, IAudioClockAdjustment, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, eCapture, eCommunications, eRender,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{HSTRING, PCWSTR, PWSTR, implement};

use super::{Capture, Health, Listed, Listing, Playback, RATE, VoiceError};
use report::{Fact, word};

/// The buffer Windows keeps for a stream, in its 100 ns units: 40 ms, a few of a device's periods.
const BUFFER: i64 = 400_000;
/// The longest a stream's thread waits for its device before it looks whether it should stop, in milliseconds.
const WAIT_MS: u32 = 100;

/// The most a speaker's endpoint is filled to, in frames: two of the device's periods, however much more buffer
/// Windows keeps, so what waits there, which no ring can take back, is as little as the device lets it be. A device
/// whose two periods are more than its buffer, or that tells no period, is filled to the buffer.
fn ceiling(period: i64, size: u32) -> u32 {
    let frames = period.saturating_mul(i64::from(RATE)) / 10_000_000;
    match u32::try_from(frames.saturating_mul(2)) {
        Ok(two) if two > 0 => two.min(size),
        Ok(_) | Err(_) => size,
    }
}

impl From<windows::core::Error> for VoiceError {
    fn from(_: windows::core::Error) -> Self {
        Self::Stream
    }
}

/// This thread's COM apartment, left when dropped.
pub(crate) struct Apartment;

impl Apartment {
    pub(crate) fn enter() -> Result<Self, VoiceError> {
        // SAFETY: this thread's own apartment, left by `Drop` after every interface made in it is released.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|_| VoiceError::Unavailable)?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: paired with `enter`; the interfaces of this apartment are dropped before it.
        unsafe { CoUninitialize() };
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator, VoiceError> {
    // SAFETY: called inside an apartment.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|_| VoiceError::Unavailable)
}

/// Reads a string Windows allocated, then frees it.
fn owned(text: PWSTR) -> Option<String> {
    // SAFETY: a nul-terminated string Windows handed over, read once.
    let read = unsafe { text.to_string() }.ok();
    // SAFETY: allocated by Windows with the task allocator, freed once, never read after.
    unsafe { CoTaskMemFree(Some(text.0.cast_const().cast())) };
    read
}

fn id(device: &IMMDevice) -> Option<String> {
    // SAFETY: a live device.
    owned(unsafe { device.GetId() }.ok()?)
}

/// The communications default for `flow`, taken again by its endpoint id. Windows takes a stream on a device got
/// through the communications role for a call's and lowers every other application while it plays (Sound,
/// Communications); the same device got by its id, as a chosen one is, leaves them as they are.
pub(crate) fn communications_default(
    devices: &IMMDeviceEnumerator,
    flow: EDataFlow,
) -> windows::core::Result<IMMDevice> {
    // SAFETY: a live enumerator.
    let default = unsafe { devices.GetDefaultAudioEndpoint(flow, eCommunications) }?;
    // SAFETY: a live device.
    let id = unsafe { default.GetId() }?;
    // SAFETY: a live enumerator and the id Windows just gave, read during the call.
    let device = unsafe { devices.GetDevice(PCWSTR(id.0.cast_const())) };
    // SAFETY: allocated by Windows with the task allocator, freed once, never read after.
    unsafe { CoTaskMemFree(Some(id.0.cast_const().cast())) };
    device
}

fn label(device: &IMMDevice) -> Option<String> {
    // SAFETY: a live device.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok()?;
    // SAFETY: a live store and a key that lives across the call.
    let value = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.ok()?;
    // SAFETY: a value the store handed over, alive across the call.
    owned(unsafe { PropVariantToStringAlloc(&raw const value) }.ok()?)
}

fn listed(devices: &IMMDeviceEnumerator, flow: EDataFlow) -> Result<(Vec<Listed>, Option<String>), VoiceError> {
    // SAFETY: a live enumerator.
    let collection = unsafe { devices.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) }?;
    // SAFETY: a live collection.
    let count = unsafe { collection.GetCount() }?;
    let listed = (0..count)
        // SAFETY: an index under the collection's count.
        .filter_map(|index| unsafe { collection.Item(index) }.ok())
        .filter_map(|device| Some(Listed { id: id(&device)?, label: label(&device)? }))
        .collect();
    // SAFETY: a live enumerator.
    let default = unsafe { devices.GetDefaultAudioEndpoint(flow, eCommunications) }.ok().and_then(|device| id(&device));
    Ok((listed, default))
}

/// The microphones and speakers `devices` lists as active, by their endpoint ids, and its communications defaults.
pub(crate) fn listing(devices: &IMMDeviceEnumerator) -> Result<Listing, VoiceError> {
    let (microphones, default_microphone) = listed(devices, eCapture)?;
    let (speakers, default_speaker) = listed(devices, eRender)?;
    Ok(Listing { microphones, speakers, default_microphone, default_speaker })
}

/// What a stream's thread tells its owner, through atomics only.
#[derive(Debug, Default)]
struct Status {
    /// `Health` as `HEALTH`'s index.
    health: AtomicU8,
    /// The stream's delay, in microseconds.
    delay: AtomicU64,
    /// The speaker's rate as `f32` bits; zero is unset.
    rate: AtomicU32,
    stop: AtomicBool,
    /// The communications default moved while the stream follows it.
    moved: AtomicBool,
}

const HEALTH: [Health; 3] = [Health::Starting, Health::Running, Health::Stopped];

impl Status {
    fn health(&self) -> Health {
        HEALTH[usize::from(self.health.load(Ordering::Acquire)).min(2)]
    }

    fn set(&self, health: Health) {
        let index = HEALTH.iter().position(|known| *known == health).unwrap_or(2);
        #[expect(clippy::cast_possible_truncation, reason = "one of three")]
        self.health.store(index as u8, Ordering::Release);
    }

    fn set_delay(&self, delay: Duration) {
        self.delay.store(u64::try_from(delay.as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// Tells a stream that follows the default when Windows moves the communications default of its direction.
#[implement(IMMNotificationClient)]
struct Moved {
    flow: EDataFlow,
    status: Arc<Status>,
}

impl IMMNotificationClient_Impl for Moved_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> windows::core::Result<()> {
        if flow == self.flow && role == eCommunications {
            self.status.moved.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// An initialised shared-mode client on the endpoint asked for, its event, and the registration that follows the
/// default when none was asked for. Stopped and released when dropped.
struct Opened {
    client: IAudioClient,
    flow: EDataFlow,
    ready: HANDLE,
    devices: IMMDeviceEnumerator,
    following: Option<IMMNotificationClient>,
}

impl Opened {
    /// Opens `device` (none: the communications default) for 16-bit PCM of `channels` at [`RATE`].
    fn open(
        flow: EDataFlow,
        device: Option<&str>,
        channels: u16,
        flags: u32,
        status: &Arc<Status>,
    ) -> Result<Self, VoiceError> {
        let devices = enumerator()?;
        let endpoint = match device {
            // SAFETY: a live enumerator; the id is copied into a string Windows reads during the call.
            Some(id) => unsafe { devices.GetDevice(&HSTRING::from(id)) }?,
            None => communications_default(&devices, flow)?,
        };
        let following = match device {
            Some(_) => None,
            None => {
                let moved: IMMNotificationClient = Moved { flow, status: Arc::clone(status) }.into();
                // SAFETY: our live object, unregistered by `Drop` before it goes.
                unsafe { devices.RegisterEndpointNotificationCallback(&moved) }?;
                Some(moved)
            }
        };
        // SAFETY: a live device.
        let client: IAudioClient = unsafe { endpoint.Activate(CLSCTX_ALL, None) }?;
        #[expect(clippy::cast_possible_truncation, reason = "the PCM tag is 1")]
        let tag = WAVE_FORMAT_PCM as u16;
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
        let flags = flags
            | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        // SAFETY: the format lives across the call, which copies it.
        unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER, 0, &raw const format, None) }?;
        // SAFETY: an auto-reset event with no name, closed by `Drop`.
        let ready = unsafe { CreateEventW(None, false, false, None) }?;
        let opened = Self { client, flow, ready, devices, following };
        // SAFETY: an initialised client and an event that lives as long as it.
        unsafe { opened.client.SetEventHandle(opened.ready) }?;
        Ok(opened)
    }

    /// The stream's own latency.
    fn latency(&self) -> Duration {
        // SAFETY: an initialised client.
        let hundreds = unsafe { self.client.GetStreamLatency() }.unwrap_or(0);
        Duration::from_nanos(u64::try_from(hundreds).unwrap_or(0).saturating_mul(100))
    }

    /// Waits for the device to ask or hand over, saying whether it did; none when the stream should end: told to
    /// stop, or the default it follows moved.
    fn wait(&self, status: &Status) -> Option<bool> {
        if status.stop.load(Ordering::Acquire) {
            return None;
        }
        if status.moved.load(Ordering::Acquire) {
            let end = if self.flow == eCapture { word!("microphone") } else { word!("speaker") };
            report::note!(info, "a stream ended as the default device it follows moved", end = Fact::Word(end));
            return None;
        }
        // SAFETY: the event this client signals, alive while `self` is.
        Some(unsafe { WaitForSingleObject(self.ready, WAIT_MS) } == WAIT_OBJECT_0)
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: a client of this apartment; stopping a stopped one is harmless.
        let _ = unsafe { self.client.Stop() };
        if let Some(following) = &self.following {
            // SAFETY: the object registered by `open` on this enumerator.
            let _ = unsafe { self.devices.UnregisterEndpointNotificationCallback(following) };
        }
        // SAFETY: the event made in `open`, used by nothing once the client stopped.
        let _ = unsafe { CloseHandle(self.ready) };
    }
}

/// A stream's thread and what it tells; dropping it stops the thread and waits for it.
struct Running {
    status: Arc<Status>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    /// Runs `serve` on a thread of its own once `open` has opened the stream there; blocks until it has or refused.
    fn start<T: 'static>(
        name: &str,
        open: impl FnOnce(&Arc<Status>) -> Result<T, VoiceError> + Send + 'static,
        serve: impl FnOnce(T, &Status) + Send + 'static,
    ) -> Result<Self, VoiceError> {
        let status = Arc::new(Status::default());
        let (opened, open_said) = mpsc::sync_channel(1);
        let told = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let apartment = match Apartment::enter() {
                    Ok(apartment) => apartment,
                    Err(why) => {
                        let _ = opened.send(Err(why));
                        return;
                    }
                };
                match open(&told) {
                    Ok(stream) => {
                        let _ = opened.send(Ok(()));
                        let ahead = super::realtime();
                        serve(stream, &told);
                        drop(ahead);
                    }
                    Err(why) => {
                        let _ = opened.send(Err(why));
                    }
                }
                told.set(Health::Stopped);
                drop(apartment);
            })
            .map_err(|_| VoiceError::Unavailable)?;
        match open_said.recv() {
            Ok(Ok(())) => Ok(Self { status, thread: Some(thread) }),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => Err(VoiceError::Stream),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.status.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A microphone stream: one channel.
pub(super) struct Input(Running);

impl Capture for Input {
    fn open(device: Option<&str>, mut hear: impl FnMut(&[i16]) + Send + 'static) -> Result<Self, VoiceError> {
        let device = device.map(str::to_owned);
        let open = move |status: &Arc<Status>| {
            let opened = Opened::open(eCapture, device.as_deref(), 1, 0, status)?;
            // SAFETY: an initialised client.
            let reader = unsafe { opened.client.GetService::<IAudioCaptureClient>() }?;
            // SAFETY: an initialised client with its event set.
            unsafe { opened.client.Start() }?;
            Ok((opened, reader))
        };
        let serve = move |(opened, reader): (Opened, IAudioCaptureClient), status: &Status| {
            status.set_delay(opened.latency());
            while let Some(ready) = opened.wait(status) {
                if ready && read(&reader, &mut hear, status).is_err() {
                    return;
                }
                status.set(Health::Running);
            }
        };
        Running::start("chatkord-microphone", open, serve).map(Self)
    }

    fn health(&self) -> Health {
        self.0.status.health()
    }

    fn delay(&self) -> Duration {
        Duration::from_micros(self.0.status.delay.load(Ordering::Relaxed))
    }
}

/// Hands every packet waiting to `hear`, a silent one as zeros, and says in `status` how long after its capture the
/// packet was handed over (Windows stamps it on the performance counter). A device gone fails it
/// (`AUDCLNT_E_DEVICE_INVALIDATED`), which ends the stream.
fn read(reader: &IAudioCaptureClient, hear: &mut impl FnMut(&[i16]), status: &Status) -> windows::core::Result<()> {
    const QUIET: [i16; 480] = [0; 480];
    loop {
        let (mut data, mut frames, mut flags, mut captured) = (std::ptr::null_mut(), 0u32, 0u32, 0u64);
        // SAFETY: the reader of a started stream; a packet taken is released below before the next.
        unsafe { reader.GetBuffer(&raw mut data, &raw mut frames, &raw mut flags, None, Some(&raw mut captured)) }?;
        if frames == 0 {
            return Ok(());
        }
        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
        if silent || data.is_null() {
            for piece in (0..frames as usize).step_by(QUIET.len()) {
                hear(&QUIET[..QUIET.len().min(frames as usize - piece)]);
            }
        } else {
            // SAFETY: Windows lent `frames` frames of one 16-bit channel at `data`, aligned for it, until the release.
            hear(unsafe { std::slice::from_raw_parts(data.cast::<i16>(), frames as usize) });
        }
        if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0
            && let Some(at) = i64::try_from(captured).ok().and_then(clock::instant_of)
        {
            status.set_delay(at.elapsed());
        }
        // SAFETY: releases exactly the packet taken above.
        unsafe { reader.ReleaseBuffer(frames) }?;
    }
}

/// A speaker stream: two channels, its rate set through the clock adjustment `RATEADJUST` offers.
pub(super) struct Output(Running);

impl Playback for Output {
    fn open(device: Option<&str>, mut fill: impl FnMut(&mut [i16]) + Send + 'static) -> Result<Self, VoiceError> {
        let device = device.map(str::to_owned);
        let open = move |status: &Arc<Status>| {
            let opened = Opened::open(eRender, device.as_deref(), 2, AUDCLNT_STREAMFLAGS_RATEADJUST, status)?;
            // SAFETY: an initialised client.
            let writer = unsafe { opened.client.GetService::<IAudioRenderClient>() }?;
            // SAFETY: an initialised client opened with `RATEADJUST`.
            let clock = unsafe { opened.client.GetService::<IAudioClockAdjustment>() }?;
            // SAFETY: an initialised client.
            let size = unsafe { opened.client.GetBufferSize() }?;
            let mut period = 0_i64;
            // SAFETY: an initialised client; writes the one i64 it is given a pointer to.
            unsafe { opened.client.GetDevicePeriod(Some(&raw mut period), None) }?;
            Ok((opened, writer, clock, ceiling(period, size)))
        };
        let serve = move |(opened, writer, clock, size): (Opened, IAudioRenderClient, IAudioClockAdjustment, u32),
                          status: &Status| {
            let mut quantum = Vec::new();
            if write(&opened, &writer, size, &mut quantum, &mut fill, status).is_err() {
                return;
            }
            // SAFETY: an initialised client with its first period written.
            if unsafe { opened.client.Start() }.is_err() {
                return;
            }
            let mut rate = 0;
            while let Some(ready) = opened.wait(status) {
                let asked = status.rate.load(Ordering::Relaxed);
                if asked != rate {
                    rate = asked;
                    // SAFETY: the clock adjustment of this started client, on its own thread.
                    let _ = unsafe { clock.SetSampleRate(f32::from_bits(asked) * RATE as f32) };
                }
                if ready && write(&opened, &writer, size, &mut quantum, &mut fill, status).is_err() {
                    return;
                }
                status.set(Health::Running);
            }
        };
        Running::start("chatkord-speaker", open, serve).map(Self)
    }

    fn health(&self) -> Health {
        self.0.status.health()
    }

    fn delay(&self) -> Duration {
        Duration::from_micros(self.0.status.delay.load(Ordering::Relaxed))
    }

    fn set_rate(&self, ratio: f32) {
        self.0.status.rate.store(ratio.to_bits(), Ordering::Relaxed);
    }
}

/// Fills the endpoint up to `size` frames waiting (see [`ceiling`]), and tells how long what waits there takes to
/// play. A device gone fails it, which ends the stream.
fn write(
    opened: &Opened,
    writer: &IAudioRenderClient,
    size: u32,
    quantum: &mut Vec<i16>,
    fill: &mut impl FnMut(&mut [i16]),
    status: &Status,
) -> windows::core::Result<()> {
    // SAFETY: an initialised client.
    let waiting = unsafe { opened.client.GetCurrentPadding() }?;
    let room = size.saturating_sub(waiting);
    status.set_delay(opened.latency() + Duration::from_micros(u64::from(waiting) * 1_000_000 / u64::from(RATE)));
    if room == 0 {
        return Ok(());
    }
    let samples = room as usize * 2;
    quantum.resize(samples, 0);
    fill(quantum);
    // SAFETY: an initialised client's writer, asked for no more than the room it has.
    let buffer = unsafe { writer.GetBuffer(room) }?;
    // SAFETY: Windows lent `room` frames of two 16-bit channels at `buffer`, aligned for them, until the release.
    unsafe { std::slice::from_raw_parts_mut(buffer.cast::<i16>(), samples) }.copy_from_slice(quantum);
    // SAFETY: releases exactly the frames taken above.
    unsafe { writer.ReleaseBuffer(room, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_filled_to_two_periods_not_its_whole_buffer() {
        // A 10 ms period is 480 frames: two of them, in a buffer of 40 ms (1 920 frames).
        assert_eq!(ceiling(100_000, 1_920), 960);
        assert_eq!(ceiling(100_000, 600), 600, "never past what the endpoint holds");
        assert_eq!(ceiling(0, 1_920), 1_920, "a device that tells no period is filled as Windows sized it");
        assert_eq!(ceiling(-1, 1_920), 1_920);
    }
}
