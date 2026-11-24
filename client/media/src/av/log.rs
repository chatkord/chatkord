//! FFmpeg's own log, taken from its stderr.

use ffmpeg_sys_next as ff;
use std::ffi::{c_char, c_int, c_void};
use std::sync::{Once, OnceLock};

/// How the binding spells FFmpeg's `va_list` argument on this target.
#[cfg(all(unix, target_arch = "x86_64"))]
type Arguments = *mut ff::__va_list_tag;
#[cfg(not(all(unix, target_arch = "x86_64")))]
type Arguments = ff::va_list;

static SINK: OnceLock<fn(i32)> = OnceLock::new();

/// Takes FFmpeg's log for the rest of the process, handing `sink` the level of every line at warning or worse and
/// nothing else of it. Called before any work that may make FFmpeg log; the first sink stays.
pub fn forward_log(sink: fn(i32)) {
    static FORWARD: Once = Once::new();
    let _ = SINK.set(sink);
    // SAFETY: the callback reads none of its arguments and may be called from any thread, as FFmpeg's may.
    FORWARD.call_once(|| unsafe { ff::av_log_set_callback(Some(forwarded)) });
}

/// FFmpeg's log callback. Its text is foreign and names paths, devices and addresses, so only its level is kept, never
/// the line, and nothing below a warning (a larger number) is even looked at.
unsafe extern "C" fn forwarded(_: *mut c_void, level: c_int, _: *const c_char, _: Arguments) {
    if level > ff::AV_LOG_WARNING as c_int {
        return;
    }
    if let Some(sink) = SINK.get() {
        sink(level);
    }
}

/// The level of an error line.
#[cfg(test)]
pub const ERROR_LEVEL: i32 = ff::AV_LOG_ERROR;

/// Makes FFmpeg log one line at the error level, as its own code would.
#[cfg(test)]
pub fn log_error_line() {
    // SAFETY: a NUL-terminated format with one string argument, as `av_log` takes.
    unsafe {
        ff::av_log(std::ptr::null_mut(), ERROR_LEVEL, c"chatkord %s\n".as_ptr(), c"probe".as_ptr());
    };
}
