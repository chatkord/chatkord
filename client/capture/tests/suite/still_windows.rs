//! A still of the primary monitor, drawn smaller on the GPU and read back, with no person.

use capture::{Gpu, Validation, still};
use tier::skipped;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

fn primary_monitor() -> GraphicsCaptureItem {
    // SAFETY: plain values in; the primary monitor always exists while a desktop does.
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
    // SAFETY: a live monitor handle.
    unsafe { interop.CreateForMonitor(monitor) }.unwrap()
}

mod lab {
    use super::*;

    #[test]
    fn a_still_of_the_primary_monitor_fits_a_card_and_shows_something() {
        let item = primary_monitor();
        let size = item.Size().unwrap();
        let took = std::time::Instant::now();
        let Some(gpu) = Gpu::open(Validation::of_this_run()) else {
            return skipped("this machine has no D3D11 device");
        };
        let still = still(&item, &gpu, (320, 180)).expect("a still");
        let elapsed = took.elapsed();
        assert!(still.width <= 320 && still.height <= 180 && (still.width == 320 || still.height == 180), "{still:?}");
        assert_eq!(still.rgba.len(), (still.width * still.height * 4) as usize);
        // A desktop is never one colour from corner to corner: the picture was read, not left blank.
        let first = &still.rgba[..4];
        assert!(still.rgba.chunks(4).any(|pixel| pixel != first), "the still is blank");
        eprintln!("a still of {}x{}: {}x{} in {elapsed:?}", size.Width, size.Height, still.width, still.height);
        assert_eq!(capture::validation::unknown(gpu.reported()), Vec::<String>::new(), "the D3D11 debug layer");
    }
}
