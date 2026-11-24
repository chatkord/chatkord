//! What a run logs, kept in a file of its own beside stderr, where a desktop launch (and a Windows release, which has
//! no console) would lose it: `<state>/chatkord/logs/<seconds>-<pid>.log`, the newest [`KEEP`] runs kept, each up to
//! [`CAP`]. The folder is named by [`Platform`]; a program that named none
//! keeps no log. What goes in is only what `report::note!` accepts: a literal and public facts, so no secret, name,
//! host, address or path can reach it. [`open`] finds the newest earlier run's log before it makes this run's, and
//! [`Earlier::read`] reads it back for a debug report, only as `report::Previous` accepts it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Platform, Unavailable};

const FOLDER: &str = "chatkord";
const LOGS: &str = "logs";
const EXTENSION: &str = "log";
/// What a run's mark that it ended cleanly is named by: its log's name with this extension.
const CLEAN: &str = "clean";
/// What a run's mark that a thread of it panicked is named by: its log's name with this extension.
const FAULT: &str = "fault";

/// How many runs' logs are kept, this one's included.
pub const KEEP: usize = 5;

/// The most bytes a run's log takes: past it, one line says so and the rest goes to stderr alone.
pub const CAP: u64 = 8 * 1024 * 1024;

const FULL: &[u8] = b"the log is full; later lines go to stderr only\n";

/// The system's folder for what a program keeps between runs that is not configuration: `XDG_STATE_HOME`, else
/// `$HOME/.local/state` on Linux; `LOCALAPPDATA` on Windows. Only a path stated from the root counts.
#[cfg(target_os = "linux")]
pub(crate) fn directory() -> Option<PathBuf> {
    from_env("XDG_STATE_HOME").or_else(|| from_env("HOME").map(|home| home.join(".local").join("state")))
}

#[cfg(target_os = "windows")]
pub(crate) fn directory() -> Option<PathBuf> {
    from_env("LOCALAPPDATA")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn directory() -> Option<PathBuf> {
    None
}

fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var(name).ok().map(PathBuf::from).filter(|stated| stated.is_absolute())
}

fn folder_in(root: &Path) -> PathBuf {
    root.join(FOLDER).join(LOGS)
}

/// One run's log: written whole lines at a time until it holds [`CAP`].
#[derive(Debug)]
pub struct LogFile {
    file: File,
    written: u64,
    full: bool,
}

/// The log of the newest run before this one, found before this run's file was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Earlier(PathBuf);

/// A run's log started, and the earlier run's log if one was left.
#[derive(Debug)]
pub struct Opened {
    /// This run's log, to hand to the logger.
    pub log: LogFile,
    /// The newest earlier run's log, for a debug report to read.
    pub earlier: Option<Earlier>,
    /// What this run writes when it ends cleanly, so the next run can tell it did.
    pub exit: CleanExit,
    /// What the panic hook writes when a thread of this run panics, so the next run offers its report though this one
    /// went on to end cleanly.
    pub fault: FaultMark,
}

/// The mark that this run ended cleanly, written beside its log at a clean exit and never at a crash or a kill.
#[derive(Debug)]
pub struct CleanExit(PathBuf);

impl CleanExit {
    /// Writes the mark; a folder that cannot take it only costs the next run a report offered for nothing.
    pub fn mark(self) {
        if fs::write(&self.0, b"").is_err() {
            report::note!(warn, "this run's clean exit could not be marked");
        }
    }
}

/// The mark that a thread of this run panicked, its path made at start so the panic hook only writes.
#[derive(Debug)]
pub struct FaultMark(PathBuf);

impl FaultMark {
    /// Writes the mark, once or again; a folder that cannot take it only costs the next run a report not offered.
    pub fn mark(&self) {
        let _ = fs::write(&self.0, b"");
    }
}

/// Starts this run's log, started at `now` by process `pid`, and takes away all but the newest runs' before it.
/// Blocking: called once at start, before the window.
pub fn open(platform: &Platform, now: SystemTime, pid: u32) -> Result<Opened, Unavailable> {
    let root = platform.logs().ok_or(Unavailable)?;
    open_in(&folder_in(root), now, pid)
}

impl Earlier {
    /// Whether the run that made this log ended cleanly, marked as [`CleanExit::mark`] does, and no thread of it
    /// panicked, which [`FaultMark::mark`] says.
    pub fn ended_cleanly(&self) -> bool {
        self.0.with_extension(CLEAN).exists() && !self.0.with_extension(FAULT).exists()
    }

    /// The notes of this log, read back strictly and bounded; none when it cannot be read. On the blocking pool: the
    /// disk may be slow.
    pub async fn read(self) -> Option<report::Previous> {
        tokio::task::spawn_blocking(move || self.read_now()).await.ok()?
    }

