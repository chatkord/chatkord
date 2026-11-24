//! Windows' cameras through Media Foundation's source reader, asynchronous as Chromium's capture is: the reader hands
//! each picture to a callback, and a read waits on our own channel, never inside Media Foundation, so a camera that
//! stops answering (disabled, unplugged, its driver stuck) is ended by a stop or a removal told into that channel. The
//! mode is one of the camera's own, so nothing is scaled, and the reader's video processing turns its YUY2 or MJPEG
//! into NV12 in memory.

mod plugged;

pub use plugged::{Watch, watch};

/// What the reader's callback, a stop or a removal tells a read.
enum Arrived {
    /// A picture, and when it arrived.
    Picture(Sample, Instant),
    /// A gap in the stream: no picture this time.
    Gap,
    /// The camera ended, and why.
    Ended(CameraError),
}

/// A picture from the reader's thread.
struct Sample(IMFSample);

// SAFETY: Media Foundation's samples are free-threaded; one is handed over once and read by its receiver alone.
unsafe impl Send for Sample {}

/// Ends a camera from any thread: a read waiting on it returns at once as gone.
#[derive(Clone)]
pub struct Stopper(mpsc::Sender<Arrived>);

impl Stopper {
    /// Ends the camera, whatever it is doing.
    pub fn stop(&self) {
        let _ = self.0.send(Arrived::Ended(CameraError::Gone));
    }
}

/// The reader's callback: each answer to a read, sent to the camera.
#[implement(IMFSourceReaderCallback)]
struct Told(mpsc::Sender<Arrived>);

impl IMFSourceReaderCallback_Impl for Told_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        _timestamp: i64,
        sample: Ref<'_, IMFSample>,
    ) -> windows::core::Result<()> {
        let ended = (MF_SOURCE_READERF_ENDOFSTREAM.0 | MF_SOURCE_READERF_ERROR.0).cast_unsigned();
        let arrived = match (status.is_ok(), flags & ended == 0, sample.as_ref()) {
            (false, ..) => Arrived::Ended(refusal(status)),
            (true, false, _) => Arrived::Ended(CameraError::Gone),
            (true, true, Some(sample)) => Arrived::Picture(Sample(sample.clone()), Instant::now()),
            (true, true, None) => Arrived::Gap,
        };
        let _ = self.0.send(arrived);
        Ok(())
    }

    fn OnFlush(&self, _stream: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnEvent(&self, _stream: u32, _event: Ref<'_, IMFMediaEvent>) -> windows::core::Result<()> {
        Ok(())
    }
}

use super::{CameraError, Device, Mode, Pixel, Shot};
use std::cell::Cell;
use std::sync::mpsc;
use std::time::Instant;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_Interface_PropertyW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW,
    CONFIGRET, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_FriendlyName, DEVPKEY_Device_InstanceId, DEVPROP_TYPE_STRING,
    DEVPROPTYPE,
};
use windows::Win32::Foundation::{DEVPROPKEY, E_ACCESSDENIED};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaEvent, IMFMediaSource, IMFMediaType, IMFSample, IMFSourceReader,
    IMFSourceReaderCallback, IMFSourceReaderCallback_Impl, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_E_HW_MFT_FAILED_START_STREAMING, MF_E_SHUTDOWN,
    MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED, MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ASYNC_CALLBACK, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION,
    MFCreateAttributes, MFCreateDeviceSource, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup, MFVideoFormat_NV12,
    MFVideoFormat_YUY2,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::core::{GUID, HRESULT, HSTRING, IUnknown, Interface, PWSTR, Ref, implement};

/// The reader's first video stream, as its calls take it.
const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.cast_unsigned();

