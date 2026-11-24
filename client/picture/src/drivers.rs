//! Which Vulkan drivers a start loads: listing the adapters loads every driver, and a driver that opens a GPU the
//! kernel put to sleep wakes it, its own or one it only looks at for its hardware. While a GPU sleeps the loader is
//! told, in the environment before any thread starts, to load only the drivers of awake GPUs, so the window and the
//! decoders' own Vulkan devices alike open no other; what serves which GPU is learned by a child process, so that this
//! one never lists to learn.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
use elsewhere as here;
#[cfg(target_os = "linux")]
use linux as here;

/// What a start did about the Vulkan drivers of sleeping GPUs, for the log once there is one ([`Drivers::note`]).
#[must_use]
#[derive(Debug)]
pub struct Drivers(here::Outcome);

impl Drivers {
    /// Logs what was done: drivers selected, or a learning that failed. A learning still running is watched from here
    /// on, beside the window, and stopped when it takes too long.
    pub fn note(self) {
        self.0.note();
    }
}

/// While a GPU sleeps, loads only the Vulkan drivers learned to serve an awake one, deciding from sysfs and a file in
/// `state` alone, with no Vulkan call: Mesa's drivers for other hardware open every render node looking for theirs (on
/// Machine B intel's and freedreno's each woke the NVIDIA, 1.6 s). A driver is known to serve a GPU from a learning that
/// ran while none was asleep; while an awake GPU has no learned driver, or every GPU sleeps, every driver loads. What
/// is not known yet is learned by this program started beside this process in a hidden mode ([`run_hidden`]), after
/// the environment is set and not waited for.
///
/// Off Linux there is no such loader and nothing happens.
///
/// # Safety
///
/// It sets and removes environment variables of the process: no other thread may exist. Call it as the first
/// statement of `main`, after [`run_hidden`] only.
pub unsafe fn leave_out_sleeping(state: &Path) -> Drivers {
    // SAFETY: the caller promised that no other thread exists.
    Drivers(unsafe { here::leave_out_sleeping(state) })
}

/// Runs this process as the learner, or one of its listers, that [`leave_out_sleeping`] starts, when `args` (without
/// the program's name) are one's, and gives its exit code; any other arguments are none of its business.
pub fn run_hidden(args: &[OsString]) -> Option<ExitCode> {
    here::run_hidden(args)
}

#[cfg(not(target_os = "linux"))]
mod elsewhere {
    use std::ffi::OsString;
    use std::path::Path;
    use std::process::ExitCode;

    /// Nothing done.
    #[derive(Debug)]
    pub(super) struct Outcome;

    impl Outcome {
        pub(super) fn note(self) {}
    }

    /// # Safety
    ///
    /// None needed here; unsafe as the Linux one is, so that one caller fits both.
    pub(super) unsafe fn leave_out_sleeping(_: &Path) -> Outcome {
        Outcome
    }