    /// The newest log in `folder`, whose names sort by start.
    fn newest_in(folder: &Path) -> Option<Self> {
        let newest = fs::read_dir(folder)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == EXTENSION))
            .max()?;
        Some(Self(newest))
    }

    /// Read from the log's last [`report::READ_BACK`] bytes. Blocking.
    fn read_now(&self) -> Option<report::Previous> {
        let back = u64::try_from(report::READ_BACK).unwrap_or(u64::MAX);
        let mut file = File::open(&self.0).ok()?;
        let length = file.metadata().ok()?.len();
        let start = length.saturating_sub(back);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.take(back).read_to_end(&mut bytes).ok()?;
        // A read that began inside the file starts inside a line, which is not one.
        let skipped =
            if start > 0 { bytes.iter().position(|&byte| byte == b'\n').map_or(bytes.len(), |at| at + 1) } else { 0 };
        let whole = bytes.get(skipped..)?;
        Some(report::Previous::read(&String::from_utf8_lossy(whole)))
    }
}

/// The notes of the newest log in `folder`, as [`Earlier::read`] gives them, for a test of what a log file may add to
/// a report. Blocking.
pub fn read_previous(folder: &Path) -> Option<report::Previous> {
    Earlier::newest_in(folder)?.read_now()
}

fn open_in(folder: &Path, now: SystemTime, pid: u32) -> Result<Opened, Unavailable> {
    fs::create_dir_all(folder).map_err(|_| Unavailable)?;
    let seconds = now.duration_since(UNIX_EPOCH).map_err(|_| Unavailable)?.as_secs();
    prune(folder, KEEP - 1)?;
    let earlier = Earlier::newest_in(folder);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let path = folder.join(format!("{seconds:020}-{pid}.{EXTENSION}"));
    let file = options.open(&path).map_err(|_| Unavailable)?;
    let exit = CleanExit(path.with_extension(CLEAN));
    let fault = FaultMark(path.with_extension(FAULT));
    Ok(Opened { log: LogFile { file, written: 0, full: false }, earlier, exit, fault })
}

/// Takes away all but the newest `kept` logs in `folder`: their names start with the second they began, padded, so
/// the newest sort last.
fn prune(folder: &Path, kept: usize) -> Result<(), Unavailable> {
    let mut logs: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|_| Unavailable)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == EXTENSION))
        .collect();
    logs.sort();
    let excess = logs.len().saturating_sub(kept);
    for old in &logs[..excess] {
        fs::remove_file(old).map_err(|_| Unavailable)?;
        let _ = fs::remove_file(old.with_extension(CLEAN));
        let _ = fs::remove_file(old.with_extension(FAULT));
    }
    Ok(())
}

/// Takes away every run's log; nothing kept is done already.
pub(crate) fn remove(platform: &Platform) -> Result<(), Unavailable> {
    match platform.logs() {
        Some(root) => crate::removed(&folder_in(root)),
        None => Ok(()),
    }
}