/// The cameras plugged in now, in the order Windows lists them.
pub fn list() -> Result<Vec<Device>, CameraError> {
    let _started = Started::enter()?;
    let cameras = cameras()?;
    let (mut found, mut count) = (std::ptr::null_mut(), 0_u32);
    // SAFETY: both outputs are written by the call; the array is then this function's to free.
    unsafe { MFEnumDeviceSources(&cameras, &raw mut found, &raw mut count) }.map_err(error_of)?;
    let activates: Vec<_> = (0..count as usize)
        .map(|at| {
            // SAFETY: `found` holds `count` interfaces, so `at` is inside it.
            let each = unsafe { found.add(at) };
            // SAFETY: each interface is moved out once, with its reference.
            unsafe { each.read() }
        })
        .collect();
    // SAFETY: allocated by Media Foundation with the task allocator, freed once, its interfaces moved out first.
    unsafe { CoTaskMemFree(Some(found.cast_const().cast())) };
    Ok(activates
        .iter()
        .flatten()
        .filter_map(|camera| {
            let id = string(camera, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?;
            let label = device_name(&id).or_else(|| string(camera, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME))?;
            Some(Device { id, label })
        })
        .collect())
}

/// The modes the camera `id` names captures at, read from its own types without streaming, so it never lights.
pub fn modes(id: &str) -> Result<Vec<Mode>, CameraError> {
    let _started = Started::enter()?;
    let device = cameras()?;
    // SAFETY: a live store; the string is copied in.
    unsafe { device.SetString(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, &HSTRING::from(id)) }
        .map_err(error_of)?;
    // SAFETY: a store naming one camera.
    let source = Source(unsafe { MFCreateDeviceSource(&device) }.map_err(error_of)?);
    // SAFETY: a live source; no store is needed to read types.
    let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source.0, None) }.map_err(error_of)?;
    Ok(natives(&reader).into_iter().map(|(mode, _)| mode).collect())
}

/// An open camera, read on the thread that opened it. Dropping it lets the camera go.
pub struct Camera {
    reader: IMFSourceReader,
    /// The reader's answers, a stop and a removal, in the order they came.
    arrivals: mpsc::Receiver<Arrived>,
    /// What a stop or a removal tells.
    ends: mpsc::Sender<Arrived>,
    /// A read is asked of the reader and not yet answered.
    asked: bool,
    /// The pictures the reader answered with; each is asked for, so none is dropped before a read.
    delivered: u64,
    _source: Source,
    mode: Mode,
    /// The bytes from one row's start to the next's, when the reader's buffer is not a 2D one.
    stride: usize,
    /// The memory of a picture given back, for the next to be put in.
    spare: Cell<Option<Vec<u8>>>,
    /// After the source, so this process's next opener of the camera waits until it is shut down.
    _held: Option<plugged::Held>,
    _started: Started,
}

