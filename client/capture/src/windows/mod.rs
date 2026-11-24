//! Windows' capture: Windows Graphics Capture draws each picture as a BGRA texture on the encoder's D3D11 device
//! ([`Gpu`]), lent until dropped; its frame pool calls back on a thread of its own, which fills the shared slot.

use std::fmt;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::GraphicsCaptureSession;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::core::{IInspectable, Interface};

mod gpu;
mod preview;
mod processor;
mod retire;
mod still;

pub use gpu::{AdapterId, Gpu, Locked};
pub use preview::{Preview, PreviewPicture};
pub use processor::{Output, Processor};
pub use still::{Still, still};

use crate::{CaptureError, Frame, Layout, Native, Pixels, Shared};
use retire::Retire;

/// Two pictures in the pool: one the encoder reads while the next is drawn.
const BUFFERS: i32 = 2;

/// A captured picture's texture, and what keeps it from being drawn over: dropping the last of its lends gives it
/// back to the capture's frame pool.
pub struct D3d11 {
    texture: ID3D11Texture2D,
    lend: Arc<Mutex<dyn Send>>,
}

// SAFETY: the texture belongs to a multithread-protected device ([`Gpu`]), so its methods may be called from any
// thread; the lend is `Send` and shared behind a mutex.
unsafe impl Send for D3d11 {}

impl D3d11 {
    /// A picture somebody else drew into `texture`, a BGRA texture of a [`Gpu`]'s device, held as long as this is.
    pub fn new(texture: ID3D11Texture2D) -> Self {
        Self { texture, lend: Arc::new(Mutex::new(())) }
    }

    /// The texture, BGRA, on the device it was drawn on.
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    /// The same texture lent a second time; the capture has it back when both have dropped.
    pub fn again(&self) -> Self {
        Self { texture: self.texture.clone(), lend: Arc::clone(&self.lend) }
    }
}

impl fmt::Debug for D3d11 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D3d11").finish_non_exhaustive()
    }
}

/// The WinRT device the pool is remade on, moved into its callback.
struct Remade(IDirect3DDevice);

// SAFETY: the device Windows makes from a DXGI device is free-threaded (agile), and the DXGI device under it is
// multithread-protected.
unsafe impl Send for Remade {}

impl Remade {
    /// Through a method, so a closure captures the whole wrapper, not its field.
    fn device(&self) -> &IDirect3DDevice {
        &self.0
    }
}

/// A picture of the pool, closed when its last lend drops and the GPU has done its reads, so the pool can draw into
/// it again. The close is the retire thread's: dropping this never waits on the GPU.
struct Lent(Option<Direct3D11CaptureFrame>, Arc<Retire>);

impl Drop for Lent {
    fn drop(&mut self) {
        if let Some(frame) = self.0.take() {
            self.1.retire(frame);
        }
    }
}

/// A running capture of one monitor or window.
pub struct Capture {
    shared: Arc<Shared>,
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    item: GraphicsCaptureItem,
    arrived: i64,
    closed: i64,
    /// Shut when this drops, once the pool is closed: the pictures still lent close on its thread first.
    retire: Arc<Retire>,
    /// The window captured, by the `HWND` it was picked by; none for a monitor.
    window: Option<isize>,
}

/// How often a share takes a picture: at most `rate` a second, from a display that refreshes `refresh` times a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paced {
    /// The share's pictures a second.
    pub rate: NonZeroU32,
    /// The display's refreshes a second, as Windows says them: a rate such as 59.94 as the whole number below it.
    pub refresh: NonZeroU32,
}

impl Paced {
    /// The least time between two pictures, in Windows' 100 ns ticks: half a refresh short of the most whole refreshes
    /// one picture's time holds, so a refresh that comes a little early still counts and none between them does.
    /// Exactly `1 / rate` refused every refresh that came early: 52 of a 60 Hz monitor's 60.
    fn interval(self) -> i64 {
        let (rate, refresh) = (i64::from(self.rate.get()), i64::from(self.refresh.get()));
        // The refresh Windows says may be the whole number below the display's.
        let whole = ((refresh + 1) / rate).max(1);
        (2 * whole - 1) * 10_000_000 / (2 * refresh)
    }
}

