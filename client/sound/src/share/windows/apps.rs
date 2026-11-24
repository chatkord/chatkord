//! The applications playing sound now on Windows, each as the process tree its loopback hears: the audio sessions of
//! every active output, grouped by their programs' processes as one snapshot of the system lists them.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    AudioSessionStateExpired, DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    eRender,
};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::Interface;

use super::SoundError;

/// How far up a process's parents are followed: deeper than any real tree, and a bound on a loop reused ids could make.
const ANCESTORS: usize = 64;

/// `pid` and each parent above it, as far as the bound.
fn ancestors(table: &HashMap<u32, Process>, pid: u32) -> impl Iterator<Item = u32> + '_ {
    std::iter::successors(Some(pid), |pid| table.get(pid).map(|process| process.parent)).take(ANCESTORS)
}

/// Whether the tree of `root` holds `ours`: `ours` is `root` or lies below it.
fn holds(root: u32, table: &HashMap<u32, Process>, ours: u32) -> bool {
    ancestors(table, ours).any(|up| up == root)
}

/// Whether the tree of any of `roots` holds this process, as the system's processes stand now. A window whose owner
/// does is one of ours or one that launched us (the shell, a terminal), and hearing its tree would hear the call.
pub fn holds_us(roots: &[u32]) -> bool {
    let ours = std::process::id();
    match processes() {
        Ok(table) => roots.iter().any(|&root| holds(root, &table, ours)),
        Err(_) => roots.contains(&ours),
    }
}

/// The first of `roots` whose tree does not hold this process, for a loopback of that tree.
pub(super) fn root_not_holding_us(roots: &[u32]) -> Option<u32> {
    let ours = std::process::id();
    let table = processes().unwrap_or_default();
    roots.iter().copied().find(|&root| !holds(root, &table, ours))
}

/// An application whose sound can go with a share: one program's processes, heard as one tree. A restart is another.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct App {
    /// Its program's file name without `.exe`, as the system lists it: what a person types.
    pub name: String,
    /// The process heard with its children: the topmost of its program's own processes above one that plays.
    pub root: u32,
}

impl App {
    /// Whether `typed` names it: its program, with or without `.exe`, in any case, as Windows reads a file name.
    pub fn answers_to(&self, typed: &str) -> bool {
        let cut = typed.len().saturating_sub(".exe".len());
        let program = match (typed.get(..cut), typed.get(cut..)) {
            (Some(program), Some(end)) if end.eq_ignore_ascii_case(".exe") => program,
            _ => typed,
        };
        program.eq_ignore_ascii_case(&self.name)
    }
}

/// A process as the snapshot lists it.
#[derive(Debug)]
struct Process {
    parent: u32,
    program: String,
}

/// The applications playing now, in name order, this process's tree and any tree that holds it left out. Blocks on
/// COM and the system's snapshot: called on the sound owner's thread, whose apartment `devices` belongs to.
fn apps(devices: &IMMDeviceEnumerator) -> Result<Vec<App>, SoundError> {
    let playing = sessions(devices);
    // SAFETY: reads this process's own id.
    let ours = unsafe { GetCurrentProcessId() };
    Ok(group(playing.map_err(|_| SoundError::Unavailable)?, &processes()?, ours))
}

/// The applications an ask for them is answered with, listed when asked: Windows lists its sessions at once.
pub(crate) struct Listen {
    apps: Vec<App>,
    until: std::time::Instant,
}

impl Listen {
    /// Lists them at `now` from `devices`.
    pub(crate) fn start(devices: &IMMDeviceEnumerator, now: std::time::Instant) -> Result<Self, SoundError> {
        apps(devices).map(|apps| Self { apps, until: now })
    }

    /// When it can be read: when it was asked.
    pub(crate) fn until(&self) -> std::time::Instant {
        self.until
    }

    /// The applications listed.
    pub(crate) fn heard(self) -> Vec<App> {
        self.apps
    }
}

/// The process of every live audio session on every active output; the system's sounds, process 0, left out.
fn sessions(devices: &IMMDeviceEnumerator) -> windows::core::Result<Vec<u32>> {
    // SAFETY: a live enumerator.
    let outputs = unsafe { devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }?;
    let mut pids = Vec::new();
    // SAFETY: a live collection.
    for index in 0..unsafe { outputs.GetCount() }? {
        // SAFETY: an index under the collection's count.
        let device = unsafe { outputs.Item(index) }?;
        // An output that will not list its sessions has none to offer: the others still do.
        // SAFETY: a live device.
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) }) else { continue };
        // SAFETY: a live session manager.
        let Ok(list) = (unsafe { manager.GetSessionEnumerator() }) else { continue };
        // SAFETY: a live session list.
        for at in 0..unsafe { list.GetCount() }.unwrap_or(0) {
            // SAFETY: an index under the list's count.
            let Ok(session) = (unsafe { list.GetSession(at) }).and_then(|one| one.cast::<IAudioSessionControl2>())
            else {
                continue;
            };
            // SAFETY: a live session.
            if unsafe { session.GetState() }.is_ok_and(|state| state != AudioSessionStateExpired) {
                // SAFETY: a live session.
                if let Ok(pid @ 1..) = unsafe { session.GetProcessId() } {
                    pids.push(pid);
                }
            }
        }
    }
    Ok(pids)
}