impl Camera {
    /// Opens the camera `id` names ([`Device::id`]) at the mode of its own nearest `asked` ([`Mode::nearest`]).
    pub fn open(id: &str, asked: Mode) -> Result<Self, CameraError> {
        let started = Started::enter()?;
        let device = cameras()?;
        let link = HSTRING::from(id);
        // SAFETY: a live store; the string is copied in.
        unsafe { device.SetString(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, &link) }
            .map_err(error_of)?;
        let (ends, arrivals) = mpsc::channel();
        let held = plugged::hold(id, Stopper(ends.clone()));
        // SAFETY: a store naming one camera.
        let source = Source(unsafe { MFCreateDeviceSource(&device) }.map_err(error_of)?);
        let reading = attributes(2)?;
        // SAFETY: a live store.
        unsafe { reading.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1) }.map_err(error_of)?;
        let told: IUnknown = IMFSourceReaderCallback::from(Told(ends.clone())).into();
        // SAFETY: a live store; the reader keeps its own reference to the callback.
        unsafe { reading.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &told) }.map_err(error_of)?;
        // SAFETY: a live source and store.
        let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source.0, &reading) }.map_err(error_of)?;
        // SAFETY: a live reader; a camera's other streams (a still pin) are never read.
        unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0.cast_unsigned(), false) }
            .map_err(error_of)?;
        // SAFETY: a live reader.
        unsafe { reader.SetStreamSelection(STREAM, true) }.map_err(error_of)?;
        let natives = natives(&reader);
        let offered: Vec<Mode> = natives.iter().map(|(mode, _)| *mode).collect();
        let mode = Mode::nearest(&offered, asked).ok_or(CameraError::Format)?;
        let native = natives
            .iter()
            .filter(|(each, _)| *each == mode)
            .min_by_key(|(_, kind)| decoding(kind))
            .map(|(_, kind)| kind)
            .ok_or(CameraError::Format)?;
        // SAFETY: a live reader and one of its stream's own types, which the camera is set to.
        unsafe { reader.SetCurrentMediaType(STREAM, None, native) }.map_err(error_of)?;
        let output = nv12_like(native)?;
        // SAFETY: a live reader and type; the reader's processing makes NV12 of the camera's own.
        unsafe { reader.SetCurrentMediaType(STREAM, None, &output) }.map_err(|_| CameraError::Format)?;
        // SAFETY: a live reader.
        let current = unsafe { reader.GetCurrentMediaType(STREAM) }.map_err(error_of)?;
        // SAFETY: a live type; an absent stride is a packed row.
        let stride = unsafe { current.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
            .ok()
            .map_or(mode.width as usize, |stride| stride as usize);
        Ok(Self {
            reader,
            arrivals,
            ends,
            asked: false,
            delivered: 0,
            _held: held,
            _source: source,
            mode,
            stride,
            spare: Cell::new(None),
            _started: started,
        })
    }

    /// The mode it captures at.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// How many pictures the camera has handed over since it opened: what it chose to send, which in dim light is
    /// fewer than its mode's rate.
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// What ends it from another thread, a read under way included.
    pub fn stopper(&self) -> Stopper {
        Stopper(self.ends.clone())
    }

    /// The next picture and when it arrived, waiting for it. A camera unplugged, taken away or stopped is
    /// [`CameraError::Gone`], whatever its driver does; its memory goes back through [`Camera::recycle`].
    pub fn read(&mut self) -> Result<(Shot, Instant), CameraError> {
        loop {
            if !self.asked {
                // SAFETY: a live reader in asynchronous mode: the answer comes to the callback.
                unsafe { self.reader.ReadSample(STREAM, 0, None, None, None, None) }.map_err(error_of)?;
                self.asked = true;
            }
            match self.arrivals.recv() {
                Ok(Arrived::Picture(Sample(sample), captured)) => {
                    self.asked = false;
                    self.delivered += 1;
                    return Ok((self.shot(&sample)?, captured));
                }
                Ok(Arrived::Gap) => self.asked = false,
                Ok(Arrived::Ended(why)) => return Err(why),
                // This camera holds a sender, so the channel never closes while it reads.
                Err(mpsc::RecvError) => return Err(CameraError::Gone),
            }
        }
    }

    /// Gives a picture's memory back, so the next is put in it.
    pub fn recycle(&self, shot: Shot) {
        self.spare.set(Some(shot.into_memory()));
    }

    fn shot(&self, sample: &IMFSample) -> Result<Shot, CameraError> {
        let (width, height) = (self.mode.width as usize, self.mode.height as usize);
        let size = (self.mode.width, self.mode.height);
        // SAFETY: a live sample.
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(error_of)?;
        if let Ok(plane) = buffer.cast::<IMF2DBuffer>() {
            let (mut first, mut pitch) = (std::ptr::null_mut(), 0_i32);
            // SAFETY: a live buffer, unlocked below.
            unsafe { plane.Lock2D(&raw mut first, &raw mut pitch) }.map_err(error_of)?;
            // A negative pitch is a picture stored bottom up, which NV12 never is.
            let taken = usize::try_from(pitch).ok().and_then(|pitch| {
                let length = pitch.checked_mul(height + height / 2)?;
                // SAFETY: while locked, an NV12 picture's rows of `pitch` bytes each lie from `first`.
                let bytes = unsafe { std::slice::from_raw_parts(first, length) };
                let picture = rows(bytes, pitch, width, height)?;
                Some(Shot::new(picture, Some(u32::try_from(pitch).ok()?), Pixel::Nv12, size, self.spare.take()))
            });
            // SAFETY: locked above, and no slice of it outlives this.
            let _ = unsafe { plane.Unlock2D() };
            return taken.ok_or(CameraError::Format);
        }
        let (mut start, mut length) = (std::ptr::null_mut(), 0_u32);
        // SAFETY: a live buffer, unlocked below.
        unsafe { buffer.Lock(&raw mut start, None, Some(&raw mut length)) }.map_err(error_of)?;
        // SAFETY: while locked, `length` bytes lie from `start`.
        let bytes = unsafe { std::slice::from_raw_parts(start, length as usize) };
        let taken = rows(bytes, self.stride, width, height).and_then(|picture| {
            Some(Shot::new(picture, Some(u32::try_from(self.stride).ok()?), Pixel::Nv12, size, self.spare.take()))
        });
        // SAFETY: locked above, and `bytes` is not read after.
        let _ = unsafe { buffer.Unlock() };
        taken.ok_or(CameraError::Format)
    }
}

