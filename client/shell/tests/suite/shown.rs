//! A minimised window is hidden: Windows still paints it when asked and says nothing of its minimise but its size, so
//! the window reads it, lends its own pictures nothing and says so in its log, and shows again once restored.

mod lab {
    use crate::common::{Home, Which, shown, start};
    use std::path::Path;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_MINIMIZE, SW_RESTORE, ShowWindow};

    /// How long the window may take to say what it was told.
    const SAID: Duration = Duration::from_secs(5);

    /// The run's log lines, from the one log under `home`.
    fn log(home: &Path) -> String {
        let logs = home.join(channel::THIS.id).join("logs");
        std::fs::read_dir(&logs)
            .map(|entries| {
                entries.filter_map(Result::ok).filter_map(|entry| std::fs::read_to_string(entry.path()).ok()).collect()
            })
            .unwrap_or_default()
    }

    /// Waits until the log says `line` as many times as `times`.
    fn says(home: &Path, line: &str, times: usize) -> bool {
        let asked = Instant::now();
        while asked.elapsed() < SAID {
            if log(home).matches(line).count() >= times {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    #[test]
    fn a_minimised_window_is_hidden_and_shows_again_once_restored() {
        tier::lab();
        let home = Home::new("shown");
        let mut running = start(&home);
        let window = shown(&mut running, Which::Main).expect("the window showed");
        std::thread::sleep(Duration::from_secs(1));
        // SAFETY: a live window.
        unsafe { ShowWindow(window, SW_MINIMIZE) };
        assert!(says(home.path(), "the window is hidden", 1), "a minimised window is hidden");
        // SAFETY: a live window.
        unsafe { ShowWindow(window, SW_RESTORE) };
        assert!(says(home.path(), "the window shows", 1), "restored, it shows");
    }
}
