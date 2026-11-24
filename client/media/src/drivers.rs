//! Which VA-API driver libva tries first: a start names its carried driver before the system's, and where that
//! driver does not open an awake AMD GPU, the system's goes first. Whether it opens is learned by a child
//! process of this program, started in a hidden mode with the carried folder alone, so that a driver that fails or
//! crashes costs the child and never this process; the verdict is kept per carried driver and per GPU, so a later
//! start asks nobody.

#[cfg(target_os = "linux")]
use crate::probe::PciVendor;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
use elsewhere as here;
#[cfg(target_os = "linux")]
use linux as here;

/// What a start did about the order of VA-API drivers, for the log once there is one ([`Drivers::note`]).
#[must_use]
#[derive(Debug)]
pub struct Drivers(here::Outcome);

impl Drivers {
    /// Logs what was done: the carried driver moved after the system's, or a question that could not be asked.
    pub fn note(self) {
        self.0.note();
    }
}

/// Names libva's driver folders in `LIBVA_DRIVERS_PATH` where the person named none: the carried folder (the tree's
/// for a build in the tree, else the one beside the program), then the distributions'. Where libva's path starts
/// with the carried folder and the carried driver does not open an awake AMD GPU, puts the others before it. What
/// is not known yet is asked of a child of this program ([`run_hidden`]) before this returns, once for each GPU and
/// carried driver, and kept in a file in `state`; with no `state` nobody is asked. A GPU that sleeps is not asked,
/// so not woken. `LIBVA_DRIVER_NAME` is never touched.
///
/// Off Linux there is no such path and nothing happens.
///
/// # Safety
///
/// It sets an environment variable of the process: no other thread may exist. Call it as the first statement of
/// `main`, after [`run_hidden`] only.
pub unsafe fn order_va_drivers(state: Option<&Path>) -> Drivers {
    // SAFETY: the caller promised that no other thread exists.
    Drivers(unsafe { here::order_va_drivers(state) })
}

/// Runs this process as the child [`order_va_drivers`] starts, when `args` (without the program's name)
/// are its, and gives its exit code: success when VA-API opens on the node it names; any other arguments are none of
/// its business.
pub fn run_hidden(args: &[OsString]) -> Option<ExitCode> {
    here::run_hidden(args)
}

/// The PCI vendor and device ids of the GPU whose render node the kernel lists at `listed` (an entry of
/// [`crate::startup::render_nodes`]); none where they cannot be read.
#[cfg(target_os = "linux")]
pub fn pci_ids(listed: &Path) -> Option<(PciVendor, u16)> {
    linux::Gpu::of(listed).map(|gpu| (gpu.vendor, gpu.device))
}

