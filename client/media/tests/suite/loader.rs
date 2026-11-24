//! Every way into the Vulkan loader at once, from threads that reach it in the same moment: a decoder on a Vulkan
//! device, a survey of the encoders, and the capture's question about which layouts the GPU can import; and what the
//! lock does not hold up, which is everything that never enters the loader.

use crate::common;

use common::skipped;
use media::{Backend, Codec, Decoder, Path, Session, Sources, alone, importable, survey, survey_on};
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::{Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const THREADS: usize = 8;
const ROUNDS: usize = 10;

/// Sources that name nothing but Vulkan's default device: a decoder opened this way is on the loader or nowhere,
/// which decides whether this machine proves anything here.
fn vulkan_only() -> Sources {
    Sources { render_nodes: Vec::new(), vulkan: true, ..Sources::default() }
}

/// A decoder on Vulkan's default device, or none where it does not open there.
fn on_vulkan() -> Option<Decoder> {
    Decoder::open(Codec::H264, &vulkan_only()).ok().filter(|decoder| decoder.path() == Path::Hardware(Backend::Vulkan))
}

/// Runs `work` on a thread of its own and says whether it finished within `bound`.
fn finishes_within(bound: Duration, work: impl FnOnce() + Send + 'static) -> bool {
    let (done, told) = mpsc::channel();
    thread::spawn(move || {
        work();
        let _ = done.send(());
    });
    told.recv_timeout(bound).is_ok()
}

#[test]
fn a_survey_with_no_device_a_software_encoder_and_a_software_decoder_do_not_wait_for_the_loaders_lock() {
    let held = alone();
    let survey_ends = finishes_within(Duration::from_secs(5), || {
        survey_on(&Sources::default());
    });
    let encoder_ends = finishes_within(Duration::from_secs(5), || {
        let encoder = survey_on(&Sources::default())
            .encoders
            .into_iter()
            .find(|encoder| encoder.codec() == Codec::H264)
            .expect("x264 is built");
        drop(Session::open(&encoder, common::SETTINGS).expect("x264 opens"));
    });
    let decoder_ends = finishes_within(Duration::from_secs(5), || {
        let software = Sources { render_nodes: Vec::new(), ..Sources::default() };
        Decoder::open(Codec::Av1, &software).expect("dav1d opens");
    });
    drop(held);
    assert!(survey_ends, "a survey that opens no device waited for the loader's lock");
    assert!(encoder_ends, "a software encoder's open waited for the loader's lock");
    assert!(decoder_ends, "a software decoder's open waited for the loader's lock");
}

mod lab {
    use super::*;

    #[test]
    fn libva_is_linked_in_so_no_shared_one_is_ever_loaded() {
        // FFmpeg's default is the first render node, which may be a GPU no carried driver serves.
        let opened = Sources::system()
            .render_nodes
            .iter()
            .find_map(|node| media::av::HwDevice::open(Backend::VaApi, Some(node), media::Validation::Off));
        let Some(device) = opened else {
            return skipped("no VA-API device opens here, so libva is not asked for");
        };
        let maps = std::fs::read_to_string("/proc/self/maps").expect("the kernel lists this process's maps");
        let loaded: Vec<&str> =
            maps.lines().filter(|line| line.contains("libva.so") || line.contains("libva-drm.so")).collect();
        drop(device);
        assert!(loaded.is_empty(), "a shared libva is mapped beside the one linked in: {loaded:?}");
    }

    #[test]
    fn a_devices_last_release_waits_for_the_loaders_lock() {
        let Some(decoder) = on_vulkan() else {
            return skipped("no H.264 decoder opens on a Vulkan device here, so there is no device to release");
        };
        let held = alone();
        let (released, told) = mpsc::channel();
        let releasing = thread::spawn(move || {
            drop(decoder);
            let _ = released.send(());
        });
        assert!(
            told.recv_timeout(Duration::from_millis(300)).is_err(),
            "the device's last release waits while another thread holds the loader"
        );
        drop(held);
        assert!(told.recv_timeout(Duration::from_secs(5)).is_ok(), "and ends once the loader is free");
        releasing.join().expect("no thread panics");
    }

    #[test]
    fn a_thread_holding_the_lock_releases_a_device_without_waiting_on_itself() {
        let Some(decoder) = on_vulkan() else {
            return skipped("no H.264 decoder opens on a Vulkan device here, so there is no device to release");
        };
        let (released, told) = mpsc::channel();
        let releasing = thread::spawn(move || {
            let _alone = alone();
            drop(decoder);
            let _ = released.send(());
        });
        assert!(told.recv_timeout(Duration::from_secs(5)).is_ok(), "a release under its own thread's lock ends");
        releasing.join().expect("no thread panics");
    }

    #[test]
    fn decoders_encoders_and_surveys_from_many_threads_do_not_crash() {
        let opened = Decoder::open(Codec::H264, &vulkan_only());
        if opened.is_err() || opened.expect("a Vulkan decoder").path() != Path::Hardware(Backend::Vulkan) {
            skipped("no H.264 decoder opens on a Vulkan device here, so there is no device to open twice");
            return;
        }
        for _ in 0..ROUNDS {
            let barrier = Barrier::new(THREADS);
            // Each thread owns its number and borrows the barrier, which outlives every thread.
            let together = &barrier;
            thread::scope(|scope| {
                let workers: Vec<_> = (0..THREADS)
                    .map(|where_| {
                        scope.spawn(move || {
                            together.wait();
                            // The three entries into the loader, cycled over the threads.
                            let role = where_ % 3;
                            if role == 0 {
                                // A decoder: the device, then the codec opened on it.
                                let decoder =
                                    Decoder::open(Codec::H264, &vulkan_only()).expect("a Vulkan decoder opens");
                                assert_eq!(
                                    decoder.path(),
                                    Path::Hardware(Backend::Vulkan),
                                    "the machine was chosen by the check above"
                                );
                            } else if role == 1 {
                                // The survey: a device per backend, and an encoder opened on each.
                                survey();
                            } else {
                                // The capture's Vulkan question: an instance of the same loader.
                                importable();
                            }
                        })
                    })
                    .collect();
                workers.into_iter().for_each(|worker| worker.join().expect("no thread panics"));
            });
        }
    }

    /// Measures, and asserts nothing: which class of entry crashes the loader when its threads meet, and after how
    /// many rounds, for the `Measured:` line of the item that scopes the lock.
    #[test]
    fn the_loader_crash_without_the_lock_is_measured_for_each_class_of_entry() {
        tier::lab();
        if let Ok(name) = std::env::var(unlocked::CLASS) {
            return unlocked::Class::named(&name).expect("a class the parent named").child();
        }
        let program = std::env::current_exe().expect("the test binary's path");
        for class in unlocked::Class::ALL {
            let mut child = Command::new(&program)
                .args([
                    "--exact",
                    "loader::lab::the_loader_crash_without_the_lock_is_measured_for_each_class_of_entry",
                    "--nocapture",
                ])
                .env(unlocked::CLASS, class.name())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("a child of the test binary starts");
            let began = Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().expect("the child can be asked") {
                    break Some(status);
                }
                if began.elapsed() > unlocked::HUNG {
                    child.kill().expect("a hung child is killed");
                    child.wait().expect("a killed child is waited for");
                    break None;
                }
                thread::sleep(Duration::from_millis(50));
            };
            let mut said = String::new();
            child.stdout.take().expect("stdout was piped").read_to_string(&mut said).expect("the child's lines");
            let rounds = said.lines().filter(|line| *line == unlocked::ROUND).count();
            let of = unlocked::ROUNDS_EACH * THREADS;
            let outcome = match status.map(|status| (status, status.signal())) {
                None => format!("hung after {rounds} of {of} thread-rounds"),
                Some((_, Some(signal))) => format!("died of signal {signal} after {rounds} of {of} thread-rounds"),
                Some((status, None)) => format!("ended with {status} after {rounds} of {of} thread-rounds"),
            };
            let _ = writeln!(std::io::stderr(), "loader without the lock, {}: {outcome}", class.name());
        }
    }
}

