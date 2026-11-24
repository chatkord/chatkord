//! A window of the test's own that draws a new picture at every refresh, captured by Windows Graphics Capture: every
//! picture the compositor shows reaches the capture, as the share's own pace, not Windows, decides the rate.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use capture::{Capture, Gpu, Paced, Validation};
use tier::skipped;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW, RegisterClassW,
    SendMessageW, WM_CLOSE, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::w;

/// How long the window draws while it is captured.
const DRAWING: Duration = Duration::from_secs(3);
/// The share of what was shown that must reach the capture: Windows' default interval let through 158 to 168 of
/// this window's 181 pictures, and 49 a second of a monitor's 60.
const AT_LEAST: f64 = 0.97;

extern "system" fn procedure(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: the arguments are the system's own, passed on unchanged.
    unsafe { DefWindowProcW(window, message, w, l) }
}

/// Opens a 640x360 window on its own thread that presents a new colour at every refresh until `stop` is set, when it
/// is closed as a person closes one; its handle comes back once its first picture is shown, and `shown` counts the pictures presented.
pub(crate) fn drawing(stop: Arc<AtomicBool>, shown: Arc<AtomicU32>) -> (std::thread::JoinHandle<()>, isize) {
    let (tell, told) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        // SAFETY: a null name asks for this executable's own module.
        let instance = unsafe { GetModuleHandleW(None) }.unwrap();
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("chatkord-capture-rate"),
            ..WNDCLASSW::default()
        };
        // SAFETY: `class` names a procedure that lives as long as the program.
        unsafe { RegisterClassW(&raw const class) };
        // SAFETY: the class was registered above; the window is destroyed on this thread before it ends.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("chatkord-capture-rate"),
                w!("capture rate"),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                640,
                360,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .unwrap();
        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: 640,
                Height: 360,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                ..Default::default()
            },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: window,
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            ..Default::default()
        };
        let (mut chain, mut device, mut context) = (None, None, None);
        // SAFETY: every out pointer is a local written on success, and `desc` names the window made above.
        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&raw const desc),
                Some(&raw mut chain),
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }
        .unwrap();
        let (chain, device, context) = (chain.unwrap(), device.unwrap(), context.unwrap());
        let mut told = Some(tell);
        let mut message = MSG::default();
        let mut n = 0u32;
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: `message` is a local the call fills; the window is this thread's.
            while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                // SAFETY: a message just taken from this thread's queue.
                unsafe { DispatchMessageW(&raw const message) };
            }
            // SAFETY: buffer 0 of a flip chain is the one drawn next, a texture of the chain's device.
            let buffer: ID3D11Texture2D = unsafe { chain.GetBuffer(0) }.unwrap();
            let mut view = None;
            // SAFETY: a render target view of a render-target buffer of this device, written to a local.
            unsafe { device.CreateRenderTargetView(&buffer, None, Some(&raw mut view)) }.unwrap();
            let shade = if n.is_multiple_of(2) { 0.0 } else { 1.0 };
            n += 1;
            // SAFETY: the view was just made on this device; the context is only used on this thread.
            unsafe { context.ClearRenderTargetView(&view.unwrap(), &[shade, 0.5, 1.0 - shade, 1.0]) };
            // SAFETY: a chain of this thread's window; waiting for one refresh paces the loop to the screen.
            unsafe { chain.Present(1, DXGI_PRESENT(0)) }.ok().unwrap();
            shown.fetch_add(1, Ordering::Relaxed);
            if let Some(tell) = told.take() {
                tell.send(window.0 as isize).unwrap();
            }
        }
        // Closed as a person closes a window: `WM_CLOSE`, which the default procedure answers by destroying it.
        // SAFETY: the window was made on this thread and is sent its close once.
        unsafe { SendMessageW(window, WM_CLOSE, None, None) };
        // SAFETY: `IsWindow` takes any handle, one of a window gone included.
        assert!(!unsafe { IsWindow(Some(window)) }.as_bool(), "the close destroyed the window");
    });
    (thread, told.recv().unwrap())
}

/// How many times a second the display this thread draws on refreshes, as a share's pick reads it.
fn refresh() -> Option<NonZeroU32> {
    #[expect(clippy::cast_possible_truncation, reason = "DEVMODEW's size, far under u16::MAX")]
    let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: no device name is this thread's display; the mode's size is said in it.
    let read = unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &raw mut mode) };
    read.as_bool().then_some(mode.dmDisplayFrequency).and_then(NonZeroU32::new).filter(|hertz| hertz.get() > 1)
}

/// The window drawn at every refresh for [`DRAWING`] and captured asking for at most `rate` a second: the pictures it
/// presented and those the capture delivered.
#[expect(clippy::print_stderr, reason = "a measuring test's output is what it prints")]
fn captured(rate: NonZeroU32) -> Option<(u32, u32)> {
    let Some(gpu) = Gpu::open(Validation::Off) else {
        skipped("this machine has no D3D11 device");
        return None;
    };
    let (stop, shown) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
    let (thread, window) = drawing(Arc::clone(&stop), Arc::clone(&shown));
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
    // SAFETY: a live window of this process, drawing on its own thread until `stop` is set.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(window as *mut _)) }.unwrap();
    let Some(refresh) = refresh() else {
        skipped("Windows does not say how often this display refreshes");
        return None;
    };
    let capture = Capture::open(&item, &gpu, false, Some(Paced { rate, refresh })).expect("a capture of the window");
    // The first pictures come while the pool settles on the window's size.
    std::thread::sleep(Duration::from_millis(500));
    while capture.next(Duration::ZERO).unwrap().is_some() {}
    let (shown_before, dropped_before) = (shown.load(Ordering::Relaxed), capture.dropped());
    let started = Instant::now();
    let mut taken = 0u32;
    while started.elapsed() < DRAWING {
        taken += u32::from(capture.next(Duration::from_millis(100)).unwrap().is_some());
    }
    let (presented, arrived) = (
        shown.load(Ordering::Relaxed) - shown_before,
        taken + u32::try_from(capture.dropped() - dropped_before).unwrap(),
    );
    stop.store(true, Ordering::Relaxed);
    thread.join().unwrap();
    let per_second = |count: u32| f64::from(count) / DRAWING.as_secs_f64();
    eprintln!(
        "asked for {rate} a second, in {DRAWING:?} the window presented {presented} pictures ({:.1} a second), the \
         capture delivered {arrived} ({:.1} a second)",
        per_second(presented),
        per_second(arrived)
    );
    assert!(presented > 0, "the window drew");
    Some((presented, arrived))
}

mod lab {
    use super::*;

    #[test]
    fn a_window_that_draws_at_every_refresh_is_captured_at_every_refresh_the_share_asks_for() {
        tier::lab();
        let Some((presented, arrived)) = captured(NonZeroU32::new(60).unwrap()) else { return skipped("see above") };
        assert!(
            f64::from(arrived) >= AT_LEAST * f64::from(presented),
            "the capture let through {arrived} of {presented} pictures shown"
        );
    }

    #[test]
    fn a_share_at_half_the_refresh_rate_is_given_half_the_pictures_and_no_fewer() {
        tier::lab();
        let Some((presented, arrived)) = captured(NonZeroU32::new(30).unwrap()) else { return skipped("see above") };
        let half = f64::from(presented) / 2.0;
        assert!(f64::from(arrived) >= AT_LEAST * half, "the capture let through {arrived} of the {half} asked for");
        assert!(f64::from(arrived) <= half * 1.1, "the capture delivered {arrived}, more than the {half} asked for");
    }
}
