//! A window of the test's own, captured and then closed as a person closes one (`WM_CLOSE`, then destroyed): its
//! capture ends, though Windows was seen not to say that such a window's capture item closed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::capture_rate::drawing;
use capture::{Capture, CaptureError, Gpu, Validation};
use tier::skipped;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// How long a capture whose window is gone may still run: a wait or two of the share's.
const ENDS_WITHIN: Duration = Duration::from_secs(2);
/// The wait for each picture, as a share's is.
const WAIT: Duration = Duration::from_millis(100);

mod lab {
    use super::*;

    #[test]
    fn a_captured_window_that_is_closed_ends_its_capture() {
        tier::lab();
        let Some(gpu) = Gpu::open(Validation::Off) else { return skipped("this machine has no D3D11 device") };
        let (stop, shown) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let (thread, window) = drawing(Arc::clone(&stop), Arc::clone(&shown));
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
        // SAFETY: a live window of this process, drawing on its own thread until `stop` is set.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(window as *mut _)) }.unwrap();
        let capture = Capture::open(&item, &gpu, false, None).expect("a capture of the window").of_window(window);
        let drawing_for = Instant::now();
        while drawing_for.elapsed() < Duration::from_millis(500) {
            assert_eq!(capture.next(WAIT).err(), None, "a window that draws is captured on");
        }
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        let gone = Instant::now();
        let ended = loop {
            match capture.next(WAIT) {
                Err(ended) => break ended,
                Ok(_) => assert!(gone.elapsed() < ENDS_WITHIN, "the capture of a closed window ran {ENDS_WITHIN:?}"),
            }
        };
        assert_eq!(ended, CaptureError::Ended, "after {:?}", gone.elapsed());
        assert!(shown.load(Ordering::Relaxed) > 0, "the window drew");
    }
}