/// What the reproducer's child runs without the loader's lock, a class at a time.
mod unlocked {
    use super::*;

    /// The class a child of the test binary runs, in the environment of the child alone.
    pub(super) const CLASS: &str = "CHATKORD_LOADER_CLASS";
    /// The line a child's thread writes after each round it finished.
    pub(super) const ROUND: &str = "round";
    pub(super) const ROUNDS_EACH: usize = 100;
    /// The longest a child may run before it is called hung.
    pub(super) const HUNG: Duration = Duration::from_secs(120);

    /// A way into a loader, run by every thread at once.
    #[derive(Clone, Copy)]
    pub(super) enum Class {
        /// An instance and its first GPU's device, made and ended through ash.
        RawVulkan,
        /// FFmpeg's Vulkan device, made and released.
        FfmpegVulkan,
        /// FFmpeg's VA-API device, made and released.
        FfmpegVaapi,
    }

    impl Class {
        pub(super) const ALL: [Self; 3] = [Self::RawVulkan, Self::FfmpegVulkan, Self::FfmpegVaapi];

        pub(super) const fn name(self) -> &'static str {
            match self {
                Self::RawVulkan => "vulkan-raw",
                Self::FfmpegVulkan => "ffmpeg-vulkan",
                Self::FfmpegVaapi => "ffmpeg-vaapi",
            }
        }

