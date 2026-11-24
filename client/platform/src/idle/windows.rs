//! The time since the last input of the session, which Windows keeps for the whole desktop.

use std::time::Duration;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Milliseconds between two readings of the tick count, which wraps every 49 days.
fn since(now: u32, last: u32) -> Duration {
    Duration::from_millis(u64::from(now.wrapping_sub(last)))
}

/// The time since the last input, or `None` where Windows will not say.
pub(super) fn idle_for() -> Option<Duration> {
    let mut last = LASTINPUTINFO { cbSize: u32::try_from(size_of::<LASTINPUTINFO>()).ok()?, dwTime: 0 };
    // SAFETY: `last` is a live LASTINPUTINFO whose `cbSize` is set, as the call requires.
    let told = unsafe { GetLastInputInfo(&raw mut last) };
    // SAFETY: no arguments; reads the tick count.
    let now = unsafe { GetTickCount() };
    (told != 0).then(|| since(now, last.dwTime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idle_time_survives_the_tick_count_wrapping() {
        assert_eq!(since(1_000, 400), Duration::from_millis(600));
        assert_eq!(since(100, u32::MAX - 99), Duration::from_millis(200));
    }

    mod lab {
        use super::*;

        #[test]
        fn a_session_with_a_desktop_tells_its_idle_time() {
            tier::lab();
            assert!(idle_for().is_some(), "the time since the last input");
        }
    }
}
