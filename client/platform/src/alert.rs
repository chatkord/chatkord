//! The last word to a person when the program cannot go on and has no window to say it in: a message box on Windows,
//! a desktop notification and a line on the error stream on Linux. Nothing a person sees is decided here.

use std::io::Write as _;

/// Tells the person, once and then returns, that the program is ending and why: `title` names the program, `body` is
/// one plain sentence. A system that cannot show it has still written it to the error stream.
pub fn fatal(title: &str, body: &str) {
    // Nothing is left to tell where this fails: the stream may be closed.
    let _ = writeln!(std::io::stderr(), "{title}: {body}");
    shown(title, body);
}

#[cfg(windows)]
fn shown(title: &str, body: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().filter(|unit| *unit != 0).chain([0]).collect() };
    let (title, body) = (wide(title), wide(body));
    // SAFETY: both texts are NUL-terminated and outlive the call; no owner window is the null handle.
    unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
}

#[cfg(not(windows))]
fn shown(title: &str, body: &str) {
    use crate::notify::{self, Popup};
    let Ok(popup) = Popup::new("startup-failed", title, body) else { return };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
    // A desktop with no notification service leaves the line on the error stream.
    let _ = runtime.block_on(notify::show(popup));
}
