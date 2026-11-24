//! The encoder survey kept for a process: a survey opens every device and every candidate encoder, so it is made again
//! only when this machine's video devices change or an encoder it listed fails.

#[cfg(windows)]
use crate::probe::PciVendor;
use crate::probe::{Report, Sources, survey_on};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

/// This machine's video devices as a survey saw them: read at every ask, far cheaper than a survey.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Devices(Vec<Seen>);

/// One video device, by what changes when it goes, comes back or is replaced.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Seen {
    /// A render node, by its path and the device number the kernel gave it.
    #[cfg(not(windows))]
    Node(String, u64),
    /// A graphics adapter, by the id Windows gives it until it goes or its driver restarts.
    #[cfg(windows)]
    Adapter(capture::AdapterId),
}

impl Devices {
    /// The render nodes under `/dev/dri`.
    #[cfg(not(windows))]
    pub fn system() -> Self {
        use std::os::unix::fs::MetadataExt;
        let entries = std::fs::read_dir("/dev/dri").into_iter().flatten().filter_map(Result::ok);
        let mut seen: Vec<Seen> = entries
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
            .filter_map(|entry| Some(Seen::Node(entry.path().display().to_string(), entry.metadata().ok()?.rdev())))
            .collect();
        seen.sort();
        Self(seen)
    }

    /// `node` alone, as [`Devices::system`] sees it: none when it is gone or there is none.
    #[cfg(not(windows))]
    fn of(node: Option<&str>) -> Self {
        let mut all = Self::system();
        all.0.retain(|Seen::Node(path, _)| Some(path.as_str()) == node);
        all
    }

    /// `adapter`'s alone, as [`Devices::system`] sees it, named as a survey's devices are: none when it is gone; every
    /// adapter where the window names none.
    #[cfg(windows)]
    fn of(adapter: Option<&str>) -> Self {
        let mut all = Self::system();
        if let Some(adapter) = adapter {
            all.0.retain(|Seen::Adapter(id)| id.to_string() == adapter);
        }
        all
    }

    /// The hardware adapters DXGI lists; Windows' own software adapter encodes nothing.
    #[cfg(windows)]
    pub fn system() -> Self {
        use windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE;
        let mut seen: Vec<Seen> = described()
            .into_iter()
            .filter(|described| described.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0.cast_unsigned() == 0)
            .map(|described| Seen::Adapter(capture::AdapterId::of(described.AdapterLuid)))
            .collect();
        seen.sort();
        Self(seen)
    }
}

/// The adapters DXGI lists, each as it describes itself, in its order: the default adapter first. None where no
/// factory is made.
#[cfg(windows)]
fn described() -> Vec<windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    // SAFETY: a factory made with no arguments, owned by the returned interface.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    // SAFETY: the factory is live; an index past the last adapter ends the list with an error.
    let adapters = (0..).map_while(|at| unsafe { factory.EnumAdapters1(at) }.ok());
    // SAFETY: the adapter is live and its description is read once.
    adapters.filter_map(|adapter| unsafe { adapter.GetDesc1() }.ok()).collect()
}

/// The vendor of `adapter`'s GPU, or of the default adapter's where none is named, as DXGI describes it; none where
/// it is not listed.
#[cfg(windows)]
pub(crate) fn adapter_vendor(adapter: Option<capture::AdapterId>) -> Option<PciVendor> {
    let mut listed = described().into_iter();
    let found = match adapter {
        Some(adapter) => listed.find(|described| capture::AdapterId::of(described.AdapterLuid) == adapter),
        None => listed.next(),
    }?;
    u16::try_from(found.VendorId).ok().map(PciVendor)
}

/// Where a kept survey's facts come from: this machine, or a test's stand-in.
pub trait Surveyor: Send + Sync {
    /// The video devices there now.
    fn devices(&self) -> Devices;
    /// A new survey of them, and the devices it was made on.
    fn survey(&self) -> (Devices, Report);
}

/// This machine's own devices and encoders.
struct System;

impl Surveyor for System {
    fn devices(&self) -> Devices {
        Devices::system()
    }

    fn survey(&self) -> (Devices, Report) {
        (Devices::system(), survey_on(&Sources::system()))
    }
}

/// The GPU that paints the window alone, its adapter asked at every look: a driver's reset gives the adapter another id,
/// and a survey must follow it.
#[cfg(windows)]
struct Painting {
    find: Box<dyn Fn() -> Option<String> + Send + Sync>,
}