impl Capture {
    /// Starts capturing `item` into `gpu`, with the pointer drawn in when `cursor` is set, as often as `paced` says;
    /// none takes every picture Windows shows.
    pub fn open(
        item: &GraphicsCaptureItem,
        gpu: &Gpu,
        cursor: bool,
        paced: Option<Paced>,
    ) -> Result<Self, CaptureError> {
        let device = winrt_device(gpu).ok_or(CaptureError::Connect)?;
        let retire = Arc::new(Retire::new(gpu).ok_or(CaptureError::Connect)?);
        let arriving = Arc::clone(&retire);
        let size = item.Size().map_err(|_| CaptureError::Connect)?;
        let format = DirectXPixelFormat::B8G8R8A8UIntNormalized;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, format, BUFFERS, size)
            .map_err(|_| CaptureError::Connect)?;
        let shared = Shared::new();
        let drawn = Mutex::new(size);
        let into = Arc::clone(&shared);
        let remade = Remade(device.clone());
        let arrived = pool
            .FrameArrived(&TypedEventHandler::new(
                move |pool: windows::core::Ref<'_, Direct3D11CaptureFramePool>, _| {
                    if let Some(pool) = pool.as_ref() {
                        arrive(pool, remade.device(), &drawn, &into, &arriving);
                    }
                    Ok(())
                },
            ))
            .map_err(|_| CaptureError::Connect)?;
        let ending = Arc::clone(&shared);
        let closed = item
            .Closed(&TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                ending.end(CaptureError::Ended);
                Ok(())
            }))
            .map_err(|_| CaptureError::Connect)?;
        let session = pool.CreateCaptureSession(item).map_err(|_| CaptureError::Connect)?;
        session.SetIsCursorCaptureEnabled(cursor).map_err(|_| CaptureError::Connect)?;
        // Windows draws a yellow border round what is captured unless it lets us leave it out (11 and later);
        // where it does not, the border stays.
        let _ = session.SetIsBorderRequired(false);
        // Windows' own interval held a 60 Hz monitor to 49 pictures a second. A Windows that cannot set it keeps its
        // own.
        let interval = paced.map_or(0, Paced::interval);
        let _ = session.SetMinUpdateInterval(windows::Foundation::TimeSpan { Duration: interval });
        session.StartCapture().map_err(|_| CaptureError::Stream)?;
        Ok(Self { shared, session, pool, item: item.clone(), arrived, closed, retire, window: None })
    }

    /// The same capture, of the window `window` names (the `HWND` it was picked by): once a wait for a picture ends
    /// with none, it asks whether that window still exists and ends when it does not.
    #[must_use]
    pub fn of_window(mut self, window: isize) -> Self {
        self.window = Some(window);
        self
    }

    /// The newest picture, waiting up to `wait` for one; `Ok(None)` is a quiet screen, not a failure. A window that
    /// is gone is `CaptureError::Ended`.
    pub fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        let taken = self.shared.take(wait)?;
        // A window closed normally was seen not to raise the item's `Closed`, leaving its share at no pictures.
        if taken.is_none() && self.window.is_some_and(|window| !exists(window)) {
            return Err(CaptureError::Ended);
        }
        Ok(taken)
    }

    /// What Windows calls the monitor or window being captured.
    pub fn source_name(&self) -> Option<String> {
        self.item.DisplayName().ok().map(|name| name.to_string()).filter(|name| !name.is_empty())
    }

    /// Pictures replaced before anyone took them.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped()
    }

    /// Frames the system made, those nobody took included.
    pub fn delivered(&self) -> u64 {
        self.shared.delivered()
    }

    /// The widest gap between two frames' own times since it was last asked, which starts it over.
    pub fn take_widest_gap(&self) -> Duration {
        self.shared.take_widest_gap()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Each step is tried whatever the one before did: a capture that cannot stop cleanly still lets go.
        let _ = self.pool.RemoveFrameArrived(self.arrived);
        let _ = self.item.RemoveClosed(self.closed);
        let _ = self.session.Close();
        let _ = self.pool.Close();
        self.retire.shut();
    }
}

/// Whether the window `window` names still exists.
fn exists(window: isize) -> bool {
    let window = windows::Win32::Foundation::HWND(window as *mut std::ffi::c_void);
    // SAFETY: `IsWindow` takes any handle, one of a window gone included, and only reads whether it names a window.
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)) }.as_bool()
}