    pub(super) fn run_hidden(_: &[OsString]) -> Option<ExitCode> {
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use crate::dmabuf::{Asleep, present_nodes, render_node};
    use crate::vulkan::Listing;
    use media::startup::{DRM, is_render_node, keep, single_threaded, within};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{OsStr, OsString};
    use std::fmt;
    use std::io::{self, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    /// The file in the state folder that keeps what was learned.
    const FILE: &str = "vulkan-drivers.txt";
    const HEADER: &str = "chatkord-vulkan-drivers 1";
    /// The loader's lists of manifest file names to load only, and to leave out.
    const SELECT: &str = "VK_LOADER_DRIVERS_SELECT";
    const DISABLE: &str = "VK_LOADER_DRIVERS_DISABLE";
    /// Variables with which a person chose the drivers themselves: then nothing here chooses.
    const CHOSEN: [&str; 4] = ["VK_DRIVER_FILES", "VK_ICD_FILENAMES", SELECT, DISABLE];
    /// The list a start of this program selected, set beside the loader's: a program it starts inherits both, and one
    /// of this program decides again where a person would have chosen.
    pub(super) const MARKER: &str = "CHATKORD_VULKAN_DRIVERS_SELECTED";
    /// The hidden arguments of the learner, which is given the file to keep in, and of a lister.
    pub(super) const LEARN: &str = "--learn-vulkan-drivers";
    pub(super) const LIST: &str = "--list-vulkan-nodes";
    /// How long the learner lists in all, and one lister at most; past them what is left is learned at a later start.
    const LEARN_TIME: Duration = Duration::from_secs(30);
    const LIST_TIME: Duration = Duration::from_secs(10);
    /// How long the window waits for the learner before it is stopped: its own bound and time to start and keep.
    const WATCHED: Duration = LEARN_TIME.saturating_add(Duration::from_secs(5));
    /// The most a lister says: one line for each of a few GPUs.
    pub(super) const SAID: u16 = 4096;

    /// The environment as a lookup, so that a test gives its own.
    type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

    /// What a start did about the drivers.
    #[derive(Debug)]
    pub(super) enum Outcome {
        /// Every driver loads.
        Loaded,
        /// Only this many drivers load, those of the awake GPUs, as this start or the one of this program that started
        /// it decided.
        Selected(usize),
        /// The drivers not known yet are being learned in this child.
        Learning(Child),
        /// The learner could not start.
        Unlearned,
        /// Drivers were to be selected or all loaded again, but this process was not proven to have one thread, so the
        /// environment was left as it was.
        NotAlone,
    }

    impl Outcome {
        pub(super) fn note(self) {
            match self {
                Self::Loaded => {}
                Self::Selected(drivers) => {
                    report::note!(
                        info,
                        "a GPU sleeps, so only the Vulkan drivers of awake ones load",
                        drivers = drivers
                    );
                }
                Self::Learning(learner) => watch(learner),
                Self::Unlearned => {
                    report::note!(warn, "learning which Vulkan driver serves which GPU was skipped: it did not start");
                }
                Self::NotAlone => {
                    report::note!(warn, "a thread had already started, so a sleeping GPU's drivers were not left out");
                }
            }
        }
    }

    /// Reaps `learner` on a thread of its own, logging an end that is not a success and stopping it past its bound.
    fn watch(mut learner: Child) {
        let watching = std::thread::Builder::new().name("vulkan-drivers".into()).spawn(move || {
            match within(&mut learner, WATCHED) {
                Ok(Some(status)) if status.success() => {}
                Ok(Some(_)) => {
                    report::note!(warn, "not every Vulkan driver was learned; the rest are at a later start")
                }
                Ok(None) => report::note!(warn, "learning which Vulkan driver serves which GPU took too long: stopped"),
                Err(_) => report::note!(warn, "learning which Vulkan driver serves which GPU could not be watched"),
            }
        });
        if watching.is_err() {
            report::note!(warn, "learning which Vulkan driver serves which GPU could not be watched");
        }
    }

    /// # Safety
    ///
    /// No other thread may exist: it sets and removes environment variables of the process.
    pub(super) unsafe fn leave_out_sleeping(state: &Path) -> Outcome {
        let environment = |name: &str| std::env::var_os(name);
        let file = state.join(FILE);
        let world = World::read(&search_paths(&environment), DRM, &file);
        let selected = |list: &OsStr| Outcome::Selected(list.to_string_lossy().split(',').count());
        match decision(&environment, &world) {
            Decision::Keep => match environment(SELECT) {
                Some(list) if inherited(&environment) => selected(list.as_os_str()),
                _ => Outcome::Loaded,
            },
            Decision::Set(_) if !single_threaded() => Outcome::NotAlone,
            Decision::Set(Some(list)) => {
                // SAFETY: the caller promised that no other thread exists to read or write the environment meanwhile.
                unsafe { std::env::set_var(SELECT, &list) };
                // SAFETY: as above.
                unsafe { std::env::set_var(MARKER, &list) };
                selected(OsStr::new(&list))
            }
            Decision::Set(None) => {
                // SAFETY: the caller promised that no other thread exists to read or write the environment meanwhile.
                unsafe { std::env::remove_var(SELECT) };
                // SAFETY: as above.
                unsafe { std::env::remove_var(MARKER) };
                Outcome::Loaded
            }
            Decision::Learn => start_learner(&file).map_or(Outcome::Unlearned, Outcome::Learning),
        }
    }

    /// The learner, started beside this process and not waited for, keeping what it learns in `file`.
    fn start_learner(file: &Path) -> io::Result<Child> {
        Command::new(std::env::current_exe()?)
            .arg(LEARN)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    pub(super) fn run_hidden(args: &[OsString]) -> Option<ExitCode> {
        match args {
            [mode, file] if mode == LEARN => Some(learner(Path::new(file))),
            [mode] if mode == LIST => Some(lister()),
            _ => None,
        }
    }

    /// The learner: each driver not known yet is listed in a lister of its own, which is the only way the loader
    /// takes one driver alone without the environment of a process with threads changing.
    fn learner(file: &Path) -> ExitCode {
        let Ok(program) = std::env::current_exe() else { return ExitCode::FAILURE };
        let started = Instant::now();
        let folders = search_paths(&|name| std::env::var_os(name));
        let learned = learn(&folders, DRM, file, |manifest| {
            let left = LEARN_TIME.checked_sub(started.elapsed())?;
            listed(&program, manifest, left.min(LIST_TIME))
        });
        match learned {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }

    /// A lister: the render nodes of the adapters the loader lists, one a line, with the drivers it was started with.
    fn lister() -> ExitCode {
        let listing = Listing::of_vulkan();
        let served: BTreeSet<String> = listing.iter().filter_map(|adapter| render_node(&adapter.get_info())).collect();
        drop(listing);
        let mut out = io::stdout().lock();
        match served.iter().try_for_each(|node| writeln!(out, "{node}")).and_then(|()| out.flush()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }

    /// The render nodes `manifest`'s driver serves, from a lister given that driver alone and at most `bound`; none
    /// when it fails, says too much or takes longer.
    fn listed(program: &Path, manifest: &str, bound: Duration) -> Option<BTreeSet<String>> {
        let mut lister = Command::new(program)
            .arg(LIST)
            .env(SELECT, manifest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let status = within(&mut lister, bound).ok()??;
        if !status.success() {
            return None;
        }
        let mut said = String::new();
        lister.stdout.take()?.take(u64::from(SAID) + 1).read_to_string(&mut said).ok()?;
        served(&said)
    }

    /// A lister's words read: render nodes, one a line; anything else, or more than a lister says, refuses them all.
    pub(super) fn served(said: &str) -> Option<BTreeSet<String>> {
        if said.len() > usize::from(SAID) {
            return None;
        }
        said.lines().map(|line| is_render_node(line).then(|| line.to_string())).collect()
    }

    /// Whether the loader's list in `environment` is the one a start of this program selected, not a person's.
    fn inherited(environment: Environment<'_>) -> bool {
        environment(SELECT).is_some_and(|list| environment(MARKER).is_some_and(|marker| marker == list))
    }

    /// What a start does about the drivers.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum Decision {
        /// Go on as started.
        Keep,
        /// Tell the loader to load these manifests only, or every one.
        Set(Option<String>),
        /// Go on, and learn the drivers not known yet beside it.
        Learn,
    }

    /// What a start with `environment` does on `world`. A person's choice of drivers stands; a list inherited from a
    /// start of this program is decided again.
    pub(super) fn decision(environment: Environment<'_>, world: &World) -> Decision {
        let inherited = inherited(environment);
        if CHOSEN.iter().any(|name| environment(name).is_some() && !(inherited && *name == SELECT)) {
            return Decision::Keep;
        }
        let current = environment(SELECT).filter(|_| inherited);
        let wanted = world.selecting();
        if current.as_deref() != wanted.as_deref().map(OsStr::new) {
            Decision::Set(wanted)
        } else if world.unknown() {
            Decision::Learn
        } else {
            Decision::Keep
        }
    }

    /// The folders the loader reads driver manifests from, in its order, as `environment` sets the places they hang
    /// off.
    pub(super) fn search_paths(environment: Environment<'_>) -> Vec<PathBuf> {
        let absolute = |name: &str| environment(name).map(PathBuf::from).filter(|path| path.is_absolute());
        let list = |name: &str, default: &str| -> Vec<PathBuf> {
            let value = environment(name).filter(|value| !value.is_empty()).unwrap_or_else(|| default.into());
            std::env::split_paths(&value).filter(|path| path.is_absolute()).collect()
        };
        let home = absolute("HOME");
        let under = |inside: &str| home.as_ref().map(|home| home.join(inside));
        let mut found: Vec<PathBuf> = Vec::new();
        found.extend(absolute("XDG_CONFIG_HOME").or_else(|| under(".config")));
        found.extend(list("XDG_CONFIG_DIRS", "/etc/xdg"));
        found.push("/etc".into());
        found.extend(absolute("XDG_DATA_HOME").or_else(|| under(".local/share")));
        found.extend(list("XDG_DATA_DIRS", "/usr/local/share:/usr/share"));
        found.extend([PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
        let mut folders: Vec<PathBuf> = Vec::new();
        for base in found {
            let folder = base.join("vulkan/icd.d");
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }

    /// Whether the loader's list of file names can name `manifest` and no other: no separator, wildcard or blank.
    fn nameable(manifest: &str) -> bool {
        manifest.ends_with(".json") && !manifest.contains([',', '*', '?', '[', ' ', '\t', '\n'])
    }

    /// The file names of the driver manifests in `folders`.
    fn manifests(folders: &[PathBuf]) -> BTreeSet<String> {
        folders
            .iter()
            .filter_map(|folder| std::fs::read_dir(folder).ok())
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| nameable(name))
            .collect()
    }

    /// What a start decides on: the driver manifests, which GPUs sleep, and what was learned on these GPUs.
    #[derive(Debug)]
    pub(super) struct World {
        manifests: BTreeSet<String>,
        asleep: Asleep,
        learned: Learned,
    }

    impl World {
        /// The manifests in `folders`, the GPUs under `sysfs`, and what `file` keeps, thrown away when the machine's
        /// GPUs are not the ones it was learned on.
        pub(super) fn read(folders: &[PathBuf], sysfs: &str, file: &Path) -> Self {
            let present = present_nodes(sysfs);
            let kept = std::fs::read_to_string(file).ok().and_then(|text| Learned::parse(&text).ok());
            let mut learned = kept.filter(|kept| kept.nodes == present).unwrap_or_else(|| Learned::on(present));
            let manifests = manifests(folders);
            learned.drivers.retain(|name, _| manifests.contains(name));
            Self { manifests, asleep: Asleep::read(sysfs), learned }
        }

        /// The loader's list of the only manifests to load while a GPU sleeps: those serving an awake one. A driver
        /// that serves no GPU here may still open every render node to look for its own, waking a sleeping one.
        pub(super) fn selecting(&self) -> Option<String> {
            self.learned.selecting(&self.manifests, &self.asleep)
        }

        /// Whether a driver is not known yet and can be learned now, while no GPU sleeps that listing would wake.
        fn unknown(&self) -> bool {
            !self.asleep.any() && self.manifests.iter().any(|name| !self.learned.drivers.contains_key(name))
        }
    }

    /// Why a learner ended without every driver learned and kept.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Unlearned {
        /// A driver's lister failed or ran out of time, or a GPU fell asleep: the rest are learned at a later start.
        Partly,
        /// What was learned could not be kept.
        Unkept,
    }

    /// Learns the drivers not known yet with `list`, which gives the render nodes a manifest's driver serves or none
    /// when it could not, and keeps what it learned in `file`. A GPU under `sysfs` that falls asleep stops it.
    pub(super) fn learn(
        folders: &[PathBuf],
        sysfs: &str,
        file: &Path,
        mut list: impl FnMut(&str) -> Option<BTreeSet<String>>,
    ) -> Result<(), Unlearned> {
        let World { manifests, mut learned, .. } = World::read(folders, sysfs, file);
        let (mut whole, mut changed) = (true, false);
        for name in manifests.iter().filter(|name| !learned.drivers.contains_key(*name)).collect::<Vec<_>>() {
            if Asleep::read(sysfs).any() {
                whole = false;
                break;
            }
            match list(name) {
                Some(served) => {
                    learned.drivers.insert(name.clone(), served);
                    changed = true;
                }
                None => whole = false,
            }
        }
        if changed {
            keep(file, &learned.text()).map_err(|_| Unlearned::Unkept)?;
        }
        if whole { Ok(()) } else { Err(Unlearned::Partly) }
    }

    /// What was learned: the render nodes the machine had, and for each driver manifest the ones it serves.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Learned {
        nodes: Vec<String>,
        drivers: BTreeMap<String, BTreeSet<String>>,
    }

    /// A line of a learned file that is not what the format says.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Malformed {
        pub(super) line: usize,
    }

    impl fmt::Display for Malformed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "line {} of the learned drivers is not one the format has", self.line)
        }
    }

    impl std::error::Error for Malformed {}

    impl Learned {
        fn on(nodes: Vec<String>) -> Self {
            Self { nodes, drivers: BTreeMap::new() }
        }

        /// The file's text, in the order it is kept.
        pub(super) fn text(&self) -> String {
            let mut text = format!("{HEADER}\nnodes");
            for node in &self.nodes {
                text.push(' ');
                text.push_str(node);
            }
            for (name, served) in &self.drivers {
                text.push_str("\ndriver ");
                text.push_str(name);
                for node in served {
                    text.push(' ');
                    text.push_str(node);
                }
            }
            text.push('\n');
            text
        }

        /// The learned file's text read back; any line the format does not have, or a driver told twice, refuses all.
        pub(super) fn parse(text: &str) -> Result<Self, Malformed> {
            let mut lines = text.lines().enumerate().map(|(at, line)| (at + 1, line));
            if lines.next().map(|(_, line)| line) != Some(HEADER) {
                return Err(Malformed { line: 1 });
            }
            let mut learned = None::<Self>;
            for (line, text) in lines {
                let bad = Malformed { line };
                let mut words = text.split(' ');
                match (words.next(), learned.as_mut()) {
                    (Some("nodes"), None) => {
                        let nodes: Vec<String> = words.map(str::to_string).collect();
                        if !nodes.iter().all(|token| is_render_node(token)) || !nodes.is_sorted() {
                            return Err(bad);
                        }
                        learned = Some(Self::on(nodes));
                    }
                    (Some("driver"), Some(learned)) => {
                        let name = words.next().filter(|name| nameable(name)).ok_or(bad)?;
                        let served: BTreeSet<String> = words.map(str::to_string).collect();
                        if !served.iter().all(|token| is_render_node(token))
                            || learned.drivers.insert(name.to_string(), served).is_some()
                        {
                            return Err(bad);
                        }
                    }
                    _ => return Err(bad),
                }
            }
            learned.ok_or(Malformed { line: 2 })
        }

        /// The manifests of `manifests` that serve an awake GPU, as the loader's list of the only names to load: none
        /// while no GPU sleeps, none while every one does, and none while an awake GPU has no learned driver, so the
        /// GPU that paints never loses its own.
        fn selecting(&self, manifests: &BTreeSet<String>, asleep: &Asleep) -> Option<String> {
            let awake: Vec<&String> = self.nodes.iter().filter(|node| !asleep.holds(node)).collect();
            if awake.len() == self.nodes.len() || awake.is_empty() {
                return None;
            }
            let serves_awake = |served: &BTreeSet<String>| awake.iter().any(|node| served.contains(*node));
            let chosen: Vec<&str> = manifests
                .iter()
                .filter(|name| self.drivers.get(*name).is_some_and(serves_awake))
                .map(String::as_str)
                .collect();
            let all_served = awake.iter().all(|node| {
                chosen.iter().any(|name| self.drivers.get(*name).is_some_and(|served| served.contains(*node)))
            });
            all_served.then(|| chosen.join(","))
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[expect(clippy::unwrap_used, reason = "a test's fixture: a step that fails is the test failing")]
mod tests {
    use super::linux::{
        Decision, LEARN, LIST, Learned, MARKER, Malformed, SAID, Unlearned, World, decision, learn, search_paths,
        served,
    };
    use super::run_hidden;
    use media::startup::keep;
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    const AMD: &str = "radeon_icd.x86_64.json";
    const NOUVEAU: &str = "nouveau_icd.x86_64.json";
    const SOFTWARE: &str = "lvp_icd.x86_64.json";
    const BOTH: &str = "both_icd.x86_64.json";
    const AWAKE_NODE: &str = "/dev/dri/renderD129";
    const ASLEEP_NODE: &str = "/dev/dri/renderD128";
    const HEADER: &str = "chatkord-vulkan-drivers 1";
    const SELECT: &str = "VK_LOADER_DRIVERS_SELECT";
    /// The drivers serving the awake GPU, alone or beside the sleeper: never the sleeper's, nor the rasteriser's.
    const SELECTED: &str = "both_icd.x86_64.json,radeon_icd.x86_64.json";

    /// A folder of manifests and a stand-in for `/sys/class/drm`, taken away when dropped.
    struct Machine(PathBuf);

    impl Machine {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("picture-drivers-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("icd.d")).unwrap();
            for manifest in [AMD, NOUVEAU, SOFTWARE, BOTH, "notes.txt"] {
                std::fs::write(root.join("icd.d").join(manifest), "{}").unwrap();
            }
            let machine = Self(root);
            machine.node("renderD128", "suspended");
            machine.node("renderD129", "active");
            machine
        }

        fn node(&self, node: &str, status: &str) {
            let power = self.0.join("drm").join(node).join("device/power");
            std::fs::create_dir_all(&power).unwrap();
            std::fs::write(power.join("runtime_status"), status).unwrap();
        }

        fn folders(&self) -> Vec<PathBuf> {
            vec![self.0.join("icd.d")]
        }

        fn sysfs(&self) -> String {
            self.0.join("drm").to_str().unwrap().to_string()
        }

        fn file(&self) -> PathBuf {
            self.0.join("state/vulkan-drivers.txt")
        }

        fn world(&self) -> World {
            World::read(&self.folders(), &self.sysfs(), &self.file())
        }

        /// What a learner learns with `list` standing in for its listers.
        fn learn(&self, list: impl FnMut(&str) -> Option<BTreeSet<String>>) -> Result<(), Unlearned> {
            learn(&self.folders(), &self.sysfs(), &self.file(), list)
        }

        fn selecting(&self) -> Option<String> {
            self.world().selecting()
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn serving(manifest: &str) -> Option<BTreeSet<String>> {
        let nodes: &[&str] = match manifest {
            AMD => &[AWAKE_NODE],
            NOUVEAU => &[ASLEEP_NODE],
            BOTH => &[ASLEEP_NODE, AWAKE_NODE],
            _ => &[],
        };
        Some(nodes.iter().map(ToString::to_string).collect())
    }

    fn environment(set: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let set: Vec<(String, String)> = set.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        move |name: &str| set.iter().find(|(key, _)| key == name).map(|(_, value)| OsString::from(value))
    }

    #[test]
    fn nothing_is_learned_while_a_gpu_sleeps_so_nothing_is_woken_and_nothing_selected() {
        let machine = Machine::new("asleep");
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Keep, "no learner is started");
        let learned = machine.learn(|manifest| panic!("learning {manifest} would list while a GPU sleeps"));
        assert_eq!(learned, Err(Unlearned::Partly));
        assert_eq!(machine.selecting(), None);
        assert!(!machine.file().exists());
    }

    #[test]
    fn while_a_gpu_sleeps_only_the_learned_drivers_of_awake_ones_load() {
        let machine = Machine::new("learned");
        machine.node("renderD128", "active");
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Learn);
        let mut asked = Vec::new();
        let learned = machine.learn(|manifest| {
            asked.push(manifest.to_string());
            serving(manifest)
        });
        assert_eq!(learned, Ok(()));
        assert_eq!(machine.selecting(), None, "all awake: all loaded");
        assert_eq!(asked, [BOTH, SOFTWARE, NOUVEAU, AMD], "each manifest once, and notes.txt is no manifest");
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Keep, "all known");

        machine.node("renderD128", "suspended");
        assert_eq!(machine.learn(|manifest| panic!("{manifest} was learned already")), Ok(()));
        assert_eq!(
            machine.selecting().as_deref(),
            Some(SELECTED),
            "a driver serving no GPU may open the sleeper looking for its own, so it does not load either"
        );
        machine.node("renderD129", "suspended");
        assert_eq!(machine.selecting(), None, "every GPU asleep: one must wake to paint, so all load");
    }

    #[test]
    fn what_was_learned_on_other_gpus_is_thrown_away_and_a_new_driver_is_learned_alone() {
        let machine = Machine::new("changed");
        machine.node("renderD128", "active");
        machine.learn(serving).unwrap();
        machine.node("renderD130", "active");
        let mut asked = Vec::new();
        machine
            .learn(|manifest| {
                asked.push(manifest.to_string());
                serving(manifest)
            })
            .unwrap();
        assert_eq!(asked.len(), 4, "a machine with another GPU is learned again");
        std::fs::write(machine.folders()[0].join("new_icd.json"), "{}").unwrap();
        let mut asked = Vec::new();
        machine
            .learn(|manifest| {
                asked.push(manifest.to_string());
                serving(manifest)
            })
            .unwrap();
        assert_eq!(asked, ["new_icd.json"]);
    }

    #[test]
    fn a_driver_whose_lister_failed_stays_unknown_and_is_asked_again_later() {
        let machine = Machine::new("failed");
        machine.node("renderD128", "active");
        let learned = machine.learn(|manifest| if manifest == NOUVEAU { None } else { serving(manifest) });
        assert_eq!(learned, Err(Unlearned::Partly));
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Learn);
        let mut asked = Vec::new();
        machine
            .learn(|manifest| {
                asked.push(manifest.to_string());
                serving(manifest)
            })
            .unwrap();
        assert_eq!(asked, [NOUVEAU]);
    }

    #[test]
    fn a_learned_file_reads_back_and_a_malformed_one_is_refused() {
        let learned = Learned::parse(&format!(
            "{HEADER}\nnodes {ASLEEP_NODE} {AWAKE_NODE}\ndriver {AMD} {AWAKE_NODE}\ndriver {SOFTWARE}\n"
        ))
        .unwrap();
        assert_eq!(Learned::parse(&learned.text()), Ok(learned));
        let refused = |text: &str, line| assert_eq!(Learned::parse(text), Err(Malformed { line }), "{text:?}");
        refused("", 1);
        refused("chatkord-vulkan-drivers 2\nnodes\n", 1);
        refused(&format!("{HEADER}\n"), 2);
        refused(&format!("{HEADER}\ndriver a.json\n"), 2);
        refused(&format!("{HEADER}\nnodes /dev/dri/card0\n"), 2);
        refused(&format!("{HEADER}\nnodes {AWAKE_NODE} {ASLEEP_NODE}\n"), 2);
        refused(&format!("{HEADER}\nnodes\ndriver a.json /dev/null\n"), 3);
        refused(&format!("{HEADER}\nnodes\ndriver a.json\ndriver a.json\n"), 4);
        refused(&format!("{HEADER}\nnodes\ndriver *.json\n"), 3);
        refused(&format!("{HEADER}\nnodes\nsomething else\n"), 3);
        refused(&format!("{HEADER}\nnodes\nnodes\n"), 3);
    }

    #[test]
    fn a_kept_file_is_whole_and_reads_back_as_it_was_learned() {
        let machine = Machine::new("kept");
        let text = format!("{HEADER}\nnodes {ASLEEP_NODE} {AWAKE_NODE}\ndriver {AMD} {AWAKE_NODE}\n");
        keep(&machine.file(), "an earlier file").unwrap();
        keep(&machine.file(), &text).unwrap();
        assert_eq!(std::fs::read_to_string(machine.file()).unwrap(), text);
        let beside: Vec<OsString> = std::fs::read_dir(machine.file().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(beside, [OsString::from("vulkan-drivers.txt")], "nothing half-written is left beside it");
        assert_eq!(machine.selecting().as_deref(), Some(AMD), "a start reads what a learner kept");
    }

    #[test]
    fn an_awake_gpu_with_no_learned_driver_keeps_every_driver() {
        let machine = Machine::new("unlearned");
        let text = format!("{HEADER}\nnodes {ASLEEP_NODE} {AWAKE_NODE}\ndriver {NOUVEAU} {ASLEEP_NODE}\n");
        keep(&machine.file(), &text).unwrap();
        assert_eq!(machine.selecting(), None, "selecting without the painter's own driver would leave it none");
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Keep, "nor is it learned while one sleeps");
    }

    #[test]
    fn a_file_that_cannot_be_read_is_learned_again_and_the_start_is_not_stopped() {
        let machine = Machine::new("broken");
        machine.node("renderD128", "active");
        keep(&machine.file(), "not a learned file").unwrap();
        assert_eq!(decision(&environment(&[]), &machine.world()), Decision::Learn);
        assert_eq!(machine.learn(serving), Ok(()));
        assert!(Learned::parse(&std::fs::read_to_string(machine.file()).unwrap()).is_ok());
    }

    #[test]
    fn the_awake_ones_are_selected_and_a_person_s_choice_stands() {
        let machine = Machine::new("decided");
        machine.node("renderD128", "active");
        machine.learn(serving).unwrap();
        machine.node("renderD128", "suspended");
        let world = machine.world();
        let decided = |set: &[(&str, &str)]| decision(&environment(set), &world);

        assert_eq!(decided(&[]), Decision::Set(Some(SELECTED.into())));
        for chosen in ["VK_DRIVER_FILES", "VK_ICD_FILENAMES", SELECT, "VK_LOADER_DRIVERS_DISABLE"] {
            assert_eq!(decided(&[(chosen, AMD)]), Decision::Keep, "{chosen} is a person's choice");
        }
        assert_eq!(
            decided(&[(MARKER, AMD), (SELECT, SELECTED)]),
            Decision::Keep,
            "a list that is not the one this program selected is a person's"
        );
        assert_eq!(
            decided(&[(MARKER, SELECTED), (SELECT, SELECTED)]),
            Decision::Keep,
            "a list inherited from a start that decided alike stands"
        );
        assert_eq!(
            decided(&[(MARKER, AMD), (SELECT, AMD)]),
            Decision::Set(Some(SELECTED.into())),
            "a list inherited from a start that decided otherwise is decided again"
        );
        assert_eq!(
            decided(&[(MARKER, AMD), ("VK_DRIVER_FILES", AMD), (SELECT, AMD)]),
            Decision::Keep,
            "beside an inherited list a person's choice still stands"
        );

        machine.node("renderD128", "active");
        let awake = machine.world();
        assert_eq!(
            decision(&environment(&[(MARKER, SELECTED), (SELECT, SELECTED)]), &awake),
            Decision::Set(None),
            "an inherited list whose GPU woke is taken away"
        );
        assert_eq!(decision(&environment(&[]), &awake), Decision::Keep);
    }

    #[test]
    fn a_lister_says_render_nodes_and_nothing_else() {
        let nodes = |set: &[&str]| Some(set.iter().map(ToString::to_string).collect::<BTreeSet<String>>());
        assert_eq!(served(""), nodes(&[]));
        assert_eq!(served(&format!("{AWAKE_NODE}\n{ASLEEP_NODE}\n")), nodes(&[ASLEEP_NODE, AWAKE_NODE]));
        assert_eq!(served("/dev/dri/card0\n"), None);
        assert_eq!(served(&format!("{AWAKE_NODE}\nsomething else\n")), None);
        let long = format!("{AWAKE_NODE}\n").repeat(usize::from(SAID) / AWAKE_NODE.len() + 1);
        assert_eq!(served(&long), None, "more than a lister says");
    }

    #[test]
    fn only_the_hidden_arguments_run_hidden() {
        let args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        for words in [&[][..], &["--forget"], &[LEARN], &[LIST, "more"], &["register", "example.invalid", "ada"]] {
            assert_eq!(run_hidden(&args(words)), None, "{words:?}");
        }
    }

    #[test]
    fn the_loaders_folders_come_from_the_places_it_documents() {
        let folder = |base: &str| Path::new(base).join("vulkan/icd.d");
        assert_eq!(
            search_paths(&environment(&[("HOME", "/srv/user")])),
            [
                folder("/srv/user/.config"),
                folder("/etc/xdg"),
                folder("/etc"),
                folder("/srv/user/.local/share"),
                folder("/usr/local/share"),
                folder("/usr/share"),
            ]
        );
        let set = search_paths(&environment(&[
            ("HOME", "/srv/user"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "relative/is/ignored"),
            ("XDG_DATA_DIRS", "/opt/share:/usr/share"),
        ]));
        assert_eq!(set[0], folder("/cfg"));
        assert!(set.contains(&folder("/opt/share")) && !set.contains(&folder("relative/is/ignored")));
        assert_eq!(set.iter().filter(|path| **path == folder("/usr/share")).count(), 1, "a folder once");
    }
}
