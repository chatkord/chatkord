//! The desktop session's end, told apart from a person's close: the system closes every window as a session ends, and
//! a close it sends is the same request as a person's. Linux hears it from the Inhibit portal's session monitor;
//! Windows sends `WM_QUERYENDSESSION` and `WM_ENDSESSION` to every top-level window, a hidden one of ours on its own
//! thread here, which an installer's Restart Manager sends too before it closes a program.

use crate::Unavailable;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How far the session's end has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// It is asked for (a sign-out or a shutdown), and a close the system sends now is its own, not a person's.
    Asked,
    /// It happens now: the program is to quit at once, its servers parted, before the system ends it.
    Happening,
}

/// Whether the program has parted its connections: told by the one that parts them, waited on by the thread that holds
/// the session's end until they are. Cloned freely; every clone is the same signal.
#[derive(Clone, Debug, Default)]
pub struct Parted(Arc<(Mutex<bool>, Condvar)>);

impl Parted {
    /// Says the connections are parted, and lets whoever waits go.
    pub fn done(&self) {
        let (parted, wake) = &*self.0;
        *parted.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    /// Waits until [`Parted::done`] is said or `bound` has passed, whichever comes first; whether it was said. Blocks
    /// the thread: never called from an async task.
    pub fn wait(&self, bound: Duration) -> bool {
        let (parted, wake) = &*self.0;
        let held = parted.lock().unwrap_or_else(PoisonError::into_inner);
        let (held, _) = wake.wait_timeout_while(held, bound, |parted| !*parted).unwrap_or_else(PoisonError::into_inner);
        *held
    }
}

/// Starts watching the session's end, and returns once the watch is in place, so a close the system sends from then
/// on is told apart: the watching itself is the future returned, which runs until the session ends, calling `ending`
/// as the end is asked for and again as it happens. A portal that will not watch, or a system with none here yet, is
/// [`Unavailable`]. Where the system ends the process as soon as it is told the session ended (Windows), the end is
/// held until `parted` says the program has parted, and at most `bound`.
pub async fn watch_end(
    ending: impl Fn(End) + Send + 'static,
    parted: Parted,
    bound: Duration,
) -> Result<impl Future<Output = ()> + Send + 'static, Unavailable> {
    #[cfg(target_os = "linux")]
    {
        use ashpd::desktop::inhibit::{CreateMonitorOptions, InhibitProxy, SessionState};
        use futures_util::StreamExt;

        // The session waits for the program's own end, as a terminate signal asks it to part.
        let _ = (parted, bound);
        let proxy = InhibitProxy::new().await.map_err(|_| Unavailable)?;
        let mut states = proxy.receive_state_changed().await.map_err(|_| Unavailable)?;
        let monitor = proxy.create_monitor(None, CreateMonitorOptions::default()).await.map_err(|_| Unavailable)?;
        Ok(async move {
            while let Some(state) = states.next().await {
                match state.session_state() {
                    SessionState::QueryEnd => {
                        ending(End::Asked);
                        // The session waits for every monitor's answer; nothing here holds it back.
                        let _ = proxy.query_end_response(&monitor).await;
                    }
                    SessionState::Ending => ending(End::Asked),
                    SessionState::Running => {}
                }
            }
        })
    }
    #[cfg(windows)]
    {
        let (opened, open) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("session-end".into())
            .spawn(move || windows::serve(Box::new(ending), parted, bound, &opened))
            .map_err(|_| Unavailable)?;
        open.recv().map_err(|_| Unavailable)??;
        // The window's thread watches from here on, for the life of the process.
        Ok(std::future::pending::<()>())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (ending, parted, bound);
        Err::<std::future::Pending<()>, _>(Unavailable)
    }
}

/// A hidden top-level window (a message-only one is sent no session messages) whose procedure tells the session's end.
#[cfg(windows)]
mod windows {
    use super::{End, Parted, Unavailable};
    use std::cell::RefCell;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{FALSE, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassExW, TranslateMessage,
        WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSEXW, WS_OVERLAPPED,
    };

    /// What this thread's window tells, and what it holds the end for, at most `bound`.
    struct Told {
        ending: Box<dyn Fn(End)>,
        parted: Parted,
        bound: Duration,
    }

    thread_local! {
        static TOLD: RefCell<Option<Told>> = const { RefCell::new(None) };
    }

    /// Makes the window, says whether it was made, then answers its messages for the life of the process.
    pub(super) fn serve(
        ending: Box<dyn Fn(End) + Send>,
        parted: Parted,
        bound: Duration,
        opened: &std::sync::mpsc::SyncSender<Result<(), Unavailable>>,
    ) {
        TOLD.with(|told| *told.borrow_mut() = Some(Told { ending, parted, bound }));
        let class: Vec<u16> = "ChatkordSessionEnd".encode_utf16().chain([0]).collect();
        // SAFETY: a null name is this process's own module.
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        let registered = WNDCLASSEXW {
            cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
            lpfnWndProc: Some(procedure),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            // SAFETY: the rest of the class is unused and zero is its absence.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: a class whose name and procedure live for the process.
        if unsafe { RegisterClassExW(&raw const registered) } == 0 {
            let _ = opened.send(Err(Unavailable));
            return;
        }
        // SAFETY: the class registered above; a top-level window, never shown.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
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
        };
        if window.is_null() {
            let _ = opened.send(Err(Unavailable));
            return;
        }
        let _ = opened.send(Ok(()));
        // SAFETY: an out buffer for each message; zero is a message the loop never reads.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: this thread's own queue, read into `message`.
        while unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) } > 0 {
            // SAFETY: a message the loop just read.
            unsafe { TranslateMessage(&raw const message) };
            // SAFETY: as above.
            unsafe { DispatchMessageW(&raw const message) };
        }
    }

    /// Says the session is ending at its query and at its end, agrees to it, and holds an end that happens until
    /// the program has parted, within its bound: Windows ends the process once this returns.
    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_QUERYENDSESSION => {
                TOLD.with(|told| told.borrow().as_ref().map(|told| (told.ending)(End::Asked)));
                LRESULT::from(true)
            }
            WM_ENDSESSION if wparam != usize::try_from(FALSE).unwrap_or(0) => {
                let held = TOLD.with(|told| {
                    told.borrow().as_ref().map(|told| {
                        (told.ending)(End::Happening);
                        (told.parted.clone(), told.bound)
                    })
                });
                if let Some((parted, bound)) = held {
                    parted.wait(bound);
                }
                0
            }
            // SAFETY: the window's own message, handed on as it came.
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const BOUND: Duration = Duration::from_secs(30);
    const SHORT: Duration = Duration::from_millis(80);

    #[test]
    fn a_wait_ends_when_parting_is_done_not_when_its_bound_passes() {
        let parted = Parted::default();
        let said = parted.clone();
        let saying = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            said.done();
        });
        let started = Instant::now();
        assert!(parted.wait(BOUND), "told it was done");
        assert!(started.elapsed() < BOUND / 10, "it did not wait out its bound");
        saying.join().unwrap();
        assert!(parted.wait(Duration::ZERO), "a parting done stays done");
    }

    #[test]
    fn a_wait_for_a_parting_that_never_ends_gives_up_at_its_bound() {
        let started = Instant::now();
        assert!(!Parted::default().wait(SHORT), "nobody said it was done");
        assert!(started.elapsed() >= SHORT, "and it waited its whole bound");
    }
}
