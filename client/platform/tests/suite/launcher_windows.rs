//! The process named for Windows' taskbar and toasts as the Start Menu shortcut names the program.

use platform::notify::APP_USER_MODEL_ID;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID;

/// The id this process was given, if it was given one.
fn named() -> Option<String> {
    // SAFETY: no argument; the string it returns is ours to free.
    let id = unsafe { GetCurrentProcessExplicitAppUserModelID() }.ok()?;
    // SAFETY: `id` is the live, null-terminated wide string the call above returned.
    let name = unsafe { id.to_string() }.ok();
    // SAFETY: `id` came from the shell's allocator and is not read again.
    unsafe { CoTaskMemFree(Some(id.as_ptr().cast())) };
    name
}

#[test]
fn a_start_names_the_process_by_the_shortcut_s_id() {
    assert_eq!(named(), None, "a process is not named until it asks");
    platform::launcher::keep().unwrap();
    assert_eq!(named().as_deref(), Some(APP_USER_MODEL_ID));
}
