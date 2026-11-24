//! Sounds on Windows: a shared-mode WASAPI stream for a sound on the chosen endpoint, whose buffer holds the whole
//! sound, stopped once it has played out. It plays from the owner thread, in the apartment and on the enumerator the
//! owner holds.

use std::sync::Arc;

use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eRender,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::HSTRING;

use super::{PlayError, RATE};
use crate::voice::Apartment;

/// The endpoint's buffer beyond the sound's own length, in 100 ns units: 50 ms of slack.
const SLACK: i64 = 500_000;

impl From<windows::core::Error> for PlayError {
    fn from(_: windows::core::Error) -> Self {
        Self::Refused
    }
}

/// Where chimes are played from: a COM apartment of the thread that made it and the device enumerator, which outlive
/// the [`Chimes`] that play from it.
pub struct Stage {
    devices: IMMDeviceEnumerator,
    _apartment: Apartment,
}

impl Stage {
    /// Enters this thread's apartment, which the stage leaves when dropped, and opens the enumerator.
    pub fn enter() -> Result<Self, PlayError> {
        let apartment = Apartment::enter().map_err(|_| PlayError::Unavailable)?;
        // SAFETY: the thread is in the apartment entered above, which `Stage` keeps for as long as the enumerator.
        let devices: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|_| PlayError::Unavailable)?;
        Ok(Self { devices, _apartment: apartment })
    }

    /// The enumerator the stage holds, for what else the thread asks of the endpoints.
    pub(crate) fn devices(&self) -> &IMMDeviceEnumerator {
        &self.devices
    }
}

/// Plays one sound at a time, the newest in place of one still playing, from the thread that holds the stage.
/// Dropping them ends the sound playing.
pub struct Chimes<'stage> {
    stage: &'stage Stage,
    playing: Option<Sounding>,
}

impl<'stage> Chimes<'stage> {
    /// Chimes that play from `stage`, nothing started until the first.
    pub fn new(stage: &'stage Stage) -> Self {
        Self { stage, playing: None }
    }

    /// Looks for the speaker a sound plays on now, so the first sound plays at once.
    pub fn open(&mut self) -> Result<(), PlayError> {
        crate::voice::communications_default(&self.stage.devices, eRender)
            .map(|_| ())
            .map_err(|_| PlayError::Unavailable)
    }

    /// Plays `samples`, one channel at [`RATE`] from -1 to 1, on the speaker with the sound server's id `speaker`
    /// (the endpoint id), or the default; one no longer there plays on the default. Returns once the sound has
    /// started, a few milliseconds.
    pub fn play(&mut self, samples: Arc<[f32]>, speaker: Option<&str>) -> Result<(), PlayError> {
        // The sound it replaces is cut before the next starts, so two never play at once.
        self.playing = None;
        let mut sounding = Sounding::start(&self.stage.devices, speaker, samples)?;
        sounding.begin()?;
        self.playing = Some(sounding);
        Ok(())
    }

    /// Whether a sound is playing, so its stream is looked at soon for having drained.
    pub fn playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Takes down a sound that has drained.
    pub fn settle(&mut self) {
        if self.playing.as_mut().is_some_and(Sounding::drained) {
            self.playing = None;
        }
    }
}

/// One sound's stream, stopped when dropped.
struct Sounding {
    client: IAudioClient,
    writer: IAudioRenderClient,
    samples: Arc<[f32]>,
    size: u32,
    sent: usize,
}

impl Sounding {
    /// The speaker named by `speaker`, or the communications default (the call's own default) when it names none or
    /// one that is gone, set up for one channel of 32-bit floats with room for all of `samples`.
    fn start(devices: &IMMDeviceEnumerator, speaker: Option<&str>, samples: Arc<[f32]>) -> Result<Self, PlayError> {
        let chosen = speaker.and_then(|id| {
            // SAFETY: a live enumerator; the id is copied into a string Windows reads during the call.
            unsafe { devices.GetDevice(&HSTRING::from(id)) }.ok()
        });
        let device = match chosen {
            Some(device) => device,
            None => crate::voice::communications_default(devices, eRender)?,
        };
        // SAFETY: a live device.
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }?;
        #[expect(clippy::cast_possible_truncation, reason = "the float tag is 3")]
        let tag = WAVE_FORMAT_IEEE_FLOAT as u16;
        let format = WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: 1,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 4,
            nBlockAlign: 4,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        let whole = i64::try_from(samples.len() as u64 * 10_000_000 / u64::from(RATE)).unwrap_or(i64::MAX);
        let buffer = whole.saturating_add(SLACK);
        // SAFETY: the format lives across the call, which copies it.
        unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, buffer, 0, &raw const format, None) }?;
        Ok(Self {
            // SAFETY: an initialised client.
            writer: unsafe { client.GetService::<IAudioRenderClient>() }?,
            // SAFETY: an initialised client.
            size: unsafe { client.GetBufferSize() }?,
            client,
            samples,
            sent: 0,
        })
    }

    /// Writes the first of the sound and starts the stream.
    fn begin(&mut self) -> Result<(), PlayError> {
        self.feed()?;
        // SAFETY: an initialised client with its first period written.
        unsafe { self.client.Start() }?;
        Ok(())
    }

    /// Whether the sound has played out; what the buffer had no room for yet is written first.
    fn drained(&mut self) -> bool {
        if self.sent < self.samples.len() {
            return self.feed().is_err();
        }
        // SAFETY: a started client.
        unsafe { self.client.GetCurrentPadding() }.map_or(true, |waiting| waiting == 0)
    }

    /// Writes as many of the samples left as the endpoint's buffer has room for.
    fn feed(&mut self) -> windows::core::Result<()> {
        // SAFETY: an initialised client.
        let room = self.size.saturating_sub(unsafe { self.client.GetCurrentPadding() }?);
        let left = self.samples.get(self.sent..).unwrap_or_default();
        let count = left.len().min(room as usize);
        if count == 0 {
            return Ok(());
        }
        let frames = u32::try_from(count).unwrap_or(room);
        // SAFETY: an initialised client's writer, asked for no more than the room it has.
        let buffer = unsafe { self.writer.GetBuffer(frames) }?;
        // SAFETY: Windows lent `frames` frames of one 32-bit float at `buffer` until the release below.
        let into = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<f32>(), count) };
        into.copy_from_slice(&left[..count]);
        // SAFETY: releases exactly the frames taken above.
        unsafe { self.writer.ReleaseBuffer(frames, 0) }?;
        self.sent += count;
        Ok(())
    }
}

impl Drop for Sounding {
    fn drop(&mut self) {
        // SAFETY: a client of this apartment; stopping a stopped one is harmless.
        let _ = unsafe { self.client.Stop() };
    }
}