#[cfg(windows)]
impl Surveyor for Painting {
    fn devices(&self) -> Devices {
        Devices::of((self.find)().as_deref())
    }

    fn survey(&self) -> (Devices, Report) {
        let adapter = (self.find)();
        (Devices::of(adapter.as_deref()), survey_on(&Sources::painting(adapter.as_deref())))
    }
}

/// What the devices on a render node are, the machine's or a test's stand-in.
#[cfg(not(windows))]
type Look = dyn Fn(Option<&str>) -> Devices + Send + Sync;

/// The GPU that paints the window alone, its render node found at each survey and kept for the looks between: finding
/// it lists every Vulkan adapter, too costly for every look, and a node renumbered changes the devices looked at.
#[cfg(not(windows))]
struct Painting {
    find: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// The devices a node names: the machine's, or a test's stand-in.
    look: Box<Look>,
    /// The node the last survey found.
    node: Mutex<Option<String>>,
}

#[cfg(not(windows))]
impl Surveyor for Painting {
    fn devices(&self) -> Devices {
        (self.look)(self.node.lock().unwrap_or_else(PoisonError::into_inner).as_deref())
    }

    fn survey(&self) -> (Devices, Report) {
        let node = (self.find)();
        let devices = (self.look)(node.as_deref());
        let report = survey_on(&Sources::painting(node.as_deref()));
        *self.node.lock().unwrap_or_else(PoisonError::into_inner) = node;
        (devices, report)
    }
}

/// The encoder survey, kept until the devices it was made on change or an encoder it listed fails. One a process,
/// shared by the window and every share and camera.
pub struct Survey {
    surveyor: Box<dyn Surveyor>,
    kept: Mutex<Option<(Devices, Arc<Report>)>>,
}

impl fmt::Debug for Survey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Survey").field("kept", &kept.as_ref().map(|(devices, _)| devices)).finish_non_exhaustive()
    }
}

impl Survey {
    /// The survey of this machine.
    pub fn system() -> Self {
        Self::with(System)
    }

    /// The survey of the GPU that paints the window alone: a GPU that does not paint is never opened, so one asleep stays
    /// asleep. `find` gives its render node at each survey on Linux, kept for the looks between; on Windows its
    /// adapter's id, as `capture::AdapterId` writes it, at every look; a window that cannot say (Windows') leaves the
    /// default adapter. None on Linux: software alone.
    pub fn painting(find: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        #[cfg(not(windows))]
        let surveyor = Painting { find: Box::new(find), look: Box::new(Devices::of), node: Mutex::new(None) };
        #[cfg(windows)]
        let surveyor = Painting { find: Box::new(find) };
        Self::with(surveyor)
    }

    /// A survey whose facts come from `surveyor`.
    pub fn with(surveyor: impl Surveyor + 'static) -> Self {
        Self { surveyor: Box::new(surveyor), kept: Mutex::new(None) }
    }