/// The NV12 rows of `bytes` that hold a `width` by `height` picture whose rows start every `pitch` bytes, luma's then
/// chroma's, the last row only as long as the picture is wide; none when they are not all there.
fn rows(bytes: &[u8], pitch: usize, width: usize, height: usize) -> Option<&[u8]> {
    let rows = height + height / 2;
    let needed = pitch.checked_mul(rows.checked_sub(1)?)?.checked_add(width)?;
    if pitch < width || rows == 0 || bytes.len() < needed {
        return None;
    }
    Some(&bytes[..bytes.len().min(pitch.checked_mul(rows)?)])
}

/// The camera's own types on its video stream, with the mode each is.
fn natives(reader: &IMFSourceReader) -> Vec<(Mode, IMFMediaType)> {
    (0..)
        .map_while(|at| {
            // SAFETY: a live reader; past its last type the call fails, which ends the list.
            unsafe { reader.GetNativeMediaType(STREAM, at) }.ok()
        })
        .filter_map(|kind| Some((mode_of(&kind)?, kind)))
        .collect()
}

fn mode_of(kind: &IMFMediaType) -> Option<Mode> {
    let (width, height) = halves(read_u64(kind, &MF_MT_FRAME_SIZE)?);
    let (numerator, denominator) = halves(read_u64(kind, &MF_MT_FRAME_RATE)?);
    let frames_per_second = numerator.checked_add(denominator / 2)?.checked_div(denominator)?;
    Some(Mode { width, height, frames_per_second })
}

/// Media Foundation's two 32-bit numbers in one: the high one first.
fn halves(both: u64) -> (u32, u32) {
    let [a, b, c, d, e, f, g, h] = both.to_be_bytes();
    (u32::from_be_bytes([a, b, c, d]), u32::from_be_bytes([e, f, g, h]))
}

fn read_u64(kind: &IMFMediaType, key: &GUID) -> Option<u64> {
    // SAFETY: a live type.
    unsafe { kind.GetUINT64(key) }.ok()
}

/// How much the reader has to do to make NV12 of a type: nothing, a shuffle, or a decode.
fn decoding(kind: &IMFMediaType) -> u8 {
    // SAFETY: a live type.
    match unsafe { kind.GetGUID(&MF_MT_SUBTYPE) } {
        Ok(subtype) if subtype == MFVideoFormat_NV12 => 0,
        Ok(subtype) if subtype == MFVideoFormat_YUY2 => 1,
        _ => 2,
    }
}

/// NV12 at the size and rate of `native`.
fn nv12_like(native: &IMFMediaType) -> Result<IMFMediaType, CameraError> {
    // SAFETY: Media Foundation is started on this thread.
    let output = unsafe { MFCreateMediaType() }.map_err(error_of)?;
    let size = read_u64(native, &MF_MT_FRAME_SIZE).ok_or(CameraError::Format)?;
    let rate = read_u64(native, &MF_MT_FRAME_RATE).ok_or(CameraError::Format)?;
    // SAFETY: a live type, each value copied in.
    unsafe { output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }.map_err(error_of)?;
    // SAFETY: as above.
    unsafe { output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12) }.map_err(error_of)?;
    // SAFETY: as above.
    unsafe { output.SetUINT64(&MF_MT_FRAME_SIZE, size) }.map_err(error_of)?;
    // SAFETY: as above.
    unsafe { output.SetUINT64(&MF_MT_FRAME_RATE, rate) }.map_err(error_of)?;
    Ok(output)
}

fn attributes(size: u32) -> Result<IMFAttributes, CameraError> {
    let mut made = None;
    // SAFETY: written by the call.
    unsafe { MFCreateAttributes(&raw mut made, size) }.map_err(error_of)?;
    made.ok_or(CameraError::Unavailable)
}

