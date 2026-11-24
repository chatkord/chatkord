//! The window started as a person starts it, on a home of the test's own, and read from outside: its top-level window
//! found by its process, and the process waited for as it ends.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
};
use windows_sys::core::BOOL;

/// Long enough for a debug window's first picture on Machine A.
pub(crate) const SHOWN: Duration = Duration::from_secs(60);
/// The close path parts within 2 s (`link`'s `PARTING`); the rest is the process ending.
pub(crate) const ENDED: Duration = Duration::from_secs(10);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A folder of the test's own for the window's settings and logs, removed when dropped.
pub(crate) struct Home(PathBuf);

impl Home {
    pub(crate) fn new(test: &str) -> Self {
        let home = std::env::temp_dir().join(format!("chatkord-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        Self(home)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Chooses, as Settings › Startup does, that closing the window quits rather than going to the tray.
    pub(crate) fn closing_quits(&self) {
        let folder = self.0.join(channel::THIS.id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(platform::kept::FILE),
            br#"{"startup":{"format":1,"settings":{"close_to":"quit"}}}"#,
        )
        .unwrap();
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The window's process, ended when dropped, so a test that fails leaves no window behind.
pub(crate) struct Running(Child);

impl std::ops::Deref for Running {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Running {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// The window, its settings under `home`.
pub(crate) fn start(home: &Home) -> Running {
    start_from(Path::new(env!("CARGO_BIN_EXE_shell")), home)
}

/// A copy of the window's program in `home`: Restart Manager closes every process of the file it is given, so a test
/// of it closes only its own window, never another test's.
pub(crate) fn own_copy(home: &Home) -> PathBuf {
    let copy = home.path().join("chatkord-window.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_shell"), &copy).unwrap();
    copy
}

/// The window from the program at `exe`, its settings under `home`.
pub(crate) fn start_from(exe: &Path, home: &Home) -> Running {
    Running(
        Command::new(exe)
            .env("APPDATA", home.path())
            .env("LOCALAPPDATA", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap(),
    )
}

/// Which of a process's visible top-level windows a test waits for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    /// The largest with a title: the main window.
    Main,
    /// Any, titled or not: winit's own untitled one shows first, before the main window is made.
    First,
}

/// The process's visible top-level window `which` once it shows, within [`SHOWN`]; none if it ended or never showed.
pub(crate) fn shown(child: &mut Child, which: Which) -> Option<HWND> {
    let started = Instant::now();
    while started.elapsed() < SHOWN {
        if child.try_wait().unwrap().is_some() {
            return None;
        }
        if let Some(window) = visible_window(child.id(), which) {
            return Some(window);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// How the process ended, within [`ENDED`]; none while it runs.
pub(crate) fn exited(child: &mut Child) -> Option<ExitStatus> {
    let started = Instant::now();
    while started.elapsed() < ENDED {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// The window `which` that `process` shows: [`Which::Main`] is the largest of its visible top-level windows with a
/// title, since winit keeps a visible, untitled one of its own (15 pixels at the desktop's corner, measured).
pub(crate) fn visible_window(process: u32, which: Which) -> Option<HWND> {
    struct Search {
        process: u32,
        which: Which,
        found: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn visit(window: HWND, search: LPARAM) -> BOOL {
        // SAFETY: `search` is the `Search` below, alive for the whole enumeration.
        let search = unsafe { &mut *(search as *mut Search) };
        let mut owner = 0;
        // SAFETY: `window` is a window the enumeration handed over; `owner` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(window, &raw mut owner) };
        // SAFETY: as above.
        let counts = search.which == Which::First || unsafe { GetWindowTextLengthW(window) } > 0;
        // SAFETY: as above.
        if owner == search.process && counts && unsafe { IsWindowVisible(window) } != 0 {
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: as above; `rect` is a valid out pointer.
            if unsafe { GetWindowRect(window, &raw mut rect) } != 0 {
                let area = i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top);
                if search.found.is_none_or(|(_, largest)| area > largest) {
                    search.found = Some((window, area));
                }
            }
        }
        TRUE
    }
    let mut search = Search { process, which, found: None };
    // SAFETY: `visit` reads `search` only during this call.
    unsafe { EnumWindows(Some(visit), &raw mut search as LPARAM) };
    search.found.map(|(window, _)| window)
}
