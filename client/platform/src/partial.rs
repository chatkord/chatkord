//! Partial files a run that was killed left behind: each owner writes `<name>.<pid>.part` beside the file it makes
//! and renames it over, so a name that carries the pid of a process no longer running is stale, and a name that
//! carries none is stale when it is older than this run. Swept once at start, each in its owner's folder.

use std::path::Path;
use std::time::SystemTime;

use crate::Platform;

/// What a partial file's name ends with.
pub(crate) const SUFFIX: &str = ".part";

/// Whether the partial file `name` (ending in `suffix`) last written at `modified` is a dead run's: its pid, the last
/// dotted word before the suffix, names no process `running` knows; with no pid, it is from before `started`.
pub fn stale(
    name: &str,
    suffix: &str,
    modified: SystemTime,
    running: impl Fn(u32) -> bool,
    started: SystemTime,
) -> bool {
    let Some(stem) = name.strip_suffix(suffix) else { return false };
    match stem.rsplit('.').next().and_then(|word| word.parse::<u32>().ok()) {
        Some(pid) => !running(pid),
        None => modified < started,
    }
}

/// Takes away every stale partial file ending in `suffix` directly in `folder`; how many went. A folder that is not
/// there, or a file that will not go, is left alone.
pub fn sweep(folder: &Path, suffix: &str, running: impl Fn(u32) -> bool, started: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    let mut swept = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(modified) = entry.metadata().ok().filter(std::fs::Metadata::is_file).and_then(|m| m.modified().ok())
        else {
            continue;
        };
        if stale(name, suffix, modified, &running, started) && std::fs::remove_file(entry.path()).is_ok() {
            swept += 1;
        }
    }
    swept
}

/// Sweeps every folder this crate writes a partial file in, as [`sweep`] does, deciding by `running` and `started`.
pub fn swept_with(platform: &Platform, running: impl Fn(u32) -> bool + Copy, started: SystemTime) -> usize {
    crate::state::sweep(platform, running, started)
        + crate::cache::sweep(platform, running, started)
        + crate::startup::sweep(platform, running, started)
}

/// [`swept_with`] by the processes this system runs; called once at start, `started` being when this run began.
pub fn swept(platform: &Platform, started: SystemTime) -> usize {
    swept_with(platform, running, started)
}

/// Whether process `pid` is running now.
#[cfg(target_os = "linux")]
pub fn running(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Whether process `pid` is running now: one that cannot be opened for its state is taken as running.
#[cfg(windows)]
pub fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// The exit code of a process that has not ended.
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: a plain call; a handle it returns is closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // SAFETY: reads this thread's last error, set by the call above.
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut code = 0;
    // SAFETY: a live handle and an out value that lives for the call.
    let read = unsafe { GetExitCodeProcess(handle, &raw mut code) };
    // SAFETY: the handle opened above, closed once.
    unsafe { CloseHandle(handle) };
    read == 0 || code == STILL_ACTIVE
}

/// Whether process `pid` is running now: no system here says, so every one is.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DEAD: u32 = 4_000_000_001;
    const ALIVE: u32 = 7;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chatkord-partial-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn lives(pid: u32) -> bool {
        pid == ALIVE
    }

    #[test]
    fn a_name_with_a_pid_is_stale_when_that_process_is_gone_whatever_its_age() {
        let (started, young) = (SystemTime::UNIX_EPOCH, SystemTime::UNIX_EPOCH + Duration::from_secs(9));
        assert!(stale(&format!("chatkord.json.{DEAD}.part"), SUFFIX, young, lives, started));
        assert!(!stale(&format!("chatkord.json.{ALIVE}.part"), SUFFIX, SystemTime::UNIX_EPOCH, lives, young));
    }

    #[test]
    fn a_name_without_a_pid_is_stale_when_it_is_older_than_this_run() {
        let started = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let name = "0fa1000000000000000000000000ff10.part";
        assert!(stale(name, SUFFIX, SystemTime::UNIX_EPOCH, lives, started));
        assert!(!stale(name, SUFFIX, started + Duration::from_secs(1), lives, started));
    }

    #[test]
    fn a_file_that_is_not_a_partial_is_never_stale() {
        assert!(!stale(&format!("chatkord.json.{DEAD}"), SUFFIX, SystemTime::UNIX_EPOCH, lives, SystemTime::now()));
    }

    #[test]
    fn a_sweep_takes_away_a_dead_runs_partial_files_and_leaves_the_rest() {
        let dir = scratch("sweep");
        let (dead, alive, whole) =
            (dir.join(format!("a.{DEAD}.part")), dir.join(format!("a.{ALIVE}.part")), dir.join("a"));
        for file in [&dead, &alive, &whole] {
            std::fs::write(file, b"x").unwrap();
        }
        assert_eq!(sweep(&dir, SUFFIX, lives, SystemTime::now()), 1);
        assert_eq!((dead.exists(), alive.exists(), whole.exists()), (false, true, true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_start_sweeps_the_kept_file_and_the_cache_of_a_dead_runs_partials() {
        let dir = scratch("owners");
        let platform = Platform::default().config_in(dir.join("config")).cache_in(dir.join("cache"));
        let kept = dir.join("config").join("chatkord");
        let files = dir.join("cache").join("chatkord").join("files");
        let (kept_dead, kept_alive) = (kept.join(format!("chatkord.json.{DEAD}.part")), kept.join("chatkord.json"));
        let (file_dead, file_alive) =
            (files.join(format!("ff10.{DEAD}.part")), files.join(format!("ff10.{ALIVE}.part")));
        for file in [&kept_dead, &kept_alive, &file_dead, &file_alive] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        }
        assert_eq!(swept_with(&platform, lives, SystemTime::now()), 2);
        assert_eq!(
            [kept_dead.exists(), kept_alive.exists(), file_dead.exists(), file_alive.exists()],
            [false, true, false, true]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn this_process_is_running_and_a_pid_far_past_any_is_not() {
        assert!(running(std::process::id()));
        assert!(!running(DEAD));
    }
}
