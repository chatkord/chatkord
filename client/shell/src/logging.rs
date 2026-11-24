//! Where a run's lines go: stderr, every crate's from INFO up and more where `CHATKORD_LOG` asks, and this run's log
//! file ([`platform::logs`]), the notes alone that the debug report's ring keeps, so a dependency's line (an address in
//! hyper's, a device in wgpu's) never reaches a file.

use platform::logs::{FaultMark, Late, LogFile};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, ParseError, Targets, filter_fn};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// This run's log, once [`keep_this_run`] opens it.
static LOG: Late = Late::new();

/// What a thread that panicked writes beside this run's log, once [`keep_this_run`] has its path.
static FAULT: OnceLock<FaultMark> = OnceLock::new();

/// The variable naming targets stderr takes below INFO, as `shell::frames=debug,call=trace`.
const ASK: report::Word = report::word!("CHATKORD_LOG");

/// What stderr takes: INFO and above from every crate, and what `asked` names besides; a list that is not one is
/// refused.
fn stderr_targets(asked: Option<&str>) -> Result<Targets, ParseError> {
    let Some(asked) = asked else { return Ok(Targets::new().with_default(LevelFilter::INFO)) };
    let targets: Targets = asked.parse()?;
    Ok(if targets.default_level().is_some() { targets } else { targets.with_default(LevelFilter::INFO) })
}

/// The layer that writes a run's file: plain text, the notes any crate of ours makes and the debug report's ring keeps,
/// each with its target, and no line of a dependency. [`start`] installs it, and the test that
/// reads the file back builds it too, so what is read is what the product writes.
pub fn file_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer).with_filter(filter_fn(report::is_kept_note))
}

/// Starts logging, to stderr at once and to this run's file once [`keep_this_run`] opens it; the ring that keeps the
/// recent notes for a debug report is returned.
pub fn start() -> report::Ring {
    let asked = std::env::var(ASK.as_str()).ok();
    let (stderr, refused) = match stderr_targets(asked.as_deref()) {
        Ok(targets) => (targets, false),
        Err(_) => (Targets::new().with_default(LevelFilter::INFO), true),
    };
    let ring = report::Ring::new(std::time::SystemTime::now);
    tracing_subscriber::registry()
        .with(ring.layer())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_filter(stderr))
        .with(file_layer(|| &LOG))
        .init();
    note_panics();
    if refused {
        report::note!(
            warn,
            "the log variable is not a list of targets and levels, so only INFO and above reach stderr",
            variable = report::Fact::Word(ASK)
        );
    }
    ring
}

/// A source file's folder and stem as words, when they are lowercase letters, digits and `_`: the build's own path,
/// never runtime data, and anything else is withheld.
fn source_word(part: Option<&str>) -> report::Fact {
    match part {
        Some(word)
            if !word.is_empty()
                && word.len() <= 32
                && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') =>
        {
            report::Product::fact_of(word)
        }
        _ => report::Fact::Withheld,
    }
}

/// Notes that a thread panicked, with where in the source: the file's stem and folder, the line and the column, and
/// never the panic's message, which may hold what a person typed. A panic is not always the end of the run (a task or a
/// guarded call may survive it), so the note says no more.
fn panicked(location: Option<&std::panic::Location<'_>>) {
    let mut path = location.map_or("", |at| at.file()).rsplit(['/', '\\']);
    let file = source_word(path.next().and_then(|name| name.strip_suffix(".rs")));
    let folder = source_word(path.next());
    let (line, column) = (location.map(std::panic::Location::line), location.map(std::panic::Location::column));
    report::note!(error, "a thread panicked", file = file, folder = folder, line = line, column = column);
}

/// What a panic does on its thread: notes where it happened, marks the run faulted so the next start offers its
/// report even if this one exits cleanly, and, when `loud` (a debug build), writes the panic and its backtrace to
/// `stderr`. A release build writes nothing there, since a panic's message may hold what a person typed.
fn hook<W: Write + Send + 'static>(
    stderr: W,
    loud: bool,
    fault: &'static OnceLock<FaultMark>,
) -> impl Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static {
    let stderr = Mutex::new(stderr);
    move |info| {
        panicked(info.location());
        if let Some(fault) = fault.get() {
            fault.mark();
        }
        if loud && let Ok(mut stderr) = stderr.lock() {
            let _ = writeln!(stderr, "thread '{}' {info}", std::thread::current().name().unwrap_or("<unnamed>"));
            let backtrace = std::backtrace::Backtrace::capture();
            if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
                let _ = writeln!(stderr, "{backtrace}");
            }
        }
    }
}

/// From now on a panic is handled by [`hook`], over this process's stderr.
fn note_panics() {
    std::panic::set_hook(Box::new(hook(std::io::stderr(), cfg!(debug_assertions), &FAULT)));
}