impl Write for LogFile {
    fn write(&mut self, line: &[u8]) -> io::Result<usize> {
        if self.full {
            return Ok(line.len());
        }
        let length = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(length) > CAP {
            self.full = true;
            self.file.write_all(FULL)?;
            return Ok(line.len());
        }
        self.file.write_all(line)?;
        self.written += length;
        Ok(line.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// This run's log once it is open, shared by every line a logger writes: lines before it opens, or after it failed
/// to, are not kept.
#[derive(Debug, Default)]
pub struct Late(OnceLock<Mutex<LogFile>>);

impl Late {
    /// None yet.
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    /// From now on, lines go to `log`.
    pub fn start(&self, log: LogFile) {
        let _ = self.0.set(Mutex::new(log));
    }
}

impl Late {
    /// The log, locked; none before it opens, and none for a thread that is panicking while another holds it: the
    /// panic hook writes on the thread that panicked, which may be the holder, and must not wait for itself.
    fn locked(&self) -> Option<MutexGuard<'_, LogFile>> {
        let log = self.0.get()?;
        match log.try_lock() {
            Ok(held) => Some(held),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) if std::thread::panicking() => None,
            Err(TryLockError::WouldBlock) => Some(log.lock().unwrap_or_else(PoisonError::into_inner)),
        }
    }
}

impl Write for &Late {
    fn write(&mut self, line: &[u8]) -> io::Result<usize> {
        match self.locked() {
            Some(mut log) => log.write(line),
            None => Ok(line.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.locked() {
            Some(mut log) => log.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-logs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn the_newest_five_runs_are_kept() {
        let folder = scratch("keep");
        for run in 0..7u32 {
            open_in(&folder, at(1_000 + u64::from(run)), run).unwrap();
        }
        let mut left: Vec<String> =
            fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left.len(), KEEP);
        assert!(left[0].ends_with("-2.log") && left[4].ends_with("-6.log"), "the oldest two went: {left:?}");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_run_that_ended_cleanly_is_told_from_one_that_did_not_by_its_mark() {
        let folder = scratch("clean");
        open_in(&folder, at(10), 1).unwrap().exit.mark();
        let crashed = open_in(&folder, at(20), 2).unwrap();
        assert!(crashed.earlier.as_ref().is_some_and(Earlier::ended_cleanly), "the run before it marked its exit");
        drop(crashed);
        let after = open_in(&folder, at(30), 3).unwrap();
        assert!(after.earlier.as_ref().is_some_and(|earlier| !earlier.ended_cleanly()), "the unmarked run crashed");
        assert!(open_in(&scratch("first"), at(1), 1).unwrap().earlier.is_none(), "a first run has none to offer");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_run_that_panicked_is_offered_its_report_though_it_exited_cleanly() {
        let folder = scratch("fault");
        let panicked = open_in(&folder, at(10), 1).unwrap();
        panicked.fault.mark();
        panicked.exit.mark();
        let next = open_in(&folder, at(20), 2).unwrap();
        assert!(next.earlier.as_ref().is_some_and(|earlier| !earlier.ended_cleanly()), "a fault outweighs the exit");
        next.exit.mark();
        let after = open_in(&folder, at(30), 3).unwrap();
        assert!(after.earlier.as_ref().is_some_and(Earlier::ended_cleanly), "a run without a fault is clean");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_log_pruned_takes_its_mark_with_it() {
        let folder = scratch("marks");
        for run in 0..7u32 {
            let opened = open_in(&folder, at(1_000 + u64::from(run)), run).unwrap();
            opened.exit.mark();
            opened.fault.mark();
        }
        let with = |wanted: &str| {
            fs::read_dir(&folder)
                .unwrap()
                .filter(|entry| entry.as_ref().unwrap().path().extension().is_some_and(|found| found == wanted))
                .count()
        };
        assert_eq!(
            (with(EXTENSION), with(CLEAN), with(FAULT)),
            (KEEP, KEEP, KEEP),
            "a mark for each log kept, none for one gone"
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_run_past_its_cap_writes_no_more_and_says_so_once() {
        let folder = scratch("cap");
        let mut log = open_in(&folder, at(5), 1).unwrap().log;
        let mut line = vec![b'x'; 1024 * 1024 - 1];
        line.push(b'\n');
        for _ in 0..10 {
            assert_eq!(log.write(&line).unwrap(), line.len(), "a full log still takes every line");
        }
        let written = fs::read(folder.join(format!("{:020}-1.log", 5))).unwrap();
        assert_eq!(written.len() as u64, CAP + FULL.len() as u64, "eight lines, then the one that says so");
        assert!(written.ends_with(FULL));
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_newest_earlier_log_is_found_before_this_runs_is_made_and_read_back() {
        let folder = scratch("previous");
        assert!(open_in(&folder, at(5), 3).unwrap().earlier.is_none(), "no earlier run, no section");
        let note = |number: u32| {
            format!("2026-11-17T10:42:02.123456Z  INFO shell: a share started report.note=true number={number}\n")
        };
        fs::write(folder.join(format!("{:020}-1.log", 10)), note(1)).unwrap();
        let earlier = note(2) + "not a note\n" + &note(3);
        fs::write(folder.join(format!("{:020}-2.log", 20)), earlier).unwrap();
        let opened = open_in(&folder, at(30), 3).unwrap();
        let read = opened.earlier.expect("the newest earlier log").read_now().expect("it reads");
        assert_eq!((read.lines().len(), read.refused()), (2, 1));
        assert!(read.lines()[0].as_str().ends_with("number=2"), "{:?}", read.lines());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_log_longer_than_the_read_is_read_from_its_last_whole_line() {
        let folder = scratch("tail");
        fs::create_dir_all(&folder).unwrap();
        let note = "2026-11-17T10:42:02.123456Z  INFO shell: a share started report.note=true number=1\n";
        let many = report::READ_BACK / note.len() + 5;
        fs::write(folder.join(format!("{:020}-1.log", 10)), note.repeat(many)).unwrap();
        let read = read_previous(&folder).expect("the log");
        assert_eq!((read.lines().len(), read.refused()), (report::KEPT, 0), "a cut first line is skipped, not refused");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_log_is_read_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let folder = scratch("mode");
        open_in(&folder, at(9), 3).unwrap();
        let mode = fs::metadata(folder.join(format!("{:020}-3.log", 9))).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_folder_is_the_systems_state_folder() {
        #[cfg(target_os = "linux")]
        assert!(
            directory()
                .is_none_or(|here| here.ends_with(".local/state") || std::env::var_os("XDG_STATE_HOME").is_some())
        );
        assert_eq!(folder_in(Path::new("/s")), Path::new("/s").join("chatkord").join("logs"));
    }
}