/// Every process now, by id: its parent and its program's file name, read from one snapshot, none opened.
fn processes() -> Result<HashMap<u32, Process>, SoundError> {
    // SAFETY: a snapshot of every process, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|_| SoundError::Unavailable)?;
    #[expect(clippy::cast_possible_truncation, reason = "a Win32 struct's size, far under u32::MAX")]
    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut table = HashMap::new();
    // SAFETY: an open snapshot and an entry whose size is set, as the call requires.
    let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok();
    while more {
        let length = entry.szExeFile.iter().position(|&unit| unit == 0).unwrap_or(entry.szExeFile.len());
        let program = String::from_utf16_lossy(&entry.szExeFile[..length]);
        table.insert(entry.th32ProcessID, Process { parent: entry.th32ParentProcessID, program });
        // SAFETY: as above, the entry's size still set.
        more = unsafe { Process32NextW(snapshot, &raw mut entry) }.is_ok();
    }
    // SAFETY: the snapshot opened above, used no more.
    let _ = unsafe { CloseHandle(snapshot) };
    Ok(table)
}

/// The applications `playing` belong to, in name order, each once. A process goes up to the topmost parent of its own
/// program, so a browser's audio process is heard as the browser; one in `ours`'s tree, or above it, is left out.
fn group(playing: impl IntoIterator<Item = u32>, table: &HashMap<u32, Process>, ours: u32) -> Vec<App> {
    let mut apps = BTreeSet::new();
    for pid in playing {
        let Some(process) = table.get(&pid) else { continue };
        let root = ancestors(table, pid)
            .take_while(|up| table.get(up).is_some_and(|other| other.program.eq_ignore_ascii_case(&process.program)))
            .last()
            .unwrap_or(pid);
        if holds(ours, table, pid) || holds(root, table, ours) {
            continue;
        }
        let name = Path::new(&process.program)
            .file_stem()
            .map_or_else(|| process.program.clone(), |stem| stem.to_string_lossy().into_owned());
        apps.insert(App { name, root });
    }
    apps.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: u32 = 400;
    const SHELL: u32 = 4;
    const BRAVE: u32 = 500;
    const BRAVE_AUDIO: u32 = 510;
    const PLAYER: u32 = 600;
    const OUR_CHILD: u32 = 410;

    fn table(rows: &[(u32, u32, &str)]) -> HashMap<u32, Process> {
        rows.iter().map(|&(pid, parent, program)| (pid, Process { parent, program: program.to_owned() })).collect()
    }

    fn desktop() -> HashMap<u32, Process> {
        table(&[
            (SHELL, 1, "explorer.exe"),
            (OURS, SHELL, "chatkord.exe"),
            (OUR_CHILD, OURS, "player.exe"),
            (BRAVE, SHELL, "brave.exe"),
            (BRAVE_AUDIO, BRAVE, "brave.exe"),
            (PLAYER, BRAVE, "Player.exe"),
        ])
    }

    fn app(name: &str, root: u32) -> App {
        App { name: name.to_owned(), root }
    }

    #[test]
    fn a_process_of_its_parents_program_is_heard_as_that_parent() {
        assert_eq!(group([BRAVE_AUDIO], &desktop(), OURS), [app("brave", BRAVE)]);
    }

    #[test]
    fn a_child_of_another_program_is_an_application_of_its_own() {
        assert_eq!(group([PLAYER], &desktop(), OURS), [app("Player", PLAYER)]);
    }

    #[test]
    fn this_process_its_children_and_a_tree_that_holds_it_are_never_offered() {
        assert!(group([OURS, OUR_CHILD], &desktop(), OURS).is_empty(), "this process's tree");
        assert!(group([SHELL], &desktop(), OURS).is_empty(), "a tree this process is in");
    }

    #[test]
    fn an_application_with_several_sessions_is_listed_once_in_name_order() {
        let listed = group([PLAYER, BRAVE_AUDIO, BRAVE, BRAVE_AUDIO], &desktop(), OURS);
        assert_eq!(listed, [app("Player", PLAYER), app("brave", BRAVE)], "byte order, as Linux's list");
    }

    #[test]
    fn a_process_the_snapshot_missed_is_not_offered() {
        assert!(group([999], &desktop(), OURS).is_empty());
    }

    #[test]
    fn parents_that_loop_through_reused_ids_end() {
        let looped = table(&[(700, 701, "game.exe"), (701, 700, "game.exe")]);
        let listed = group([700], &looped, OURS);
        assert_eq!(listed.len(), 1, "one application, its root found within the bound: {listed:?}");
    }

    #[test]
    fn a_tree_that_holds_this_process_is_never_heard_and_a_sibling_is() {
        let table = desktop();
        assert!(holds(OURS, &table, OURS), "this process's own tree");
        assert!(holds(SHELL, &table, OURS), "the shell that launched it");
        assert!(!holds(BRAVE, &table, OURS), "a sibling of it");
        assert!(!holds(OUR_CHILD, &table, OURS), "a child of it is below, not above");
        assert!(!holds(999, &table, OURS), "a process the snapshot missed");
    }

    #[test]
    fn a_program_answers_to_its_name_with_or_without_exe_in_any_case() {
        let brave = app("brave", BRAVE);
        for typed in ["brave", "Brave", "brave.exe", "BRAVE.Exe"] {
            assert!(brave.answers_to(typed), "{typed}");
        }
        assert!(!brave.answers_to("brav"));
        assert!(!brave.answers_to("brave.exe.exe"));
    }
}
