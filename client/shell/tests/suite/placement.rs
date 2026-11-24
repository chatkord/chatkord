//! The main window opens next run where a person left it: moved, then minimised and closed, it comes back at the place
//! it had before it was minimised, not where Windows keeps a minimised window, off every monitor.

mod lab {
    use crate::common::{Home, Which, exited, shown, start};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, PostMessageW, SW_MINIMIZE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow,
        WM_CLOSE,
    };

    /// Where the window's outer corner stands, on the desktop.
    fn corner(window: HWND) -> (i32, i32) {
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: a live window and a valid out pointer.
        assert_ne!(unsafe { GetWindowRect(window, &raw mut rect) }, 0, "the window's rectangle");
        (rect.left, rect.top)
    }

    /// Asks the window to close, as its close button does, and waits for its process to end cleanly.
    fn close(window: HWND, child: &mut crate::common::Running) {
        // SAFETY: a live window; the message carries nothing.
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        let status = exited(child).expect("the window ended once asked to close");
        assert!(status.success(), "the window exited with {status}");
    }

    #[test]
    fn a_window_moved_minimised_and_closed_opens_next_run_where_it_stood() {
        tier::lab();
        let home = Home::new("placement");
        home.closing_quits();
        let mut first = start(&home);
        let window = shown(&mut first, Which::Main).expect("the window showed");
        // SAFETY: a live window; no size or order is changed.
        unsafe { SetWindowPos(window, std::ptr::null_mut(), 180, 140, 0, 0, SWP_NOSIZE | SWP_NOZORDER) };
        std::thread::sleep(Duration::from_millis(500));
        let moved = corner(window);
        // SAFETY: a live window.
        unsafe { ShowWindow(window, SW_MINIMIZE) };
        let asked = Instant::now();
        // SAFETY: a live window.
        while unsafe { IsIconic(window) } == 0 {
            assert!(asked.elapsed() < Duration::from_secs(5), "the window minimised");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300));
        close(window, &mut first);

        let mut second = start(&home);
        let again = shown(&mut second, Which::Main).expect("the window showed again");
        std::thread::sleep(Duration::from_millis(500));
        let opened = corner(again);
        close(again, &mut second);
        assert_eq!(opened, moved, "it opens where it stood before it was minimised");
    }
}