/// A store that asks for video capture devices.
fn cameras() -> Result<IMFAttributes, CameraError> {
    let cameras = attributes(2)?;
    // SAFETY: a live store; the values are copied in.
    unsafe { cameras.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID) }
        .map_err(error_of)?;
    Ok(cameras)
}

/// A string Media Foundation keeps under `key`, which it allocates for the caller.
fn string(attributes: &IMFAttributes, key: &GUID) -> Option<String> {
    let (mut text, mut length) = (PWSTR::null(), 0_u32);
    // SAFETY: both outputs are written by the call; the string is then this function's to free.
    unsafe { attributes.GetAllocatedString(key, &raw mut text, &raw mut length) }.ok()?;
    // SAFETY: a nul-terminated string Windows handed over, read once.
    let read = unsafe { text.to_string() }.ok();
    // SAFETY: allocated with the task allocator, freed once, never read after.
    unsafe { CoTaskMemFree(Some(text.0.cast_const().cast())) };
    read
}

#[expect(clippy::needless_pass_by_value, reason = "`map_err` hands each error over")]
fn error_of(error: windows::core::Error) -> CameraError {
    refusal(error.code())
}

/// The refusal a person is told for what Windows answered.
fn refusal(code: HRESULT) -> CameraError {
    // `HRESULT_FROM_WIN32` of `ERROR_SHARING_VIOLATION` and of `ERROR_DEVICE_NOT_CONNECTED`.
    const SHARING_VIOLATION: HRESULT = HRESULT(0x8007_0020_u32.cast_signed());
    const NOT_CONNECTED: HRESULT = HRESULT(0x8007_048F_u32.cast_signed());
    match code {
        E_ACCESSDENIED => CameraError::Denied,
        MF_E_HW_MFT_FAILED_START_STREAMING | MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED | SHARING_VIOLATION => {
            CameraError::Busy
        }
        // A source shut down is one whose camera was removed, or that was stopped.
        MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED | NOT_CONNECTED | MF_E_SHUTDOWN => CameraError::Gone,
        _ => CameraError::Unavailable,
    }
}

/// The name the camera's device goes by: a name a driver or the person gave it, else the product's own, as its USB
/// descriptor says it. Media Foundation names a camera on Windows' own driver "USB Video Device". None where the
/// Configuration Manager does not answer.
fn device_name(link: &str) -> Option<String> {
    let instance = interface_property(link, &DEVPKEY_Device_InstanceId)?;
    let mut node = 0_u32;
    // SAFETY: the output is written by the call; the id is a string of ours, alive across it.
    let found = unsafe { CM_Locate_DevNodeW(&raw mut node, &HSTRING::from(instance), CM_LOCATE_DEVNODE_NORMAL) };
    (found == CR_SUCCESS).then_some(())?;
    [&DEVPKEY_Device_FriendlyName, &DEVPKEY_Device_BusReportedDeviceDesc]
        .into_iter()
        .find_map(|key| node_property(node, key).filter(|name| !name.trim().is_empty()))
}

/// The longest property string read, in UTF-16 units: a device's names are short.
const PROPERTY_UNITS: usize = 1024;

fn interface_property(link: &str, key: &DEVPROPKEY) -> Option<String> {
    let link = HSTRING::from(link);
    property_string(|buffer, size, kind| {
        // SAFETY: the outputs are written by the call; the link is alive across it.
        unsafe { CM_Get_Device_Interface_PropertyW(&link, key, kind, buffer, size, 0) }
    })
}

fn node_property(node: u32, key: &DEVPROPKEY) -> Option<String> {
    property_string(|buffer, size, kind| {
        // SAFETY: the outputs are written by the call.
        unsafe { CM_Get_DevNode_PropertyW(node, key, kind, buffer, size, 0) }
    })
}