/// The vendor of the GPU whose render node is `node`, as `/dev/dri/renderD128`.
#[cfg(target_os = "linux")]
pub(crate) fn node_vendor(node: &str) -> Option<PciVendor> {
    let name = Path::new(node).file_name()?;
    pci_ids(&Path::new(crate::startup::DRM).join(name)).map(|(vendor, _)| vendor)
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
    pub(super) unsafe fn order_va_drivers(_: Option<&Path>) -> Outcome {
        Outcome
    }

    pub(super) fn run_hidden(_: &[OsString]) -> Option<ExitCode> {
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use crate::av::HwDevice;
    use crate::probe::{Backend, PciVendor};
    use crate::startup::{DRM, asleep, is_render_node, keep, node_path, render_nodes, single_threaded, within};
    use capture::Validation;
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::fmt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode, Stdio};
    use std::time::{Duration, SystemTime};

    /// The file in the state folder that keeps what was learned.
    const FILE: &str = "libva-drivers.txt";
    const HEADER: &str = "chatkord-libva-drivers 1";
    /// The variable libva reads its driver folders from, in order.
    const PATH: &str = "LIBVA_DRIVERS_PATH";
    /// The carried driver's file, and the folder of the carried drivers beside a package's program.
    const DRIVER: &str = "radeonsi_drv_video.so";
    const CARRIED: &str = "dri";
    /// The distributions' driver folders, searched after the carried one in the order the file gives.
    const SYSTEMS: &str = include_str!("../../../scripts/package/va-system-drivers.txt");
    /// A tree build's carried folder, which the tree's cargo config names to libva for all it runs and so to the
    /// build; none in a package's build (`CHATKORD_PACKAGE`), whose folder is beside its program.
    const TREE: Option<&str> = match option_env!("CHATKORD_PACKAGE") {
        Some(_) => None,
        None => option_env!("LIBVA_DRIVERS_PATH"),
    };
    /// The hidden argument of the child, which is given the render node to open.
    pub(super) const TRY: &str = "--try-vaapi-driver";
    /// How long one child may take to open a device before it is stopped and its driver counted as refused.
    const TRY_TIME: Duration = Duration::from_secs(10);

    /// What a start did about the drivers.
    #[derive(Debug)]
    pub(super) enum Outcome {
        /// libva's path does not start with the carried folder, or there is nothing to ask: it is as named.
        Untouched,
        /// The carried driver opens every awake AMD GPU: libva's path is as named.
        Kept,
        /// The carried driver was refused by an awake GPU, so the system's drivers go first.
        Moved,
        /// A GPU could not be asked, so its carried driver stays first.
        Unasked,
        /// libva's path was to be named or reordered, but this process was not proven to have one thread, so the
        /// environment was left as it was.
        NotAlone,
    }

    impl Outcome {
        pub(super) fn note(self) {
            match self {
                Self::Untouched | Self::Kept => {}
                Self::Moved => {
                    report::note!(
                        info,
                        "the carried VA-API driver did not open an AMD GPU, so the system's goes first"
                    );
                }
                Self::Unasked => {
                    report::note!(warn, "whether the carried VA-API driver opens a GPU was skipped: it was not asked");
                }
                Self::NotAlone => {
                    report::note!(warn, "a thread had already started, so the VA-API drivers were left as they were");
                }
            }
        }
    }

    /// A GPU by the PCI ids its vendor and model have, which stay when its render node's number changes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(super) struct Gpu {
        pub(super) vendor: PciVendor,
        pub(super) device: u16,
    }

    impl Gpu {
        /// The GPU of the render node the kernel lists at `listed`, by the ids in its `device` folder.
        pub(super) fn of(listed: &Path) -> Option<Self> {
            let device = listed.join("device");
            let read = |file: &str| std::fs::read_to_string(device.join(file)).ok();
            Some(Self { vendor: PciVendor(hex(&read("vendor")?)?), device: hex(&read("device")?)? })
        }
    }

    /// An awake GPU's render node.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Node {
        pub(super) path: String,
        pub(super) gpu: Gpu,
    }

    /// What a render node file of the kernel's holds: a hexadecimal number, as `0x1002`.
    fn hex(text: &str) -> Option<u16> {
        u16::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
    }

    /// The awake AMD GPUs under `sysfs`, by render node: AMD's is the one VA-API driver carried. One the kernel has
    /// put to sleep is left alone: asking its driver would wake it.
    pub(super) fn awake_amd(sysfs: &str) -> Vec<Node> {
        let mut found: Vec<Node> = render_nodes(sysfs)
            .filter(|entry| !asleep(entry))
            .filter_map(|entry| {
                let gpu = Gpu::of(&entry.path())?;
                (gpu.vendor == PciVendor::AMD).then(|| Node { path: node_path(&entry), gpu })
            })
            .collect();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }

    /// The carried driver as a build made it: its size and its time, so another build's is another driver.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Stamp {
        pub(super) size: u64,
        pub(super) modified: u64,
    }

    impl Stamp {
        fn of(driver: &Path) -> Option<Self> {
            let meta = std::fs::metadata(driver).ok()?;
            let modified = meta.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
            Some(Self { size: meta.len(), modified })
        }
    }

    /// Whether the carried driver opened a GPU's device.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Verdict {
        /// VA-API opened with the carried driver alone.
        Opened,
        /// It did not: it failed, crashed or took too long.
        Refused,
    }

    impl Verdict {
        fn word(self) -> &'static str {
            match self {
                Self::Opened => "opened",
                Self::Refused => "refused",
            }
        }

        fn of_word(word: &str) -> Option<Self> {
            [Self::Opened, Self::Refused].into_iter().find(|verdict| verdict.word() == word)
        }
    }

    /// What was learned: for one carried driver, what each GPU's device said of it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Verdicts {
        driver: Stamp,
        said: BTreeMap<Gpu, Verdict>,
    }

    /// A line of the kept file that is not what the format says.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Malformed {
        pub(super) line: usize,
    }

    impl fmt::Display for Malformed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "line {} of the learned VA-API drivers is not one the format has", self.line)
        }
    }

    impl std::error::Error for Malformed {}

    impl Verdicts {
        fn none(driver: Stamp) -> Self {
            Self { driver, said: BTreeMap::new() }
        }

        /// What `file` keeps of `driver`: nothing where it is missing, malformed or was learned of another driver.
        pub(super) fn read(file: &Path, driver: Stamp) -> Self {
            let kept = std::fs::read_to_string(file).ok().and_then(|text| Self::parse(&text).ok());
            kept.filter(|kept| kept.driver == driver).unwrap_or_else(|| Self::none(driver))
        }

        /// The file's text, in the order it is kept.
        pub(super) fn text(&self) -> String {
            let gpus: String = self
                .said
                .iter()
                .map(|(gpu, verdict)| format!("gpu {:04x}:{:04x} {}\n", gpu.vendor.0, gpu.device, verdict.word()))
                .collect();
            format!("{HEADER}\ndriver {} {}\n{gpus}", self.driver.size, self.driver.modified)
        }

        /// The kept file's text read back; any line the format does not have, or a GPU told twice, refuses all.
        pub(super) fn parse(text: &str) -> Result<Self, Malformed> {
            let mut lines = text.lines().enumerate().map(|(at, line)| (at + 1, line));
            if lines.next().map(|(_, line)| line) != Some(HEADER) {
                return Err(Malformed { line: 1 });
            }
            let bad = |line| Malformed { line };
            let (line, first) = lines.next().ok_or(bad(2))?;
            let mut words = first.split(' ');
            let (Some("driver"), Some(size), Some(modified), None) =
                (words.next(), words.next(), words.next(), words.next())
            else {
                return Err(bad(line));
            };
            let number = |word: &str| word.parse::<u64>().ok().filter(|number| number.to_string() == word);
            let driver = Stamp { size: number(size).ok_or(bad(line))?, modified: number(modified).ok_or(bad(line))? };
            let mut kept = Self::none(driver);
            for (line, text) in lines {
                let mut words = text.split(' ');
                let (Some("gpu"), Some(ids), Some(word), None) =
                    (words.next(), words.next(), words.next(), words.next())
                else {
                    return Err(bad(line));
                };
                let (vendor, device) = ids.split_once(':').ok_or(bad(line))?;
                let canonical = |id: &str| {
                    let digits = id.bytes().all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'));
                    (id.len() == 4 && digits).then(|| u16::from_str_radix(id, 16).ok()).flatten()
                };
                let vendor = PciVendor(canonical(vendor).ok_or(bad(line))?);
                let gpu = Gpu { vendor, device: canonical(device).ok_or(bad(line))? };
                let verdict = Verdict::of_word(word).ok_or(bad(line))?;
                if kept.said.insert(gpu, verdict).is_some() {
                    return Err(bad(line));
                }
            }
            Ok(kept)
        }
    }

    /// The folders of [`SYSTEMS`], in its order.
    pub(super) fn systems() -> impl Iterator<Item = &'static Path> {
        SYSTEMS.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).map(Path::new)
    }

    /// The carried folder: `tree` where the build was a tree's, else the folder beside `program`.
    pub(super) fn carried(tree: Option<&str>, program: &Path) -> Option<PathBuf> {
        tree.map(PathBuf::from).or_else(|| program.parent().map(|home| home.join(CARRIED)))
    }

    /// libva's driver path as a start finds it.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Start {
        /// The person's own, even empty: left as they set it.
        Own(OsString),
        /// None was set: this is to be set, the carried folder and then [`systems`].
        Named(OsString),
    }

    /// The driver path a start goes on with, from the person's own `own`; none where it is unset and the folders
    /// cannot be joined.
    pub(super) fn start(own: Option<OsString>, carried: &Path) -> Option<Start> {
        match own {
            Some(own) => Some(Start::Own(own)),
            None => std::env::join_paths(std::iter::once(carried).chain(systems().map(|system| system as &Path)))
                .ok()
                .map(Start::Named),
        }
    }

    /// Whether `list`'s first folder is `carried`.
    pub(super) fn carried_first(list: &OsStr, carried: &Path) -> bool {
        std::env::split_paths(list).next().is_some_and(|first| first == carried)
    }

    /// `list` with `carried` after every other folder, in their order; none where it is not in `list` or the folders
    /// cannot be joined again.
    pub(super) fn carried_last(list: &OsStr, carried: &Path) -> Option<OsString> {
        let (ours, others): (Vec<PathBuf>, Vec<PathBuf>) =
            std::env::split_paths(list).partition(|folder| folder == carried);
        if ours.is_empty() {
            return None;
        }
        std::env::join_paths(others.into_iter().chain(ours)).ok()
    }

    /// Asks about each of `nodes` whose GPU `known` has no verdict on, with `ask`, which gives none when it could not
    /// ask. Answers whether a verdict was added, and whether a GPU stays unasked.
    pub(super) fn asked(
        nodes: &[Node],
        known: &mut Verdicts,
        mut ask: impl FnMut(&Node) -> Option<Verdict>,
    ) -> (bool, bool) {
        let (mut added, mut unasked) = (false, false);
        let unknown: Vec<&Node> = nodes.iter().filter(|node| !known.said.contains_key(&node.gpu)).collect();
        for node in unknown {
            match ask(node) {
                Some(verdict) => {
                    known.said.insert(node.gpu, verdict);
                    added = true;
                }
                None => unasked = true,
            }
        }
        (added, unasked)
    }

    /// Whether any of `nodes` has a GPU the carried driver was refused by.
    pub(super) fn refused(nodes: &[Node], known: &Verdicts) -> bool {
        nodes.iter().any(|node| known.said.get(&node.gpu) == Some(&Verdict::Refused))
    }

    /// # Safety
    ///
    /// No other thread may exist: it sets an environment variable of the process.
    pub(super) unsafe fn order_va_drivers(state: Option<&Path>) -> Outcome {
        let Ok(program) = std::env::current_exe() else { return Outcome::Untouched };
        let Some(carried) = carried(TREE, &program) else { return Outcome::Untouched };
        let list = match start(std::env::var_os(PATH), &carried) {
            Some(Start::Own(list)) => list,
            Some(Start::Named(list)) => {
                if !single_threaded() {
                    return Outcome::NotAlone;
                }
                // SAFETY: the caller promised that no other thread exists to read or write the environment meanwhile.
                unsafe { std::env::set_var(PATH, &list) };
                list
            }
            None => return Outcome::Untouched,
        };
        let Some(state) = state else { return Outcome::Untouched };
        if !carried_first(&list, &carried) {
            return Outcome::Untouched;
        }
        let Some(driver) = Stamp::of(&carried.join(DRIVER)) else { return Outcome::Untouched };
        let nodes = awake_amd(DRM);
        if nodes.is_empty() {
            return Outcome::Untouched;
        }
        let file = state.join(FILE);
        let mut known = Verdicts::read(&file, driver);
        let (added, unasked) = asked(&nodes, &mut known, |node| ask(&program, &carried, &node.path));
        // Best effort: a start that cannot keep what it learned still acts on it, and asks again next time.
        if added {
            let _ = keep(&file, &known.text());
        }
        if !refused(&nodes, &known) {
            return if unasked { Outcome::Unasked } else { Outcome::Kept };
        }
        let Some(reordered) = carried_last(&list, &carried) else { return Outcome::Untouched };
        if !single_threaded() {
            return Outcome::NotAlone;
        }
        // SAFETY: the caller promised that no other thread exists to read or write the environment meanwhile.
        unsafe { std::env::set_var(PATH, reordered) };
        Outcome::Moved
    }

    /// The hidden arguments that ask a child to open VA-API on `node`.
    pub(super) fn hidden(node: &str) -> [OsString; 2] {
        [OsString::from(TRY), OsString::from(node)]
    }

    /// The render node `args` ask a child to open, when they are the hidden arguments.
    pub(super) fn asking(args: &[OsString]) -> Option<&OsStr> {
        match args {
            [mode, node] if mode == TRY => Some(node.as_os_str()),
            _ => None,
        }
    }

    pub(super) fn run_hidden(args: &[OsString]) -> Option<ExitCode> {
        let asked_node = asking(args)?;
        Some(match asked_node.to_str().filter(|token| is_render_node(token)) {
            Some(render_node) => trier(render_node),
            None => ExitCode::FAILURE,
        })
    }

    /// The child: opens VA-API on `node` with whatever libva's path names, and says whether it did.
    fn trier(node: &str) -> ExitCode {
        let opened = HwDevice::open(Backend::VaApi, Some(node), Validation::Off).is_some();
        if opened { ExitCode::SUCCESS } else { ExitCode::FAILURE }
    }

    /// What a child says of the carried driver on `node`: started with the carried folder alone, so the system's
    /// cannot answer for it. None where no child could be started or watched.
    fn ask(program: &Path, carried: &Path, node: &str) -> Option<Verdict> {
        let mut child = Command::new(program)
            .args(hidden(node))
            .env(PATH, carried)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(match within(&mut child, TRY_TIME).ok()? {
            Some(status) if status.success() => Verdict::Opened,
            Some(_) | None => Verdict::Refused,
        })
    }
}

