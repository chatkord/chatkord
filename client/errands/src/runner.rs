//! What the window's screens ask of the world, carried out on a runtime of the runner's own and reported back by the
//! mailbox, keeping each server's signed-in connection for what the window shows next.

use crate::call::Running;
use crate::host::Host;
use crate::keeps::{Keeping, keeps};
use crate::mailbox::Mailbox;
use crate::place::Slot;
use crate::probe::{PROBE_COMMANDS, chime, run_camera_probe, run_probe};
use crate::startup::{StartupAsk, startup_asks};
use crate::update::{Handover, Work};
use link::Trust;
use screens::{Ask, PlaceId, ProbeAsk, UpdateAsk, UpdateMessage, WindowMessage};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

/// The sizes of the tray icon handed to the desktop, which picks the one nearest its own.
const TRAY_SIDES: [u32; 3] = [16, 32, 64];

/// How long the desktop has to show the tray icon before the window takes it for one that shows none.
const TRAY_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// How long the blocking work still running at the window's close is waited for before the runtime is let go.
const RUNTIME_ENDS_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// How long the desktop has to start watching the session's end, which is in place before the window shows.
const SESSION_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

// What a popup may hold is the desktop's to say, and the screens cut to it.
const _: () = {
    assert!(screens::POPUP_TITLE_MAX == platform::notify::TITLE_MAX);
    assert!(screens::POPUP_BODY_MAX == platform::notify::BODY_MAX);
    assert!(screens::POPUP_ID_MAX == platform::notify::ID_MAX);
};

/// Carries out what the screens ask of the world (the keyring, the network, a call, the desktop) for a host, which
/// owns what its toolkit draws.
pub struct Runner<H> {
    /// The folders this run keeps its choices, servers and pictures in.
    pub(crate) platform: platform::Platform,
    /// Said once every server's connection is parted, which the desktop's end of session waits for.
    parted: platform::session::Parted,
    /// This run's mark that it ended cleanly, written once it has parted: a system that ends the process at once after
    /// (Windows' end of session) would leave a mark written later unwritten.
    exit: Option<platform::logs::CleanExit>,
    pub(crate) runtime: Runtime,
    pub(crate) trust: Trust,
    pub(crate) mailbox: Mailbox,
    pub(crate) host: Arc<H>,
    pub(crate) slots: HashMap<PlaceId, Slot>,
    /// Where to send what a person asks of the one call being run, and at which server it is.
    pub(crate) running: Arc<Running>,
    /// The engine every call and the voice settings' probe run on, built at the first of them with the encoders
    /// surveyed beside it, so a run that never uses its voice holds neither (1.56 s the first time in a process on
    /// Machine A, 135 to 175 ms after).
    pub(crate) engines: Arc<call::Engines>,
    /// Where to send what is asked of the probe behind the voice settings, while one runs.
    probing: Option<mpsc::Sender<call::probe::Command>>,
    /// The same for the cameras' probe beside it.
    camera_probing: Option<mpsc::Sender<call::camera_probe::Command>>,
    /// How the next call starts, as the window last said.
    pub(crate) next_call: choices::State,
    /// Our own picker's listing behind each place's open picker.
    pub(crate) watching: HashMap<PlaceId, tokio::task::AbortHandle>,
    /// The tray icon, where the desktop shows one.
    tray: Option<Arc<platform::tray::TrayIcon>>,
    /// The count on the app's icon in the launcher, from the first one shown.
    launcher: Option<platform::launcher::Count>,
    /// What keeps the display or the machine awake, from the first call that needs it.
    awake: Option<platform::awake::Hold>,
    /// Where the desktop's clicks on our popups are heard: begun with the first popup, once.
    clicks: Arc<tokio::sync::OnceCell<()>>,
    /// The watch for this device going idle, while one runs.
    idle: Option<tokio::task::AbortHandle>,
    /// The window's one wake, which a later ask replaces: a timer nobody waits for any more is cancelled, never left to fire.
    waking: Option<tokio::task::AbortHandle>,
    /// Where the system's start is kept and read, one ask after another, so a read never answers from before a keep.
    startup: mpsc::UnboundedSender<StartupAsk>,
    /// Where what is kept for the next run goes, written one after another in the order asked.
    pub(crate) keeping: mpsc::UnboundedSender<Keeping>,
    /// Where the window's checks and downloads go, and the package a restart starts, once this run offers updates.
    updating: Option<Updating>,
    /// The recent notes of this run, which a debug report holds; none where the host keeps none.
    log: Option<report::Ring>,
    /// The log of the run before this one, found when this run's log was made; none when it left none.
    earlier: Option<platform::logs::Earlier>,
}

/// This run's update, once it offers one.
struct Updating {
    work: mpsc::UnboundedSender<Work>,
    /// The check by itself the window last scheduled.
    schedule: tokio::sync::watch::Sender<Option<std::time::SystemTime>>,
    target: update::Target,
    handover: Handover,
}

/// The engines a runner's calls share: on Windows the encoders are surveyed on the adapter the window paints with.
#[cfg(windows)]
fn engines<H: Host>(host: &Arc<H>, mailbox: &Mailbox) -> call::Engines {
    let host = Arc::clone(host);
    surveyed(call::Engines::painting_on(move || host.painting()), mailbox)
}

