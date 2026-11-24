//! The event loop's own window. winit wakes its loop on Windows through a visible, untitled top-level window, and a
//! close the system sends every top-level window (an installer's Restart Manager) destroys it in the default procedure,
//! after which the loop hears nothing and the process stays. Until winit's procedure refuses that close itself, the
//! close is taken here and told.

use crate::Unavailable;

/// Keeps the calling thread's event-loop window open when the system closes it, calling `closed` instead, once for
/// each close. Called on the loop's thread after the loop is made and before it runs. A system whose loop has no such
/// window has nothing to keep; a thread without one is [`Unavailable`].
pub fn keep(closed: impl Fn() + 'static) -> Result<(), Unavailable> {
    #[cfg(windows)]
    {
        windows::keep(std::rc::Rc::new(closed))
    }
    #[cfg(not(windows))]
    {
        let _ = closed;
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::Unavailable;
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows_sys::Win32::Foundation::{FALSE, HWND, LPARAM, LRESULT, TRUE, WPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, EnumThreadWindows, GWLP_WNDPROC, GetClassNameW, GetWindowLongPtrW, SetWindowLongPtrW,
        WM_CLOSE, WNDPROC,
    };
    use windows_sys::core::BOOL;

    /// The class winit registers that window under.
    pub(super) const CLASS: &str = "Winit Thread Event Target";

    /// What the kept window tells on a close, and winit's procedure, which answers every other message.
    struct Kept {
        closed: Rc<dyn Fn()>,
        winit: WNDPROC,
    }

    thread_local! {
        static KEPT: RefCell<Option<Kept>> = const { RefCell::new(None) };
    }

    pub(super) fn keep(closed: Rc<dyn Fn()>) -> Result<(), Unavailable> {
        let window = find().ok_or(Unavailable)?;
        // SAFETY: a window of this thread, read on it.
        let winit = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
        // SAFETY: what `GWLP_WNDPROC` holds is the window's procedure, null for none.
        let winit = unsafe { std::mem::transmute::<isize, WNDPROC>(winit) };
        if winit.is_none() {
            return Err(Unavailable);
        }
        // Kept before the procedure is replaced: nothing is dispatched on this thread between the two.
        KEPT.with(|kept| *kept.borrow_mut() = Some(Kept { closed, winit }));
        let ours: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = procedure;
        // SAFETY: a window of this thread, given a procedure that hands every message but a close to winit's.
        if unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, (ours as usize).cast_signed()) } == 0 {
            KEPT.with(|kept| kept.borrow_mut().take());
            return Err(Unavailable);
        }
        Ok(())
    }

    /// The window of winit's class among this thread's top-level windows.
    fn find() -> Option<HWND> {
        unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` is the `Option` below, alive for the whole enumeration.
            let found = unsafe { &mut *(found as *mut Option<HWND>) };
            let mut name = [0u16; 64];
            // SAFETY: a window the enumeration handed over, and a buffer of the length given.
            let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), 64) };
            let name = name.get(..usize::try_from(length).unwrap_or(0)).unwrap_or_default();
            if CLASS.encode_utf16().eq(name.iter().copied()) {
                *found = Some(window);
                return FALSE;
            }
            TRUE
        }
        let mut found: Option<HWND> = None;
        // SAFETY: no precondition.
        let thread = unsafe { GetCurrentThreadId() };
        // SAFETY: `visit` reads `found` only during this call.
        unsafe { EnumThreadWindows(thread, Some(visit), &raw mut found as LPARAM) };
        found
    }

    /// Tells a close and answers it, so the window stays; hands everything else to winit as it came. Neither is called
    /// while the state is borrowed, since either may send a message back here.
    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let (closed, winit) = KEPT.with(|kept| {
            kept.borrow().as_ref().map_or((None, None), |kept| (Some(Rc::clone(&kept.closed)), kept.winit))
        });
        if message == WM_CLOSE
            && let Some(closed) = closed
        {
            closed();
            return 0;
        }
        // SAFETY: winit's procedure for its own window, handed the message as it came.
        unsafe { CallWindowProcW(winit, window, message, wparam, lparam) }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindow, RegisterClassExW, SendMessageW, WNDCLASSEXW,
            WS_OVERLAPPED,
        };

        /// A top-level window of winit's class on this thread, answered by the default procedure as winit's is.
        fn a_loop_window() -> HWND {
            let class: Vec<u16> = CLASS.encode_utf16().chain([0]).collect();
            // SAFETY: a null name is this process's own module.
            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let registered = WNDCLASSEXW {
                cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                // SAFETY: the rest of the class is unused and zero is its absence.
                ..unsafe { std::mem::zeroed() }
            };
            // SAFETY: a class whose name and procedure outlive the call; registered again on a thread of the same
            // process it fails, and the first registration serves.
            unsafe { RegisterClassExW(&raw const registered) };
            // SAFETY: the class registered above; a top-level window, never shown.
            unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                )
            }
        }

        #[test]
        fn a_close_the_system_sends_the_loops_window_is_told_and_the_window_stays() {
            std::thread::spawn(|| {
                let window = a_loop_window();
                assert!(!window.is_null(), "the window was made");
                let told = Rc::new(Cell::new(0));
                let counted = Rc::clone(&told);
                keep(Rc::new(move || counted.set(counted.get() + 1))).expect("this thread's loop window is kept");
                // SAFETY: a window of this thread.
                unsafe { SendMessageW(window, WM_CLOSE, 0, 0) };
                // SAFETY: as above.
                let stays = unsafe { IsWindow(window) } != 0;
                // SAFETY: as above; the procedure hands its destruction to the default one.
                unsafe { DestroyWindow(window) };
                assert_eq!(told.get(), 1, "the close was told once");
                assert!(stays, "the window stayed");
            })
            .join()
            .expect("the window's thread ended cleanly");
        }

        #[test]
        fn a_thread_without_a_loop_window_has_none_to_keep() {
            std::thread::spawn(|| assert_eq!(keep(Rc::new(|| {})).err(), Some(Unavailable)))
                .join()
                .expect("the thread ended cleanly");
        }
    }
}