/// Takes the pool's newest picture into the slot. A picture of another size than the pool's is a window that was
/// resized: the pool is remade at the new size and the picture skipped, as only its corner holds the content.
fn arrive(
    pool: &Direct3D11CaptureFramePool,
    device: &IDirect3DDevice,
    drawn: &Mutex<SizeInt32>,
    into: &Shared,
    retire: &Arc<Retire>,
) {
    let Ok(frame) = pool.TryGetNextFrame() else { return };
    let Ok(content) = frame.ContentSize() else { return };
    let Ok(mut size) = drawn.lock() else { return };
    if content != *size {
        let format = DirectXPixelFormat::B8G8R8A8UIntNormalized;
        if content.Width > 0 && content.Height > 0 && pool.Recreate(device, format, BUFFERS, content).is_ok() {
            *size = content;
        }
        return;
    }
    drop(size);
    let texture = frame
        .Surface()
        .and_then(|surface| surface.cast::<IDirect3DDxgiInterfaceAccess>())
        // SAFETY: a capture frame's surface is a D3D11 texture on the pool's device.
        .and_then(|access| unsafe { access.GetInterface::<ID3D11Texture2D>() });
    let Ok(texture) = texture else {
        into.end(CaptureError::Stream);
        return;
    };
    let (Ok(width), Ok(height)) = (u32::try_from(content.Width), u32::try_from(content.Height)) else { return };
    let layout = Layout { pixels: Pixels::Bgra, width, height };
    let captured = captured(&frame);
    let lend: Arc<Mutex<dyn Send>> = Arc::new(Mutex::new(Lent(Some(frame), Arc::clone(retire))));
    into.put(Frame::new(layout, Native::D3d11(D3d11 { texture, lend }), captured), 0);
}

/// Windows' time for `frame`, on this process's clock: its `SystemRelativeTime` is the performance counter's, in 100
/// ns, and is the composition the picture belongs to, which on the RX 9060 XT at 60 Hz was 10 to 15 ms after it arrived.
/// Where Windows gives no time, the picture is stamped as it came.
fn captured(frame: &Direct3D11CaptureFrame) -> Instant {
    let drawn = frame.SystemRelativeTime().ok().map(|time| time.Duration);
    drawn.and_then(clock::instant_of).unwrap_or_else(Instant::now)
}

/// The WinRT face of the Gpu's D3D11 device, which Windows Graphics Capture draws through.
fn winrt_device(gpu: &Gpu) -> Option<IDirect3DDevice> {
    let dxgi: IDXGIDevice = gpu.device().cast().ok()?;
    // SAFETY: a live DXGI device in, a WinRT device that holds its own reference out.
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.ok()?;
    inspectable.cast().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every(refresh: u32, rate: u32) -> f64 {
        let paced = Paced { rate: NonZeroU32::new(rate).unwrap(), refresh: NonZeroU32::new(refresh).unwrap() };
        paced.interval() as f64 / 10_000_000.0
    }

    /// The pictures a second a display refreshing `real` times a second gives a capture whose least interval is
    /// `interval` seconds: each refresh at or past the interval since the last one taken.
    fn given(real: f64, interval: f64) -> f64 {
        real / (interval * real).ceil().max(1.0)
    }

    #[test]
    fn a_share_is_given_its_rate_or_every_refresh_and_never_fewer() {
        for (real, said, rate) in [
            (60.0, 60, 60),
            (60.0, 60, 30),
            (59.94, 59, 60),
            (59.94, 59, 30),
            (100.0, 100, 60),
            (120.0, 120, 60),
            (143.86, 143, 60),
            (144.0, 144, 60),
            (165.0, 165, 60),
            (240.0, 240, 30),
            (75.0, 75, 60),
            (30.0, 30, 60),
        ] {
            let interval = every(said, rate);
            let pictures = given(real, interval);
            let wanted = f64::from(rate).min(real);
            // A refresh a little early or late, by a quarter of one, lands on the same side of the interval.
            let period = 1.0 / real;
            for jitter in [-period / 4.0, period / 4.0] {
                assert_eq!(given(real, interval + jitter), pictures, "{real} Hz asked {rate}: on the edge");
            }
            assert!(pictures >= wanted * 0.998, "{real} Hz asked {rate}: given {pictures:.2} a second");
            assert!(pictures < 2.0 * wanted, "{real} Hz asked {rate}: given {pictures:.2}, twice the rate or more");
        }
    }
}