    /// What this machine can encode: the kept report while its devices are the ones it was made on and no encoder
    /// failed since, else a new survey, which asks that come meanwhile wait for. Blocking: it may open every device.
    pub fn report(&self) -> Arc<Report> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let devices = self.surveyor.devices();
        if let Some((on, report)) = kept.as_ref()
            && *on == devices
        {
            return Arc::clone(report);
        }
        let (on, report) = self.surveyor.survey();
        let report = Arc::new(report);
        *kept = Some((on, Arc::clone(&report)));
        report
    }

    /// An encoder failed: the next ask surveys again, so one that no longer opens is not offered.
    pub fn failed(&self) {
        *self.kept.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// Counts its surveys; its devices are what a test sets.
    #[derive(Clone, Default)]
    struct Counted {
        surveys: Arc<AtomicUsize>,
        devices: Arc<Mutex<Devices>>,
    }

    impl Surveyor for Counted {
        fn devices(&self) -> Devices {
            self.devices.lock().expect("the devices").clone()
        }

        fn survey(&self) -> (Devices, Report) {
            self.surveys.fetch_add(1, Ordering::SeqCst);
            (self.devices(), Report::default())
        }
    }

    impl Counted {
        fn surveys(&self) -> usize {
            self.surveys.load(Ordering::SeqCst)
        }
    }

    #[cfg(not(windows))]
    fn another_device() -> Devices {
        Devices(vec![Seen::Node("/dev/dri/renderD128".into(), 57_472)])
    }

    #[cfg(windows)]
    fn another_device() -> Devices {
        Devices(vec![Seen::Adapter("0000000100002a3b".parse().unwrap())])
    }

    #[test]
    fn a_survey_is_made_once_and_kept_while_its_devices_stay() {
        let counted = Counted::default();
        let survey = Survey::with(counted.clone());
        let first = survey.report();
        for _ in 0..3 {
            assert!(Arc::ptr_eq(&survey.report(), &first), "the kept report");
        }
        assert_eq!(counted.surveys(), 1);
    }

    #[test]
    fn a_device_that_comes_or_goes_is_a_new_survey() {
        let counted = Counted::default();
        let survey = Survey::with(counted.clone());
        survey.report();
        *counted.devices.lock().unwrap() = another_device();
        survey.report();
        survey.report();
        assert_eq!(counted.surveys(), 2, "once more for the new device, then kept");
        *counted.devices.lock().unwrap() = Devices::default();
        survey.report();
        assert_eq!(counted.surveys(), 3, "and once more when it goes");
    }

    #[test]
    fn an_encoder_that_failed_is_a_new_survey_at_the_next_ask() {
        let counted = Counted::default();
        let survey = Survey::with(counted.clone());
        survey.report();
        survey.failed();
        assert_eq!(counted.surveys(), 1, "the next ask surveys, not the failure");
        survey.report();
        survey.report();
        assert_eq!(counted.surveys(), 2);
    }

    #[test]
    fn asks_at_once_wait_for_one_survey() {
        let counted = Counted::default();
        let survey = Arc::new(Survey::with(counted.clone()));
        let asks: Vec<_> = (0..16)
            .map(|_| {
                let survey = Arc::clone(&survey);
                std::thread::spawn(move || survey.report())
            })
            .collect();
        for ask in asks {
            ask.join().expect("an ask");
        }
        assert_eq!(counted.surveys(), 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_painting_survey_finds_its_node_at_each_survey_and_follows_a_renumbered_one() {
        const FIRST: &str = "/dev/dri/renderD990";
        const RENUMBERED: &str = "/dev/dri/renderD991";
        let finds = Arc::new(AtomicUsize::new(0));
        let painted = Arc::new(Mutex::new(FIRST.to_string()));
        let present = Arc::new(Mutex::new(vec![FIRST.to_string()]));
        let find = {
            let (finds, painted) = (Arc::clone(&finds), Arc::clone(&painted));
            move || {
                finds.fetch_add(1, Ordering::SeqCst);
                Some(painted.lock().unwrap().clone())
            }
        };
        let look = {
            let present = Arc::clone(&present);
            move |node: Option<&str>| {
                let here = present.lock().unwrap();
                Devices(
                    here.iter().filter(|at| Some(at.as_str()) == node).map(|at| Seen::Node(at.clone(), 1)).collect(),
                )
            }
        };
        let painting = Painting { find: Box::new(find), look: Box::new(look), node: Mutex::new(None) };
        let survey = Survey::with(painting);
        let first = survey.report();
        for _ in 0..3 {
            assert!(Arc::ptr_eq(&survey.report(), &first), "the kept report");
        }
        assert_eq!(finds.load(Ordering::SeqCst), 1, "found at the survey alone, never at a look");
        *painted.lock().unwrap() = RENUMBERED.to_string();
        *present.lock().unwrap() = vec![RENUMBERED.to_string()];
        let second = survey.report();
        assert!(!Arc::ptr_eq(&second, &first), "the node it was made on went: a new survey");
        assert_eq!(finds.load(Ordering::SeqCst), 2);
        assert!(Arc::ptr_eq(&survey.report(), &second), "kept on the renumbered node");
        assert_eq!(finds.load(Ordering::SeqCst), 2);
    }

    mod lab {
        use super::*;

        /// What a survey and a look at the devices cost on this machine, for the record: the look is what every ask
        /// pays once the survey is kept.
        #[test]
        fn the_machine_s_survey_and_its_devices_are_timed_for_the_record() {
            tier::lab();
            let began = Instant::now();
            let (_, report) = System.survey();
            let surveyed = began.elapsed();
            let began = Instant::now();
            let devices = Devices::system();
            let looked = began.elapsed();
            assert_eq!(Devices::system(), devices, "the same devices twice");
            eprintln!(
                "a survey took {surveyed:?} ({} encoders), a look at {} devices {looked:?}",
                report.encoders.len(),
                devices.0.len()
            );
        }
    }
}