/// A string property, read once into room for [`PROPERTY_UNITS`]: none if it is no string or longer.
fn property_string(read: impl Fn(Option<*mut u8>, *mut u32, *mut DEVPROPTYPE) -> CONFIGRET) -> Option<String> {
    let mut units = vec![0_u16; PROPERTY_UNITS];
    let mut size = u32::try_from(units.len() * 2).ok()?;
    let mut kind = DEVPROPTYPE::default();
    (read(Some(units.as_mut_ptr().cast()), &raw mut size, &raw mut kind) == CR_SUCCESS).then_some(())?;
    (kind == DEVPROP_TYPE_STRING).then_some(())?;
    units.truncate((size as usize / 2).min(PROPERTY_UNITS));
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

/// A camera's source, shut down when dropped so the camera is let go at once, not when the last reference goes.
struct Source(IMFMediaSource);

impl Drop for Source {
    fn drop(&mut self) {
        // SAFETY: a live source, shut down once; nothing reads it after.
        let _ = unsafe { self.0.Shutdown() };
    }
}

/// This thread's COM apartment and Media Foundation, left when dropped, after every interface made in them.
struct Started;

impl Started {
    fn enter() -> Result<Self, CameraError> {
        // SAFETY: this thread's own apartment, left by `Drop`.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|_| CameraError::Unavailable)?;
        // SAFETY: Media Foundation's start, counted, and paired with `Drop`'s shutdown.
        if unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }.is_err() {
            // SAFETY: paired with the apartment entered above, in which nothing was made.
            unsafe { CoUninitialize() };
            return Err(CameraError::Unavailable);
        }
        Ok(Self)
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // SAFETY: paired with `enter`'s start; the interfaces made under it are dropped before it.
        let _ = unsafe { MFShutdown() };
        // SAFETY: paired with `enter`'s apartment.
        unsafe { CoUninitialize() };
    }
}

/// As the registration does for a removal of the camera `id`: for a test, which cannot unplug one. Told in capitals,
/// as Windows writes a link in more than one case.
#[cfg(test)]
pub(super) fn removed(id: &str) {
    plugged::removed(&id.to_uppercase());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_s_rows_are_the_bytes_from_the_first_row_to_the_last_one_s_width() {
        let (width, height, pitch) = (4, 2, 6);
        let bytes: Vec<u8> = (0..pitch * 3).map(|at| u8::try_from(at).expect("small")).collect();
        assert_eq!(rows(&bytes, pitch, width, height), Some(&bytes[..]));
        assert_eq!(rows(&bytes[..16], pitch, width, height), Some(&bytes[..16]), "the last row needs only its width");
        assert_eq!(rows(&[0; 24], pitch, width, height), Some(&[0; 18][..]), "nothing past the last row");
    }

    #[test]
    fn a_buffer_short_of_its_rows_or_a_pitch_under_the_width_is_no_picture() {
        assert!(rows(&[0; 15], 6, 4, 2).is_none(), "the last row's four bytes are not all there");
        assert!(rows(&[0; 16], 6, 4, 2).is_some(), "the last row needs only its width");
        assert!(rows(&[0; 12], 2, 4, 2).is_none());
    }

    #[test]
    fn what_windows_answers_is_one_refusal_a_person_reads() {
        assert_eq!(refusal(E_ACCESSDENIED), CameraError::Denied, "the privacy switch");
        assert_eq!(refusal(MF_E_HW_MFT_FAILED_START_STREAMING), CameraError::Busy, "another application has it");
        assert_eq!(refusal(MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED), CameraError::Gone, "unplugged");
        assert_eq!(refusal(HRESULT(0x8000_4005_u32.cast_signed())), CameraError::Unavailable);
    }

    #[test]
    fn a_size_and_a_rate_are_two_numbers_high_first() {
        assert_eq!(halves((1280 << 32) | 720), (1280, 720));
        assert_eq!(halves((30_000 << 32) | 1001), (30_000, 1001));
    }

    mod lab {
        use super::*;
        use tier::skipped;

        /// The machine's first camera, and this test's turn at it.
        fn first() -> Option<(Device, tier::Turn)> {
            let turn = tier::turn("camera")?;
            Some((list().ok()?.into_iter().next()?, turn))
        }

        #[test]
        fn each_camera_is_named_by_its_device() {
            let Some((device, _turn)) = first() else { return skipped("no camera plugged in") };
            let named = device_name(&device.id);
            assert!(named.is_some(), "the Configuration Manager names it");
            assert_eq!(named.as_deref(), Some(device.label.as_str()), "the list takes the device's name");
        }
    }
}