/// The engines a runner's calls share.
#[cfg(not(windows))]
fn engines<H: Host>(_host: &Arc<H>, mailbox: &Mailbox) -> call::Engines {
    surveyed(call::Engines::default(), mailbox)
}

/// `engines`, telling the window which encoders this device has for a screen once the voice is first used.
fn surveyed(engines: call::Engines, mailbox: &Mailbox) -> call::Engines {
    let mailbox = mailbox.clone();
    engines.at_first_use(move |encoders| {
        drop(crate::errand::spawn_here(report::word!("survey encoders"), async move {
            mailbox.tell(WindowMessage::Encoders(call::screen::encoders(encoders).await)).await;
        }));
    })
}

impl<H: Host> Runner<H> {
    /// A runner on a runtime of its own, keeping what it keeps in `platform`'s folders, telling the window by `mailbox`.
    pub fn new(platform: platform::Platform, mailbox: Mailbox, host: H) -> std::io::Result<Runner<H>> {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("signin").enable_all().build()?;
        Ok(Runner::on(runtime, platform, mailbox, host))
    }

    /// A runner on `runtime`, keeping what it keeps in `platform`'s folders, telling the window by `mailbox`.
    pub fn on(runtime: Runtime, platform: platform::Platform, mailbox: Mailbox, host: H) -> Runner<H> {
        let (startup, asks) = mpsc::unbounded_channel();
        crate::errand::spawn(
            &runtime,
            report::word!("serve startup asks"),
            startup_asks(asks, platform.clone(), mailbox.clone()),
        );
        let (keeping, kept) = mpsc::unbounded_channel();
        crate::errand::spawn(&runtime, report::word!("keep choices"), keeps(kept, platform.clone()));
        let host = Arc::new(host);
        let engines = Arc::new(engines(&host, &mailbox));
        Runner {
            platform,
            parted: platform::session::Parted::default(),
            exit: None,
            runtime,
            trust: Trust::system(),
            mailbox,
            engines,
            host,
            slots: HashMap::new(),
            running: Arc::default(),
            probing: None,
            camera_probing: None,
            next_call: choices::State::default(),
            watching: HashMap::new(),
            tray: None,
            launcher: None,
            awake: None,
            clicks: Arc::default(),
            idle: None,
            waking: None,
            startup,
            keeping,
            updating: None,
            log: None,
            earlier: None,
        }
    }

    /// The ring of recent notes the debug report reads, and the log of the run before this one if it left one.
    pub fn keep_log(&mut self, log: report::Ring, earlier: Option<platform::logs::Earlier>) {
        self.log = Some(log);
        self.earlier = earlier;
    }

    /// Marks this run as ended cleanly once it has parted at the window's close.
    pub fn mark_exit(&mut self, exit: platform::logs::CleanExit) {
        self.exit = Some(exit);
    }

    /// Whether the run before this one left a log and did not end cleanly, which the window offers its report for.
    pub fn earlier_run_ended_badly(&self) -> bool {
        self.earlier.as_ref().is_some_and(|earlier| !earlier.ended_cleanly())
    }

    /// The host it carries errands out for.
    pub fn host(&self) -> &H {
        &self.host
    }

    /// Runs `work` on the runner's runtime to its end, for what a launch reads before the first picture.
    pub fn block_on<F: Future>(&self, work: F) -> F::Output {
        self.runtime.block_on(work)
    }

    /// Connects the window's sounds now, so a chime does not wait when first wanted. The call engine and the encoder
    /// survey wait for the first voice use ([`call::Engines::at_first_use`]).
    pub fn warm(&self) {
        crate::errand::spawn(&self.runtime, report::word!("connect the sounds"), async move {
            let _ = sound::Sound::shared().ready().await;
        });
        // Windows has no picker a share must go through, so the window draws its own.
        #[cfg(windows)]
        self.mailbox.try_tell(WindowMessage::Chooser(screens::Chooser::Own));
    }

    /// How the next call starts, as the window opened on.
    pub fn starts_calls(&mut self, state: choices::State) {
        self.next_call = state;
    }

    /// Every server's connection parted as the window closes, together and each within `link::PARTING`: the process
    /// that ends next would drop their goodbyes unwritten, and a call's place goes with its socket. Then the run is
    /// marked as ended cleanly, and an end of session waiting on the parting is let go.
    pub fn part(&mut self) {
        let parting: Vec<_> = self
            .slots
            .values()
            .filter_map(|slot| crate::place::open_link(&slot.kept))
            .map(|link| {
                crate::errand::spawn(&self.runtime, report::word!("part a link"), async move { link.part().await })
            })
            .collect();
        self.runtime.block_on(async {
            for parted in parting {
                let _ = parted.await;
            }
        });
        if let Some(exit) = self.exit.take() {
            exit.mark();
        }
        self.parted.done();
        self.end_runtime();
    }