#[cfg(all(test, target_os = "linux"))]
#[expect(clippy::unwrap_used, reason = "a test's fixture: a step that fails is the test failing")]
mod tests {
    use super::linux::{
        Gpu, Malformed, Node, Stamp, Start, TRY, Verdict, Verdicts, asked, asking, awake_amd, carried, carried_first,
        carried_last, hidden, refused, start, systems,
    };
    use super::{node_vendor, pci_ids, run_hidden};
    use crate::probe::PciVendor;
    use crate::startup::keep;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

    const HEADER: &str = "chatkord-libva-drivers 1";
    const CARRIED: &str = "/tmp/mount/opt/chatkord/dri";
    const RENOIR: Gpu = Gpu { vendor: PciVendor::AMD, device: 0x1636 };
    const NAVI: Gpu = Gpu { vendor: PciVendor::AMD, device: 0x73bf };
    const NODE_A: &str = "/dev/dri/renderD128";
    const NODE_B: &str = "/dev/dri/renderD129";

    /// A stand-in for `/sys/class/drm`, taken away when dropped.
    struct Machine(PathBuf);

    impl Machine {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("media-drivers-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn node(&self, node: &str, vendor: &str, device: &str, status: &str) {
            let folder = self.0.join(node).join("device");
            std::fs::create_dir_all(folder.join("power")).unwrap();
            std::fs::write(folder.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(folder.join("device"), format!("{device}\n")).unwrap();
            std::fs::write(folder.join("power/runtime_status"), status).unwrap();
        }

        fn sysfs(&self) -> String {
            self.0.to_str().unwrap().to_string()
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The systems' folders as libva's path joins them.
    fn system_list() -> String {
        systems().map(|folder| folder.to_str().unwrap()).collect::<Vec<_>>().join(":")
    }

    fn list(first: &str) -> OsString {
        OsString::from(format!("{first}:{}", system_list()))
    }

    const DRIVER: Stamp = Stamp { size: 7, modified: 9 };

    /// What a file of the carried driver `DRIVER` keeps of `said`.
    fn known(said: &[(Gpu, Verdict)]) -> Verdicts {
        let mut text = format!("{HEADER}\ndriver 7 9\n");
        for (gpu, verdict) in said {
            let word = if *verdict == Verdict::Opened { "opened" } else { "refused" };
            text.push_str(&format!("gpu {:04x}:{:04x} {word}\n", gpu.vendor.0, gpu.device));
        }
        Verdicts::parse(&text).unwrap()
    }

    fn node(path: &str, gpu: Gpu) -> Node {
        Node { path: path.to_string(), gpu }
    }

    #[test]
    fn the_carried_folder_is_found_first_and_moved_after_the_systems_in_their_order() {
        let carried = Path::new(CARRIED);
        let systems = system_list();
        assert!(carried_first(&list(CARRIED), carried));
        assert!(!carried_first(&list("/opt/mine"), carried), "a person's own folder first is none of ours");
        assert!(!carried_first(OsStr::new(""), carried));
        assert_eq!(carried_last(&list(CARRIED), carried), Some(OsString::from(format!("{systems}:{CARRIED}"))));
        assert_eq!(carried_last(OsStr::new(CARRIED), carried), Some(OsString::from(CARRIED)), "nothing to go before");
        assert_eq!(carried_last(OsStr::new(&systems), carried), None, "not in the list at all");
    }

    #[test]
    fn the_systems_folders_are_searched_as_the_distributions_own_libva_searches_them() {
        let folders: Vec<&Path> = systems().collect();
        let at = |folder: &str| folders.iter().position(|found| *found == Path::new(folder));
        assert!(folders.iter().all(|folder| folder.is_absolute()), "{folders:?}");
        let (nonfree, freeworld, fedora) =
            (at("/usr/lib64/dri-nonfree"), at("/usr/lib64/dri-freeworld"), at("/usr/lib64/dri"));
        assert!(
            nonfree.is_some() && nonfree < freeworld && freeworld < fedora,
            "RPM Fusion's before the stock: {folders:?}"
        );
        assert!(at("/usr/lib/x86_64-linux-gnu/dri").is_some(), "Debian's and Ubuntu's: {folders:?}");
        assert!(at("/usr/lib/dri").is_some(), "Arch's: {folders:?}");
    }

    #[test]
    fn a_start_with_no_driver_path_names_the_carried_folder_then_the_systems_and_keeps_a_persons_own() {
        let carried_here = Path::new(CARRIED);
        assert_eq!(start(None, carried_here), Some(Start::Named(list(CARRIED))));
        for own in ["/opt/mine", ""] {
            assert_eq!(
                start(Some(OsString::from(own)), carried_here),
                Some(Start::Own(OsString::from(own))),
                "{own:?}"
            );
        }
        let tree = "/src/build/deps/lib/dri";
        assert_eq!(carried(Some(tree), Path::new("/src/client/target/release/shell")), Some(PathBuf::from(tree)));
        let program = Path::new("/tmp/mount/opt/chatkord/chatkord");
        assert_eq!(carried(None, program), Some(PathBuf::from(CARRIED)), "a package's is beside its program");
    }

    #[test]
    fn only_awake_amd_gpus_are_asked_and_by_their_render_node() {
        let machine = Machine::new("awake");
        machine.node("renderD128", "0x1002", "0x1636", "active");
        machine.node("renderD129", "0x1002", "0x73bf", "suspended");
        machine.node("renderD130", "0x10de", "0x2684", "active");
        machine.node("renderD131", "0x1002", "0x73bf", "unsupported");
        machine.node("card0", "0x1002", "0x73bf", "active");
        std::fs::write(machine.0.join("renderD132"), "not a folder").unwrap();
        assert_eq!(awake_amd(&machine.sysfs()), [node(NODE_A, RENOIR), node("/dev/dri/renderD131", NAVI)]);
        assert!(awake_amd("/nowhere/at/all").is_empty());
    }

    #[test]
    fn a_render_node_s_pci_ids_are_read_from_its_device_folder_or_not_at_all() {
        let machine = Machine::new("ids");
        machine.node("renderD128", "0x8086", "0x4692", "active");
        machine.node("renderD129", "nonsense", "0x4692", "active");
        assert_eq!(pci_ids(&machine.0.join("renderD128")), Some((PciVendor::INTEL, 0x4692)));
        assert_eq!(pci_ids(&machine.0.join("renderD129")), None);
        assert_eq!(pci_ids(&machine.0.join("renderD130")), None);
        assert_eq!(node_vendor("/dev/dri/renderD999"), None, "a node the kernel does not list");
    }

    #[test]
    fn each_gpu_is_asked_once_and_one_refusal_decides() {
        let nodes = [node(NODE_A, RENOIR), node(NODE_B, NAVI)];
        let mut kept = known(&[(RENOIR, Verdict::Opened)]);
        let mut said = Vec::new();
        let (added, unasked) = asked(&nodes, &mut kept, |node| {
            said.push(node.path.clone());
            Some(Verdict::Refused)
        });
        assert_eq!((added, unasked, said.as_slice()), (true, false, &[NODE_B.to_string()][..]));
        assert!(refused(&nodes, &kept), "one GPU refusing is enough");
        assert!(!refused(&nodes[..1], &kept), "and the other's own verdict stands");
        let (added, unasked) = asked(&nodes, &mut kept, |node| panic!("{} was asked again", node.path));
        assert_eq!((added, unasked), (false, false), "everything is known: nobody is asked");
        let mut fresh = known(&[]);
        let (added, unasked) = asked(&nodes, &mut fresh, |_| None);
        assert_eq!((added, unasked), (false, true), "a GPU that could not be asked stays unknown");
        assert!(!refused(&nodes, &fresh));
    }

    #[test]
    fn the_kept_verdicts_print_and_read_back_and_a_malformed_file_is_refused() {
        let kept = known(&[(NAVI, Verdict::Refused), (RENOIR, Verdict::Opened)]);
        assert_eq!(kept.text(), format!("{HEADER}\ndriver 7 9\ngpu 1002:1636 opened\ngpu 1002:73bf refused\n"));
        assert_eq!(Verdicts::parse(&kept.text()), Ok(kept));
        let refused = |text: &str, line| assert_eq!(Verdicts::parse(text), Err(Malformed { line }), "{text:?}");
        refused("", 1);
        refused("chatkord-libva-drivers 2\ndriver 7 9\n", 1);
        refused(&format!("{HEADER}\n"), 2);
        refused(&format!("{HEADER}\ndriver 7\n"), 2);
        refused(&format!("{HEADER}\ndriver 07 9\n"), 2);
        refused(&format!("{HEADER}\ndriver 7 9\ngpu 1002:1636 maybe\n"), 3);
        refused(&format!("{HEADER}\ndriver 7 9\ngpu 1002:1636\n"), 3);
        refused(&format!("{HEADER}\ndriver 7 9\ngpu 1002:163 opened\n"), 3);
        refused(&format!("{HEADER}\ndriver 7 9\ngpu 1002:73BF opened\n"), 3);
        refused(&format!("{HEADER}\ndriver 7 9\ngpu 1002:1636 opened\ngpu 1002:1636 refused\n"), 4);
    }

    #[test]
    fn verdicts_of_another_driver_are_not_read_and_a_kept_file_is_whole() {
        let machine = Machine::new("kept");
        let file = machine.0.join("state/libva-drivers.txt");
        let kept = known(&[(RENOIR, Verdict::Refused)]);
        keep(&file, &kept.text()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), kept.text());
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1, "no partial file is left behind");
        assert_eq!(Verdicts::read(&file, DRIVER), kept);
        let rebuilt = Stamp { size: 8, ..DRIVER };
        assert_eq!(Verdicts::read(&file, rebuilt), Verdicts::parse(&format!("{HEADER}\ndriver 8 9\n")).unwrap());
        assert_eq!(Verdicts::read(&machine.0.join("none"), DRIVER), known(&[]), "no file is no verdict");
    }

    #[test]
    fn the_hidden_arguments_round_trip_and_nothing_else_runs_hidden() {
        let args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(asking(&hidden(NODE_A)), Some(OsStr::new(NODE_A)));
        for words in [&[][..], &["--forget"], &[TRY], &[TRY, NODE_A, "more"], &["register", "example.invalid", "ada"]] {
            assert_eq!(asking(&args(words)), None, "{words:?}");
            assert_eq!(run_hidden(&args(words)), None, "{words:?}");
        }
        assert_eq!(run_hidden(&hidden("/dev/dri/card0")), Some(std::process::ExitCode::FAILURE), "not a render node");
    }
}