        pub(super) fn named(name: &str) -> Option<Self> {
            Self::ALL.into_iter().find(|class| class.name() == name)
        }

        fn once(self) {
            match self {
                Self::RawVulkan => {
                    raw_vulkan();
                }
                Self::FfmpegVulkan => ffmpeg_device(ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN),
                Self::FfmpegVaapi => ffmpeg_device(ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI),
            }
        }

        /// The child: `THREADS` threads, together each round, each round one line on stdout, with no lock.
        pub(super) fn child(self) {
            let barrier = Barrier::new(THREADS);
            let together = &barrier;
            thread::scope(|scope| {
                for _ in 0..THREADS {
                    scope.spawn(move || {
                        for _ in 0..ROUNDS_EACH {
                            together.wait();
                            self.once();
                            let _ = writeln!(std::io::stdout().lock(), "{ROUND}");
                        }
                    });
                }
            });
        }
    }

    /// An instance, its first GPU's device, both ended: the entries the loader scans drivers in and unloads them from.
    /// `None` where the loader or an instance would not be made, which ends the round.
    fn raw_vulkan() -> Option<()> {
        // SAFETY: loading the system's Vulkan loader runs its initialisers, which is what asking for Vulkan means.
        let entry = unsafe { ash::Entry::load() }.ok()?;
        let app = ash::vk::ApplicationInfo::default().api_version(ash::vk::API_VERSION_1_1);
        // SAFETY: the create info and what it points at outlive the call.
        let instance =
            unsafe { entry.create_instance(&ash::vk::InstanceCreateInfo::default().application_info(&app), None) }
                .ok()?;
        // SAFETY: the instance is live for the whole function.
        if let Ok(gpus) = unsafe { instance.enumerate_physical_devices() } {
            let with_a_queue = |gpu| {
                // SAFETY: `gpu` came from this instance.
                let families = unsafe { instance.get_physical_device_queue_family_properties(gpu) };
                families.iter().position(|family| family.queue_count > 0).map(|family| (gpu, family))
            };
            for (gpu, family) in gpus.into_iter().take(1).filter_map(with_a_queue) {
                let priorities = [1.0];
                let queues = [ash::vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family as u32)
                    .queue_priorities(&priorities)];
                let info = ash::vk::DeviceCreateInfo::default().queue_create_infos(&queues);
                // SAFETY: the create info and what it points at outlive the call.
                if let Ok(device) = unsafe { instance.create_device(gpu, &info, None) } {
                    // SAFETY: the device is this block's, and nothing made from it outlives it.
                    unsafe { device.destroy_device(None) };
                }
            }
        }
        // SAFETY: the instance is this function's, and nothing made from it outlives this call.
        unsafe { instance.destroy_instance(None) };
        Some(())
    }

    /// FFmpeg's own device of `kind`, made and released.
    fn ffmpeg_device(kind: ffmpeg_sys_next::AVHWDeviceType) {
        let mut context = std::ptr::null_mut();
        // SAFETY: `context` is a valid out-pointer; the device name and the options are null, which FFmpeg reads as
        // none.
        let made = unsafe {
            ffmpeg_sys_next::av_hwdevice_ctx_create(&raw mut context, kind, std::ptr::null(), std::ptr::null_mut(), 0)
        };
        if made >= 0 {
            // SAFETY: the reference made above, released once; `av_buffer_unref` nulls it.
            unsafe { ffmpeg_sys_next::av_buffer_unref(&raw mut context) };
        }
    }
}