    /// Ends the runtime, waiting no longer than [`RUNTIME_ENDS_WITHIN`] for the blocking work still running on it: a
    /// runtime dropped waits for that for ever. What is asked of the runner after this runs on an idle one.
    fn end_runtime(&mut self) {
        match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(idle) => std::mem::replace(&mut self.runtime, idle).shutdown_timeout(RUNTIME_ENDS_WITHIN),
            Err(_) => report::note!(warn, "the runner's runtime could not be ended within its bound"),
        }
    }

    /// Puts the tray icon up named and with the menu `menu` holds, its picks told to the window; whether the desktop
    /// shows it. It waits on the desktop's answer, a few milliseconds, before the first picture, since a start into the
    /// tray needs it.
    pub fn show_tray(&mut self, menu: screens::TrayMenu) -> choices::Tray {
        let mailbox = self.mailbox.clone();
        let picked: platform::tray::Picked = Arc::new(move |pick| {
            mailbox.try_tell(WindowMessage::FromTray(pick));
        });
        let pictures = TRAY_SIDES
            .into_iter()
            .filter_map(|side| self.host.icon(side).map(|rgba| platform::tray::Picture { side, rgba }))
            .collect();
        let shown = self.runtime.block_on(async {
            tokio::time::timeout(TRAY_WAIT, platform::tray::TrayIcon::show(menu, pictures, picked)).await
        });
        match shown {
            Ok(Ok(icon)) => {
                self.tray = Some(Arc::new(icon));
                choices::Tray::Shown
            }
            Ok(Err(_)) | Err(_) => {
                report::note!(info, "the desktop shows no tray: closing the window quits");
                choices::Tray::Absent
            }
        }
    }

    /// Shows `menu`'s name and entries on the tray icon, if there is one.
    pub fn update_tray(&self, menu: screens::TrayMenu) {
        if let Some(tray) = self.tray.clone() {
            crate::errand::spawn(&self.runtime, report::word!("update tray"), async move { tray.update(menu).await });
        }
    }

    /// Shows the count `badge` makes on the app's icon in the launcher, whose entry is served from the first count on.
    pub fn update_launcher(&mut self, badge: screens::Badge) {
        let launcher = self.launcher.get_or_insert_with(|| {
            let (launcher, telling) = platform::launcher::Count::new();
            crate::errand::spawn(&self.runtime, report::word!("update launcher"), async move {
                if telling.await.is_err() {
                    report::note!(info, "the launcher was not told the count on the app's icon");
                }
            });
            launcher
        });
        launcher.show(badge);
    }

    /// Keeps what `awake` says awake, naming `reason` to the system, as the first call that needs it begins the hold.
    pub fn update_awake(&mut self, awake: screens::Awake, reason: &str) {
        let hold = self.awake.get_or_insert_with(|| {
            let (hold, keeping) = platform::awake::Hold::new();
            crate::errand::spawn(&self.runtime, report::word!("update awake"), async move {
                if keeping.await.is_err() {
                    report::note!(info, "the system was not asked to stay awake");
                }
            });
            hold
        });
        hold.keep(awake, reason);
    }

    /// Finds whether this build and run can update themselves, before the first picture; where they can, tells the
    /// window so and carries out its update asks from then on, a restart putting the package to start in `handover`.
    pub fn find_updates(&mut self, handover: Handover) {
        match self.runtime.block_on(crate::update::find(&self.trust)) {
            Ok(found) => {
                let (work, asks) = mpsc::unbounded_channel();
                let target = found.target().clone();
                crate::errand::spawn(
                    &self.runtime,
                    report::word!("serve updates"),
                    crate::update::serve(found, asks, self.mailbox.clone()),
                );
                let (schedule, scheduled) = tokio::sync::watch::channel(None);
                let wall_clock = std::time::SystemTime::now;
                crate::errand::spawn(
                    &self.runtime,
                    report::word!("tell when update is due"),
                    crate::update::tell_when_due(scheduled, wall_clock, self.mailbox.clone()),
                );
                self.updating = Some(Updating { work, schedule, target, handover });
                let offered = UpdateMessage::Offered(std::time::SystemTime::now());
                self.mailbox.try_tell(WindowMessage::Update(offered));
            }
            Err(why) => report::note!(info, "this run offers no updates", why = why),
        }
    }

    /// Tells the window the day it is in `zone` now, and again as each day begins there.
    pub fn keep_the_day(&self, zone: chat::TimeZone) {
        let mailbox = self.mailbox.clone();
        crate::errand::spawn(&self.runtime, report::word!("keep the day"), async move {
            loop {
                let (today, until_tomorrow) = platform::zone::today(&zone);
                mailbox.tell(WindowMessage::Today(today)).await;
                tokio::time::sleep(until_tomorrow).await;
            }
        });
    }

    /// Tells the window when the desktop session starts to end, so a close it sends then quits at once; in place
    /// before the window shows, so even a close at once (an installer's) is told apart.
    pub fn watch_session_end(&self) {
        let mailbox = self.mailbox.clone();
        let told = move |end| {
            mailbox.try_tell(match end {
                platform::session::End::Asked => WindowMessage::SessionEnding,
                platform::session::End::Happening => WindowMessage::Ended,
            });
        };
        let parted = self.parted.clone();
        let watching = self.runtime.block_on(async {
            tokio::time::timeout(SESSION_WAIT, platform::session::watch_end(told, parted, link::PARTING)).await
        });
        match watching {
            Ok(Ok(watching)) => drop(crate::errand::spawn(&self.runtime, report::word!("watch session end"), watching)),
            Ok(Err(_)) | Err(_) => report::note!(info, "the desktop does not say when its session ends"),
        }
    }

    /// Tells the window when the system asks the process to end (a terminate signal, which a session's end sends its
    /// programs), so it quits at once, every server parted.
    #[cfg(unix)]
    pub fn watch_terminate(&self) {
        let mailbox = self.mailbox.clone();
        crate::errand::spawn(&self.runtime, report::word!("watch terminate"), async move {
            let Ok(mut terminate) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else {
                return report::note!(
                    warn,
                    "a terminate signal could not be watched: it ends Chatkord without parting"
                );
            };
            if terminate.recv().await.is_some() {
                report::note!(info, "asked to end by the system");
                mailbox.try_tell(WindowMessage::Ended);
            }
        });
    }

    /// Carries out what the window asks, its answer told by the mailbox.
    pub fn start(&mut self, todo: Ask) {
        let mailbox = self.mailbox.clone();
        match todo {
            Ask::At(id, asked) => self.at(id, asked),
            Ask::Probe(asked) => self.probe(asked),
            Ask::ReadStartup => drop(self.startup.send(StartupAsk::Read)),
            Ask::ReadNotices => {
                crate::errand::spawn(&self.runtime, report::word!("read notices"), async move {
                    let read = platform::package::notices().await.and_then(|read| {
                        read.inspect_err(|why| report::note!(warn, "About Chatkord shows no notices", why = why)).ok()
                    });
                    mailbox.tell(WindowMessage::NoticesRead(read)).await;
                });
            }
            Ask::ReadLog => {
                let recent = self.log.as_ref().map(report::Ring::recent).unwrap_or_default();
                let earlier = self.earlier.clone();
                crate::errand::spawn(&self.runtime, report::word!("read log"), async move {
                    let previous = match earlier {
                        Some(earlier) => earlier.read().await,
                        None => None,
                    };
                    let logs = screens::ReportLogs { recent, previous };
                    mailbox.tell(WindowMessage::Report(screens::ReportMessage::Read(logs))).await;
                });
            }
            Ask::SaveReport(download) => {
                crate::errand::spawn(&self.runtime, report::word!("save report"), async move {
                    let saved = crate::debug_report::save(download).await;
                    mailbox.tell(WindowMessage::Report(screens::ReportMessage::Saved(saved))).await;
                });
            }
            Ask::Keep(keep) => {
                if let screens::Keep::Startup(startup) = keep {
                    return drop(self.startup.send(StartupAsk::Keep(startup)));
                }
                drop(self.keeping.send(Keeping::Choice(keep)));
            }
            Ask::NextCall(state) => {
                self.next_call = state;
                drop(self.keeping.send(Keeping::NextCall(state)));
            }
            Ask::Chime(played) => {
                let platform = self.platform.clone();
                crate::errand::spawn(&self.runtime, report::word!("chime"), async move {
                    if let Err(refused) = chime(&platform, played).await {
                        report::note!(warn, "a sound of the window did not play", chime = played, why = refused);
                    }
                });
            }
            Ask::RememberOpen(url) => drop(self.keeping.send(Keeping::Open(url))),
            Ask::Quit => self.host.quit(),
            Ask::Hide => self.host.hide(),
            Ask::Show => self.host.show(),
            Ask::Update(asked) => self.update(asked),
            Ask::Popup { id, title, body } => self.popup(&id, title, body),
            Ask::Withdraw { id } => {
                let id = id.wire();
                crate::errand::spawn(&self.runtime, report::word!("withdraw"), async move {
                    let _ = platform::notify::withdraw(&id).await;
                });
            }
            Ask::Idle(screens::IdleWatch::Start(run)) => self.watch_idle(run),
            Ask::Idle(screens::IdleWatch::Stop) => {
                if let Some(watch) = self.idle.take() {
                    watch.abort();
                }
            }
            Ask::Levels { place, shown } => {
                if let Some(asks) = crate::call::held_asks(&self.running, place) {
                    asks.act(&self.runtime, call::room::Command::Levels(shown));
                }
            }
            Ask::WakeAt(when) => {
                let wake = crate::errand::spawn(&self.runtime, report::word!("wake at"), async move {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(when)).await;
                    mailbox.tell(WindowMessage::Tick).await;
                });
                if let Some(earlier) = self.waking.replace(wake.abort_handle()) {
                    earlier.abort();
                }
            }
        }
    }

    /// Shows a desktop popup under `id`, in place of one shown under it before, and hears its click: the window comes
    /// forward, as peer apps' do, and is told which popup was clicked. A desktop with no notification service shows
    /// none.
    fn popup(&self, id: &screens::PopupId, title: String, body: String) {
        let popup = match platform::notify::Popup::new(id.wire(), title, body) {
            Ok(popup) => popup,
            Err(refused) => return report::note!(warn, "a popup was not made", why = refused),
        };
        let (listening, mailbox, host) = (Arc::clone(&self.clicks), self.mailbox.clone(), Arc::clone(&self.host));
        crate::errand::spawn(&self.runtime, report::word!("popup"), async move {
            listening
                .get_or_init(|| async {
                    match platform::notify::clicks().await {
                        Ok(mut heard) => {
                            crate::errand::spawn_here(report::word!("tell popup clicks"), async move {
                                while let Some(click) = heard.next().await {
                                    let Some(id) = screens::PopupId::from_wire(&click.id) else { continue };
                                    host.come_forward(click.activation);
                                    mailbox.tell(WindowMessage::PopupClicked(id)).await;
                                }
                            });
                        }
                        Err(platform::Unavailable) => report::note!(info, "the desktop tells no clicks on popups"),
                    }
                })
                .await;
            if platform::notify::show(popup).await.is_err() {
                report::note!(info, "the desktop showed no popup");
            }
        });
    }

    /// Starts telling the window when this device goes idle and comes back, in place of a watch begun before.
    fn watch_idle(&mut self, run: screens::IdleRun) {
        if let Some(before) = self.idle.take() {
            before.abort();
        }
        let mailbox = self.mailbox.clone();
        let task = crate::errand::spawn(&self.runtime, report::word!("watch idle"), async move {
            let (to, mut heard) = mpsc::channel(4);
            // Held until this task ends or is aborted, which stops the watch.
            let Ok(_watch) = platform::idle::start(to, |state| state).await else {
                report::note!(info, "the desktop tells no idle time: input to this window stands in for it");
                return mailbox.tell(WindowMessage::IdleSourceLost(run)).await;
            };
            report::note!(debug, "idle watch running");
            while let Some(state) = heard.recv().await {
                report::note!(debug, "the desktop told", state = state);
                let idleness = match state {
                    platform::idle::State::Idle => screens::Idleness::Idle,
                    platform::idle::State::Active => screens::Idleness::Active,
                };
                mailbox.tell(WindowMessage::IdleChanged(run, idleness)).await;
            }
            report::note!(info, "the desktop's idle time ended: input to this window stands in for it");
            mailbox.tell(WindowMessage::IdleSourceLost(run)).await;
        });
        self.idle = Some(task.abort_handle());
    }

    /// Schedules the check by itself, or hands a check or a download to the update's errand; a restart quits this run,
    /// which then starts the package in place. Nothing is asked of a run that offers no update.
    fn update(&self, asked: UpdateAsk) {
        let Some(updating) = &self.updating else { return };
        match asked {
            UpdateAsk::Schedule(due) => drop(updating.schedule.send_replace(due)),
            UpdateAsk::Check => drop(updating.work.send(Work::Check)),
            UpdateAsk::Download => drop(updating.work.send(Work::Download)),
            UpdateAsk::Restart => {
                updating.handover.hand(updating.target.clone());
                self.host.quit();
            }
        }
    }

    /// Starts, steers or stops the probe behind the voice settings, of which there is one at most.
    fn probe(&mut self, asked: ProbeAsk) {
        use call::probe::Command;
        match asked {
            ProbeAsk::Open { microphone, speaker, processing } => {
                self.tell_probe(Command::Close);
                let (commands, queued) = mpsc::channel(PROBE_COMMANDS);
                self.probing = Some(commands.clone());
                let level_every = crate::call::LEVEL_EVERY;
                let chain = self.engines.chain_watch();
                let open = call::probe::Open { microphone, speaker, processing, level_every, chain };
                let (engines, mailbox) = (Arc::clone(&self.engines), self.mailbox.clone());
                crate::errand::spawn(
                    &self.runtime,
                    report::word!("run the probe"),
                    run_probe(engines, open, commands, queued, mailbox),
                );
                self.tell_camera_probe(call::camera_probe::Command::Close);
                let (commands, queued) = mpsc::channel(PROBE_COMMANDS);
                self.camera_probing = Some(commands);
                let (own, mailbox) = (self.host.own_camera(), self.mailbox.clone());
                crate::errand::spawn(
                    &self.runtime,
                    report::word!("run the camera probe"),
                    run_camera_probe(own, queued, mailbox),
                );
            }
            ProbeAsk::Close => {
                self.tell_probe(Command::Close);
                self.probing = None;
                self.tell_camera_probe(call::camera_probe::Command::Close);
                self.camera_probing = None;
            }
            ProbeAsk::TestCamera(ask) => self.tell_camera_probe(call::camera_probe::Command::Test(ask)),
            ProbeAsk::EndCameraTest => self.tell_camera_probe(call::camera_probe::Command::EndTest),
            ProbeAsk::Microphone(name) => self.tell_probe(Command::Microphone(name)),
            ProbeAsk::Speaker(name) => self.tell_probe(Command::Speaker(name)),
            ProbeAsk::PlayTone => self.tell_probe(Command::PlayTone),
            ProbeAsk::Processing(processing) => self.tell_probe(Command::Processing(processing)),
            ProbeAsk::Test => self.tell_probe(Command::Test),
            ProbeAsk::EndTest => self.tell_probe(Command::EndTest),
        }
    }

    /// Sends the probe one command, in order with the ones before it; one that finds the queue full (never in practice)
    /// waits its turn on the runtime.
    fn tell_probe(&self, command: call::probe::Command) {
        let Some(commands) = &self.probing else { return };
        if let Err(mpsc::error::TrySendError::Full(command)) = commands.try_send(command) {
            let commands = commands.clone();
            crate::errand::spawn(&self.runtime, report::word!("tell probe"), async move {
                let _ = commands.send(command).await;
            });
        }
    }

    /// Sends the cameras' probe one command, as [`Runner::tell_probe`] does the sound's.
    fn tell_camera_probe(&self, command: call::camera_probe::Command) {
        let Some(commands) = &self.camera_probing else { return };
        if let Err(mpsc::error::TrySendError::Full(command)) = commands.try_send(command) {
            let commands = commands.clone();
            crate::errand::spawn(&self.runtime, report::word!("tell camera probe"), async move {
                let _ = commands.send(command).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Runner;
    use crate::host::{Host, Still};
    use crate::mailbox::{Inbox, mailbox};
    use proto::video::Video;
    use screens::{Ask, Opening, PlaceAsk, PlaceId, Server, UpdateAsk, UpdateMessage, Window, WindowMessage};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use terms::ServerUrl;
    use tokio::runtime::{Builder, Runtime};

    /// How long a test waits for what an errand writes or says before taking it for nothing.
    const WAIT: Duration = Duration::from_secs(5);

    /// A host with nothing to paint on, recording what the window was asked to become.
    #[derive(Default)]
    struct Recording {
        stages: Mutex<Vec<&'static str>>,
    }

    impl Recording {
        fn record(&self, stage: &'static str) {
            self.stages.lock().unwrap().push(stage);
        }
    }

    impl Host for Recording {
        fn slot(&self, _: &terms::MemberId, _: Video) -> call::Slot {
            call::Slot::new()
        }
        fn clear_slots(&self) {
            self.record("clear slots");
        }
        fn own_screen(&self) -> call::Slot {
            call::Slot::new()
        }
        fn own_camera(&self) -> call::Slot {
            call::Slot::new()
        }
        fn painting(&self) -> picture::Painting {
            picture::Painting::default()
        }
        fn hold_picture(
            &self,
            _: screens::PlaceId,
            _: proto::files::FileId,
            _: portrait::Square,
        ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
            Vec::new()
        }
        fn hold_file(
            &self,
            _: screens::PlaceId,
            _: proto::files::FileId,
            _: portrait::Decoded,
        ) -> Vec<(screens::PlaceId, proto::files::FileId)> {
            Vec::new()
        }
        fn clear_pictures(&self, _: screens::PlaceId) {}
        fn hold_stills(&self, _: Vec<Still>) {}
        fn icon(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn quit(&self) {
            self.record("quit");
        }
        fn hide(&self) {
            self.record("hide");
        }
        fn show(&self) {
            self.record("show");
        }
        fn come_forward(&self, _: Option<platform::activation::Activation>) {
            self.record("come forward");
        }
    }

    fn runner(runtime: Runtime, platform: platform::Platform) -> (Runner<Recording>, Inbox) {
        let (told, inbox) = mailbox(Arc::new(|| {}));
        (Runner::on(runtime, platform, told, Recording::default()), inbox)
    }

    /// The id a window gives its one server.
    fn place() -> PlaceId {
        let url = ServerUrl::parse("wss://example.invalid/v1").unwrap();
        let server = Server { url, name: None, colour: None, picture: None };
        let opened = Window::launch(vec![Opening::Server(server)], None, std::time::Instant::now());
        opened.0.places()[0].id()
    }

    #[test]
    fn a_keep_is_written_to_its_section() {
        let folder = std::env::temp_dir().join(format!("chatkord-errands-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let platform = platform::Platform::default().config_in(folder.clone());
        let (mut runner, _inbox) =
            runner(Builder::new_current_thread().enable_all().build().unwrap(), platform.clone());
        let look = choices::Look { contrast: design::Contrast::More, ..choices::Look::default() };
        assert_ne!(runner.block_on(platform::look::look(&platform)), look, "a fresh folder keeps the default");

        runner.start(Ask::Keep(screens::Keep::Look(look)));
        let kept = runner.block_on(async {
            tokio::time::timeout(WAIT, async {
                while platform::look::look(&platform).await != look {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
        });
        let _ = std::fs::remove_dir_all(&folder);
        assert!(kept.is_ok(), "the look was not kept where the next run reads it");
    }

    #[test]
    fn a_run_after_one_that_left_no_mark_says_it_ended_badly() {
        let folder = std::env::temp_dir().join(format!("chatkord-errands-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let platform = platform::Platform::default().logs_in(folder.clone());
        let at = |seconds| std::time::UNIX_EPOCH + Duration::from_secs(seconds);
        let open = |seconds, pid| platform::logs::open(&platform, at(seconds), pid).unwrap();
        open(1, 1).exit.mark();
        let after_clean = open(2, 2);
        let after_crash = open(3, 3);
        let ring = || report::Ring::new(std::time::SystemTime::now);
        let (mut runner, _inbox) =
            runner(Builder::new_current_thread().enable_all().build().unwrap(), platform.clone());
        assert!(!runner.earlier_run_ended_badly(), "no earlier log is no offer");
        runner.keep_log(ring(), after_clean.earlier);
        assert!(!runner.earlier_run_ended_badly(), "the run before it marked its exit");
        runner.keep_log(ring(), after_crash.earlier);
        assert!(runner.earlier_run_ended_badly(), "the run before it left no mark");
        runner.mark_exit(after_crash.exit);
        runner.part();
        let next = open(4, 4);
        assert!(next.earlier.is_some_and(|earlier| earlier.ended_cleanly()), "a run that parted is marked clean");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn warming_builds_no_engine_and_surveys_no_encoder() {
        let (mut runner, mut inbox) =
            runner(Builder::new_current_thread().enable_all().build().unwrap(), platform::Platform::default());
        let used = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let using = Arc::clone(&used);
        runner.engines = Arc::new(call::Engines::default().at_first_use(move |_| {
            using.store(true, std::sync::atomic::Ordering::Relaxed);
        }));
        runner.warm();
        runner.block_on(async { tokio::time::sleep(Duration::from_millis(100)).await });
        assert!(!used.load(std::sync::atomic::Ordering::Relaxed), "the engine waits for the first voice use");
        let told = inbox.drain();
        assert!(!told.iter().any(|told| matches!(told, WindowMessage::Encoders(_))), "{told:?}");
    }

    #[test]
    fn a_wake_tells_the_window_tick_at_its_time() {
        let paused = Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
        let (mut runner, mut inbox) = runner(paused, platform::Platform::default());
        let after = Duration::from_secs(5);
        let at = runner.block_on(async { tokio::time::Instant::now() }).into_std() + after;

        runner.start(Ask::WakeAt(at));
        runner.block_on(async { tokio::time::sleep(after - Duration::from_millis(1)).await });
        assert!(inbox.drain().is_empty(), "nothing is told before its time");
        runner.block_on(async { tokio::time::sleep(Duration::from_millis(2)).await });
        assert!(matches!(inbox.drain()[..], [WindowMessage::Tick]), "a tick at its time");
        assert!(runner.host().stages.lock().unwrap().is_empty(), "and the window asked to be nothing else");
    }

    #[test]
    fn a_later_wake_replaces_the_earlier_one_and_only_the_last_asked_fires() {
        let paused = Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
        let (mut runner, mut inbox) = runner(paused, platform::Platform::default());
        let now = runner.block_on(async { tokio::time::Instant::now() }).into_std();
        runner.start(Ask::WakeAt(now + Duration::from_secs(5)));
        runner.start(Ask::WakeAt(now + Duration::from_secs(9)));
        runner.block_on(async { tokio::time::sleep(Duration::from_secs(6)).await });
        assert!(inbox.drain().is_empty(), "the earlier wake was replaced, so it did not fire");
        runner.block_on(async { tokio::time::sleep(Duration::from_secs(4)).await });
        assert!(matches!(inbox.drain()[..], [WindowMessage::Tick]), "the one wake fires once");
    }

    #[test]
    fn a_later_wait_for_a_reconnect_replaces_the_earlier_one_of_that_server() {
        let paused = Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
        let (mut runner, mut inbox) = runner(paused, platform::Platform::default());
        let id = place();
        runner.start(Ask::At(id, screens::PlaceAsk::Wait { attempt: 1, delay: Duration::from_secs(5) }));
        runner.start(Ask::At(id, screens::PlaceAsk::Wait { attempt: 2, delay: Duration::from_secs(9) }));
        runner.block_on(async { tokio::time::sleep(Duration::from_secs(10)).await });
        let told = inbox.drain();
        assert!(
            matches!(
                &told[..],
                [WindowMessage::At(at, screens::PlaceMessage::Session(screens::SessionMessage::Reconnect(2)))]
                    if *at == id
            ),
            "only the wait asked last is carried out: {told:?}"
        );
    }

    #[test]
    fn a_run_not_from_a_package_offers_no_update_and_a_restart_asked_of_it_does_nothing() {
        let (mut runner, mut inbox) =
            runner(Builder::new_current_thread().enable_all().build().unwrap(), platform::Platform::default());
        let handover = crate::Handover::default();
        runner.find_updates(handover.clone());
        assert!(inbox.drain().is_empty(), "the window is not told updates are offered");
        for asked in
            [UpdateAsk::Schedule(Some(std::time::SystemTime::UNIX_EPOCH)), UpdateAsk::Check, UpdateAsk::Restart]
        {
            runner.start(Ask::Update(asked));
        }
        runner.block_on(async { tokio::time::sleep(Duration::from_millis(50)).await });
        assert!(inbox.drain().is_empty());
        assert!(runner.host().stages.lock().unwrap().is_empty(), "nothing quit");
        assert!(handover.take().is_none(), "nothing to start after");
    }

    /// A wall clock that runs with the paused runtime's, from 1 800 000 000 s after the epoch, jumped by `slept` as a
    /// device's is on waking: the runtime's clock does not run while it sleeps.
    fn wall_clock(slept: Arc<Mutex<Duration>>) -> impl Fn() -> std::time::SystemTime {
        let start = tokio::time::Instant::now();
        move || {
            let slept = *slept.lock().unwrap();
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000) + start.elapsed() + slept
        }
    }

    /// The window is told the clock only once the check scheduled is due, never for a reading that finds nothing due
    /// and never while nothing is scheduled, so such a reading draws nothing; a wake from sleep is told within a minute.
    #[test]
    fn the_window_is_told_the_clock_only_when_the_check_scheduled_is_due() {
        let paused = Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
        let (runner, mut inbox) = runner(paused, platform::Platform::default());
        let slept = Arc::new(Mutex::new(Duration::ZERO));
        let (schedule, scheduled) = tokio::sync::watch::channel(None);
        let clock = runner.block_on(async { wall_clock(Arc::clone(&slept)) });
        let at = |after: Duration| std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000) + after;
        crate::errand::spawn(
            &runner.runtime,
            report::word!("tell when due"),
            crate::update::tell_when_due(scheduled, clock, runner.mailbox.clone()),
        );
        let pass = |by: Duration| runner.block_on(async { tokio::time::sleep(by).await });
        let hour = Duration::from_secs(60 * 60);
        pass(10 * hour);
        assert!(inbox.drain().is_empty(), "nothing scheduled: nothing told");

        schedule.send_replace(Some(at(14 * hour)));
        pass(4 * hour - Duration::from_secs(1));
        assert!(inbox.drain().is_empty(), "about 240 readings, none due, none told");
        pass(Duration::from_secs(2));
        let told = inbox.drain();
        let due = at(14 * hour);
        assert!(matches!(&told[..], [WindowMessage::Update(UpdateMessage::Clock(now))] if *now == due), "{told:?}");
        schedule.send_replace(None);
        pass(hour);
        assert!(inbox.drain().is_empty(), "the check under way scheduled none");

        schedule.send_replace(Some(at(19 * hour)));
        pass(Duration::from_secs(5));
        *slept.lock().unwrap() = 8 * hour;
        pass(screens::CLOCK_EVERY);
        let told = inbox.drain();
        assert!(matches!(&told[..], [WindowMessage::Update(UpdateMessage::Clock(_))]), "woken: {told:?}");
    }

    #[test]
    fn a_cancel_stops_the_running_errand() {
        let (mut runner, mut inbox) =
            runner(Builder::new_current_thread().enable_all().build().unwrap(), platform::Platform::default());
        let listener = runner.block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await }).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (reached, release) = (Arc::new(tokio::sync::Notify::new()), Arc::new(tokio::sync::Notify::new()));
        // The first connection is held until released; every one after is closed at once, so a live errand ends.
        let (reaching, releasing) = (Arc::clone(&reached), Arc::clone(&release));
        runner.block_on(async {
            drop(crate::errand::spawn_here(report::word!("a cancel test"), async move {
                if let Ok((held, _)) = listener.accept().await {
                    reaching.notify_one();
                    releasing.notified().await;
                    drop(held);
                }
                while let Ok((closed, _)) = listener.accept().await {
                    drop(closed);
                }
            }));
        });
        let id = place();
        let address = terms::Address::typed(&format!("127.0.0.1:{port}")).unwrap();
        let find = screens::Errand::Find { address, code: None, pin: None };

        runner.start(Ask::At(id, PlaceAsk::Door(screens::DoorAsk::Go(find))));
        runner.block_on(async { tokio::time::timeout(WAIT, reached.notified()).await }).expect("the errand runs");
        runner.start(Ask::At(id, PlaceAsk::Door(screens::DoorAsk::Cancel)));
        release.notify_one();
        runner.block_on(async { tokio::time::sleep(Duration::from_secs(1)).await });
        let told = inbox.drain();
        assert!(told.is_empty(), "a cancelled errand told the window {told:?}");
    }

    mod server {
        use super::*;

        #[test]
        fn a_call_whose_task_panics_ends_and_its_connection_is_closed_to_be_made_again() {
            const ADA: &str = "ada";
            let runtime = Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            let (_server, link, _events) = runtime.block_on(async {
                let (server, invites) = testserver::Server::start("errands_call_panics", 1).await;
                let url = ServerUrl::parse(&server.url()).expect("a loopback address");
                let (link, events) =
                    link::Link::connect(&url, &link::Trust::system(), "call_panics", "test").await.expect("connected");
                let password = terms::Secret::new("a test password that is long enough".into());
                let invite = terms::Secret::new(invites[0].clone());
                link.register(ADA, None, &password, &invite).await.expect("registered");
                (server, link, events)
            });
            assert!(!link.is_closed(), "signed in");
            let host = Recording::default();
            let call = async { panic!("a step of the call") };
            let finished = runtime.block_on(crate::call::kept_call(call, &host, Some(link.clone())));
            assert!(matches!(finished, screens::CallReturn::Faulted), "{finished:?}");
            assert!(link.is_closed(), "the server would keep the call's place");
            assert_eq!(*host.stages.lock().unwrap(), ["clear slots"], "its pictures go with it");
        }
    }

    #[test]
    fn a_runner_with_a_hung_blocking_task_ends_within_its_bound() {
        let runtime = Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (mut runner, _inbox) = runner(runtime, platform::Platform::default());
        let (started, running) = std::sync::mpsc::channel();
        runner.block_on(async {
            drop(crate::errand::blocking(report::word!("hangs for ever"), move || {
                let _ = started.send(());
                loop {
                    std::thread::park();
                }
            }));
        });
        running.recv_timeout(WAIT).expect("the blocking task began");
        let (ended, over) = std::sync::mpsc::channel();
        // Its own thread: a runner that waits for ever must fail this test, not hang it.
        std::thread::spawn(move || {
            runner.part();
            drop(runner);
            let _ = ended.send(());
        });
        assert!(over.recv_timeout(WAIT).is_ok(), "the runner did not end within its bound");
    }
}