/// From now on, lines go to `log` too, and a panic marks `fault`: the window's instance holds, so a launch that only
/// knocks keeps nothing.
pub fn keep_this_run(log: LogFile, fault: FaultMark) {
    LOG.start(log);
    let _ = FAULT.set(fault);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn stderr_takes_info_from_everyone_and_what_the_variable_names_below_it() {
        let plain = stderr_targets(None).unwrap();
        assert!(plain.would_enable("wgpu_core::device", &Level::INFO));
        assert!(!plain.would_enable("shell::frames", &Level::DEBUG));
        let asked = stderr_targets(Some("shell::frames=debug")).unwrap();
        assert!(asked.would_enable("shell::frames", &Level::DEBUG));
        assert!(!asked.would_enable("shell::window", &Level::DEBUG), "only what it names");
        assert!(asked.would_enable("hyper::proto", &Level::INFO), "everyone's INFO stays");
        assert!(stderr_targets(Some("debug")).unwrap().would_enable("hyper::proto", &Level::DEBUG));
        assert!(stderr_targets(Some("shell=loud")).is_err());
    }

    #[test]
    fn a_panic_note_names_the_place_in_the_source_and_nothing_a_panic_said() {
        let ring = report::Ring::new(std::time::SystemTime::now);
        let subscriber = tracing_subscriber::registry().with(ring.layer());
        tracing::subscriber::with_default(subscriber, || panicked(Some(std::panic::Location::caller())));
        let lines = ring.lines();
        let [line] = &lines[..] else { panic!("{lines:?}") };
        assert!(line.as_str().contains("error shell a thread panicked  file=logging  folder=src  line="));
        assert_eq!(source_word(Some("Ada Lovelace")), report::Fact::Withheld);
        assert_eq!(source_word(Some("")), report::Fact::Withheld);
    }

    fn folder_holding_a_log(root: &std::path::Path) -> Option<std::path::PathBuf> {
        let mut pending = vec![root.to_owned()];
        while let Some(folder) = pending.pop() {
            for entry in std::fs::read_dir(&folder).ok()?.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "log") {
                    return Some(folder);
                }
            }
        }
        None
    }

    #[test]
    fn every_line_the_file_layer_writes_for_a_note_reads_back_as_a_note() {
        static FILE: Late = Late::new();
        let root = std::env::temp_dir().join(format!("chatkord-logging-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let platform = platform::Platform::default().logs_in(root.clone());
        let opened = platform::logs::open(&platform, std::time::SystemTime::now(), std::process::id()).unwrap();
        FILE.start(opened.log);
        let subscriber = tracing_subscriber::registry().with(file_layer(|| &FILE));
        tracing::subscriber::with_default(subscriber, || {
            report::note!(info, "a share started", frames = 30_u32, hardware = true, delta = -4_i32);
            report::note!(info, "watched screen: first picture painted");
            report::note!(warn, "the camera is gone", gpu = report::Product::fact_of("AMD Radeon RX 9060 XT"));
            report::note!(error, "a call ended", reason = report::Fact::Absent);
            panicked(Some(std::panic::Location::caller()));
        });
        let folder = folder_holding_a_log(&root).expect("the log file of this run");
        let read = platform::logs::read_previous(&folder).expect("the file reads back");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!((read.lines().len(), read.refused()), (5, 0), "{read:?}");
    }

    /// A note as another crate of ours makes it, under that crate's target, and a dependency's own line.
    fn other_crates_lines() {
        tracing::event!(name: "note", target: "sound::voice", Level::INFO, report.note = true, "a device was lost");
        tracing::event!(target: "hyper::proto", Level::INFO, "a dependency's line");
    }

    #[test]
    fn a_note_from_any_crate_reaches_the_file() {
        static FILE: Late = Late::new();
        let root = std::env::temp_dir().join(format!("chatkord-any-crate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let platform = platform::Platform::default().logs_in(root.clone());
        let opened = platform::logs::open(&platform, std::time::SystemTime::now(), std::process::id()).unwrap();
        FILE.start(opened.log);
        let subscriber = tracing_subscriber::registry().with(file_layer(|| &FILE));
        tracing::subscriber::with_default(subscriber, || {
            other_crates_lines();
            report::note!(debug, "a note below INFO");
        });
        let folder = folder_holding_a_log(&root).expect("the log file of this run");
        let text =
            std::fs::read_to_string(std::fs::read_dir(&folder).unwrap().flatten().next().unwrap().path()).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(text.contains("sound::voice") && text.contains("a device was lost"), "{text}");
        assert!(!text.contains("hyper") && !text.contains("below INFO"), "{text}");
    }

    /// A writer a test reads back what the hook wrote to.
    #[derive(Clone, Default)]
    struct Said(std::sync::Arc<Mutex<Vec<u8>>>);

    impl Write for Said {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_release_hook_writes_nothing_to_stderr() {
        static NONE: OnceLock<FaultMark> = OnceLock::new();
        let (quiet, loud) = (Said::default(), Said::default());
        std::panic::set_hook(Box::new(hook(quiet.clone(), false, &NONE)));
        let _ = std::panic::catch_unwind(|| panic!("what a person typed"));
        assert!(quiet.0.lock().unwrap().is_empty(), "a release build says nothing on stderr");
        std::panic::set_hook(Box::new(hook(loud.clone(), true, &NONE)));
        let _ = std::panic::catch_unwind(|| panic!("what a developer reads"));
        let said = String::from_utf8(loud.0.lock().unwrap().clone()).unwrap();
        assert!(said.contains("what a developer reads"), "a debug build shows the panic: {said}");
        let _ = std::panic::take_hook();
    }
}
