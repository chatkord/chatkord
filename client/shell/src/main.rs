#![deny(unsafe_code)]
// A released window opens no console beside it on Windows; a debug build keeps one for its log lines.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! The Chatkord window: getting in, every server's voice channels and a call, with what they ask of the world carried
//! out by `errands` on a runtime of its own. The token it is given is kept in the system keyring.

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::Arc;

use design::Scheme;
use errands::{Host, Runner};
use iced_wgpu::Renderer;
use iced_winit::core::time::Instant;
use iced_winit::core::{Element, Length, Size, Widget};
use platform::Platform;
use proto::video::Video;
use screens::{PlaceMessage, ServerMessage, Window, WindowMessage};
use shell::Scene;

mod frames;
mod held;
mod pictures;
mod stills;
use errands::mailbox::Inbox;
use held::{Held, Screens};
use shell::themed::Themed;
use shell::watched::{Feed, picture_widget};
use ui::picture::Painted;

/// The product's screens (getting in, every server's channels, a call) and what carries out what they ask.
struct Product {
    window: Window,
    runner: Runner<Held>,
    inbox: Inbox,
    themed: Themed,
    /// The call's pictures, which `ui::app` places where a call shows one.
    feeds: Feeds,
    /// What the tray icon's name and menu show now.
    tray_menu: screens::TrayMenu,
    /// The badge the launcher's count on the app's icon shows now.
    launcher: screens::Badge,
    /// What the device is kept awake for now.
    awake: screens::Awake,
    /// Where what the window draws stood when the tray, the badge and what is kept awake were last derived.
    derived: Option<screens::Revision>,
    /// The frames drawn this second, for a measurement to read.
    frames: frames::Tally,
    /// The files the host holds decoded, by id, which a view paints where a message shows one.
    handles: Box<ui::app::Handles<'static>>,
    /// Frames still to draw at once after a picture became ready, so the one that uploads it is followed.
    frames_owed: u8,
    /// The deadline for the first picture, until it is shown.
    starting: Option<shell::watchdog::Watch>,
    /// The deadline for the process to end once the window has closed.
    exiting: Arc<shell::watchdog::Exit>,
}

/// Frames drawn at once after a picture becomes ready.
const PICTURE_FRAMES: u8 = 2;

/// Where the window's picture widgets read each picture from.
struct Feeds {
    screens: Screens,
    own: Feed,
    own_camera: Feed,
}

impl Scene for Product {
    type Message = WindowMessage;
    type Theme = ui::Theme;

    fn preference(&self) -> design::Preference {
        self.window.look().theme
    }

    fn zoom(&self) -> design::Zoom {
        self.window.look().zoom
    }

    fn title(&self) -> String {
        channel::THIS.product.to_owned()
    }

    fn full_screen(&self) -> bool {
        self.window.full_screen().is_some()
    }

    fn full_screen_left(&mut self) {
        if let Some(full) = self.window.full_screen() {
            let leave = PlaceMessage::Call(screens::CallMessage::LeaveFullScreen);
            self.update(vec![WindowMessage::At(full.place, leave)]);
        }
    }

    fn room(&mut self, room: Size) {
        self.window.set_room(screens::frame::Room { width: room.width, height: room.height });
    }

    fn shown(&mut self, shown: bool) {
        // A run kept in the tray has no first picture: it is started once it is hidden.
        if !shown && let Some(starting) = self.starting.take() {
            starting.cancel();
        }
        if shown != self.window.shown() {
            if shown {
                report::note!(info, "the window shows");
            } else {
                report::note!(info, "the window is hidden: its pictures are lent nothing");
            }
        }
        self.runner.host().own().show(shown);
        self.runner.host().own_camera().show(shown);
        self.update(vec![WindowMessage::Shown(shown)]);
    }

    fn focused(&mut self, focused: bool) {
        self.update(vec![if focused { WindowMessage::Focused } else { WindowMessage::FocusLost }]);
    }

    fn typed(&mut self) {
        self.update(vec![WindowMessage::Input]);
    }

    fn files_hovered(&mut self, hovered: bool) {
        self.update(vec![WindowMessage::FilesHovered(hovered)]);
    }

    fn file_dropped(&mut self, path: std::path::PathBuf) {
        self.update(vec![WindowMessage::FilesDropped(vec![path])]);
    }

    fn prepare(&mut self, scheme: Scheme, motion: design::MotionPreference) {
        // The window is always Chatkord's colour; a server's colour is on its picture only.
        let look = self.window.look();
        self.themed.follow(scheme, design::Accent::DEFAULT, look.contrast, motion);
    }

    fn theme(&self) -> &ui::Theme {
        self.themed.theme()
    }

    fn view(&self, window: Size) -> Element<'_, WindowMessage, ui::Theme, Renderer> {
        self.runner.host().pictures.frame();
        ui::app::view_with::<Renderer>(&self.window, window, &*self.handles, &|painted| self.painted(painted))
    }

    fn update(&mut self, messages: Vec<WindowMessage>) -> bool {
        let drawn = self.window.revision();
        for message in messages {
            if let WindowMessage::At(
                _,
                PlaceMessage::Server(ServerMessage::FileReady(_) | ServerMessage::PictureReady(_)),
            ) = &message
            {
                self.frames_owed = PICTURE_FRAMES;
            }
            for todo in self.window.update(message, errands::moment(Instant::now())) {
                self.runner.start(todo);
            }
        }
        self.feeds.screens.follow_out(self.window.pop_out().map(|out| (out.screen.user_id, out.screen.video)));
        if self.window.moved_since(&mut self.derived) {
            self.follow_outside();
        }
        self.window.revision() != drawn
    }

    fn drain(&mut self) -> bool {
        let told = self.inbox.drain();
        self.update(told)
    }

    fn redrawn(&mut self, redrawn: shell::Redrawn, _: &mut shell::Input<'_>) -> shell::Next {
        if let Some(starting) = self.starting.take() {
            starting.cancel();
        }
        if let Some(second) = self.frames.count(redrawn.cpu, redrawn.laid_out, Instant::now()) {
            second.note();
        }
        // What the frame drew that the host does not hold, for each server that names it to ask for, once.
        let missed = self.runner.host().pictures.missed();
        if !missed.is_empty() {
            let told: Vec<WindowMessage> = self
                .window
                .places()
                .iter()
                .map(|place| {
                    WindowMessage::At(place.id(), PlaceMessage::Server(ServerMessage::Missing(missed.clone())))
                })
                .collect();
            if self.update(told) {
                return shell::Next::Again;
            }
        }
        // A picture the model just learned is held is uploaded by the frame that first paints it, and on the GPU painter
        // a growing atlas leaves that frame without it: frames follow until it shows.
        match self.frames_owed.checked_sub(1) {
            Some(left) => {
                self.frames_owed = left;
                shell::Next::Again
            }
            None => shell::Next::Wait,
        }
    }

    fn close_asked(&mut self) -> shell::window::Closing {
        self.update(vec![WindowMessage::CloseAsked]);
        self.runner.host().closing()
    }

    fn closing(&self) -> shell::window::Closing {
        self.runner.host().closing()
    }

    fn closed(&mut self) {
        // The connections part within their own bounds; what follows (threads that do not end) is held to this one.
        self.exiting.arm(shell::watchdog::EXIT_WITHIN, end_run);
        self.runner.part();
    }

    fn showing(&self) -> shell::window::Showing {
        self.runner.host().showing()
    }

    fn forwarded(&mut self) {
        self.runner.host().show();
    }

    fn repainted(&mut self, painting: picture::Painting) {
        self.runner.host().repaint(painting);
    }

    fn out(&self) -> Option<String> {
        let out = self.window.pop_out()?;
        let name = out.screen.name.to_string();
        let sentence = match out.screen.video {
            Video::Screen => words::Sentence::ScreenOf(name),
            Video::Camera => words::Sentence::CameraOf(name),
        };
        // The system draws the title, so it gets no marks our shaping hides.
        Some(self.window.words().said(&sentence))
    }

    fn view_out(&self, window: Size) -> Option<Element<'_, WindowMessage, ui::Theme, Renderer>> {
        ui::app::out_view_with::<Renderer>(&self.window, window, &|painted| self.painted(painted))
    }

    fn out_closed(&mut self) {
        if let Some(out) = self.window.pop_out() {
            let back = PlaceMessage::Call(screens::CallMessage::PopIn);
            self.update(vec![WindowMessage::At(out.place.id(), back)]);
        }
    }
}

impl Product {
    /// Shows the tray's menu, the launcher's badge and what is kept awake as the window has them now, each told only
    /// where it changed.
    fn follow_outside(&mut self) {
        let menu = self.window.tray_menu();
        if menu != self.tray_menu {
            self.runner.update_tray(menu.clone());
            self.tray_menu = menu;
        }
        let badge = self.window.badge();
        if badge != self.launcher {
            self.runner.update_launcher(badge);
            self.launcher = badge;
        }
        let awake = self.window.awake();
        if awake != self.awake {
            self.runner.update_awake(awake, &self.window.words().phrase(words::Phrase::KeepingAwake));
            self.awake = awake;
        }
    }

    /// What the host paints where a view shows a picture.
    fn painted(&self, painted: Painted) -> Element<'_, WindowMessage, ui::Theme, Renderer> {
        if let Painted::Picture { picture, radius, faint } = painted {
            return match self.runner.host().pictures.get(picture) {
                Some(handle) => iced_widget::image(handle)
                    .opacity(if faint { design::RAIL_DIM } else { 1.0 })
                    .border_radius(radius.px())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    ._boxed(),
                None => iced_widget::Space::new()._boxed(),
            };
        }
        if let Painted::Still(painting, radius) = painted {
            return match self.runner.host().stills.get(painting) {
                Some(handle) => iced_widget::image(handle)
                    .content_fit(iced_winit::core::ContentFit::Contain)
                    .border_radius(radius.px())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    ._boxed(),
                None => iced_widget::Space::new()._boxed(),
            };
        }
        let feed = match &painted {
            Painted::Screen { user_id, .. } => {
                let feed = Feed::default();
                if let Some(slot) = self.feeds.screens.slot(user_id, Video::Screen) {
                    feed.put(slot);
                }
                feed
            }
            Painted::Camera { user_id } => {
                let feed = Feed::default();
                if let Some(slot) = self.feeds.screens.slot(user_id, Video::Camera) {
                    feed.put(slot);
                }
                feed
            }
            Painted::OwnScreen(_) => self.feeds.own.clone(),
            Painted::OwnCamera(..) => self.feeds.own_camera.clone(),
            // Answered above.
            Painted::Still(..) | Painted::Picture { .. } => Feed::default(),
        };
        picture_widget(feed, painted)
    }
}

/// What a launch was asked to do.
#[derive(Debug, PartialEq, Eq)]
enum Run {
    /// Open the window.
    Window,
    /// Open as the desktop starts it at sign-in: in the tray, where that is chosen.
    AtSignIn,
    /// Take away all this device keeps for Chatkord: an uninstall's clean slate.
    Forget,
    /// Run the command line these words name, and no window.
    #[cfg(target_os = "linux")]
    CommandLine(Vec<OsString>),
}

/// The launch's arguments: none, or exactly `--forget` or `--autostart`; on Linux any other words are the command
/// line's, and elsewhere no launch.
fn run_from(args: impl Iterator<Item = OsString>) -> Option<Run> {
    let args: Vec<OsString> = args.collect();
    match args.as_slice() {
        [] => Some(Run::Window),
        [one] if one == "--forget" => Some(Run::Forget),
        [one] if one == platform::startup::AUTOSTART_ARGUMENT => Some(Run::AtSignIn),
        #[cfg(target_os = "linux")]
        _ => Some(Run::CommandLine(args)),
        #[cfg(not(target_os = "linux"))]
        _ => None,
    }
}

/// Why `--forget` left something in place.
#[derive(Debug, report::Public)]
enum Unforgotten {
    Running,
    Refused,
}

impl std::fmt::Display for Unforgotten {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let product = channel::THIS.product;
        match self {
            Unforgotten::Running => write!(f, "{product} is running, so nothing was removed."),
            Unforgotten::Refused => write!(f, "{product} could not remove everything it keeps on this device."),
        }
    }
}

/// Takes away the tokens, cache and folder, refused while a window runs.
fn forget(platform: &Platform) -> Result<(), Unforgotten> {
    match platform::instance::claim(platform, None) {
        Ok(Some(held)) => platform::forget::everything(platform, held).map_err(|_| Unforgotten::Refused),
        Ok(None) => Ok(()),
        Err(_) => Err(Unforgotten::Running),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(helped) = picture::run_hidden(&args).or_else(|| media::run_hidden(&args)) {
        return helped;
    }
    // The command line's watch and share open Vulkan devices too; where no state folder is named, a launch asks the
    // address again. Naming it reads the environment and starts no thread.
    let platform = Platform::system();
    #[expect(unsafe_code, reason = "the loader reads which drivers to load from the environment alone")]
    // SAFETY: no other thread exists: main has started none, and nothing before this line starts one.
    let drivers = platform.folder().map(|state| unsafe { picture::leave_out_sleeping(&state) });
    #[expect(unsafe_code, reason = "libva reads which VA-API drivers to try first from the environment alone")]
    // SAFETY: still no other thread exists.
    let va_drivers = unsafe { media::order_va_drivers(platform.folder().as_deref()) };
    #[expect(unsafe_code, reason = "the desktop's launch token is spent by this run, not handed to the ones it starts")]
    // SAFETY: still no other thread exists.
    let launched = unsafe { platform::activation::Activation::take_from_env() };
    let run = run_from(args.into_iter());
    #[cfg(target_os = "linux")]
    if let Some(Run::CommandLine(args)) = run {
        return cli::main(args.into_iter());
    }
    let log = shell::logging::start();
    if let Some(drivers) = drivers {
        drivers.note();
    }
    va_drivers.note();
    // The operating system's credential store; without one the session token is neither read nor
    // kept, so a saved sign-in is never offered and the next run has to type a password again.
    platform::keyring::use_system();
    let Some(run) = run else {
        report::note!(error, "usage: shell [--forget | --autostart]");
        return ExitCode::from(2);
    };
    if run == Run::Forget {
        // The uninstaller reads the exit code.
        return match forget(&platform) {
            Ok(()) => ExitCode::SUCCESS,
            Err(why) => {
                report::note!(error, "the data was not forgotten", why = why);
                ExitCode::FAILURE
            }
        };
    }
    // A run that shows no first picture in time says so and ends, with the window's first redraw as the end of the wait.
    let starting =
        shell::watchdog::start(shell::watchdog::START_WITHIN, || alert_start(words::failure::Start::TooSlow), end_run);
    // One window per person on this device: a later launch asks this one to come forward and ends. Held until
    // the process ends.
    let mut earlier = None;
    let mut exit = None;
    let started = std::time::SystemTime::now();
    let mut instance = match platform::instance::claim(&platform, launched.as_ref()) {
        Ok(held) => {
            match platform::logs::open(&platform, started, std::process::id()) {
                Ok(opened) => {
                    shell::logging::keep_this_run(opened.log, opened.fault);
                    earlier = opened.earlier;
                    exit = Some(opened.exit);
                }
                Err(_) => report::note!(warn, "this run's log could not be kept"),
            }
            held
        }
        Err(platform::instance::Elsewhere::Told) => {
            report::note!(info, "Chatkord is already running; its window was asked to come forward");
            return ExitCode::SUCCESS;
        }
        Err(platform::instance::Elsewhere::Silent) => {
            report::note!(warn, "Chatkord is already running");
            return ExitCode::FAILURE;
        }
        Err(platform::instance::Elsewhere::Unanswered) => {
            report::note!(warn, "Chatkord is already running, and its window did not answer");
            return ExitCode::FAILURE;
        }
    };
    // The run that holds the window takes away what a killed one left half written.
    let swept = platform::partial::swept(&platform, started);
    if swept > 0 {
        report::note!(info, "a killed run's partial files were taken away", files = swept);
    }
    if platform::launcher::keep(&platform).is_err() {
        report::note!(warn, "the launcher was not told which program this window is");
    }
    let event_loop = match shell::event_loop() {
        Ok(event_loop) => event_loop,
        Err(_) => return failed(),
    };
    if let Some(held) = &mut instance {
        let proxy = event_loop.create_proxy();
        let answered = held.answer(move |activation, reply| {
            let _ = proxy.send_event(shell::Woke::Forward(activation, Some(reply)));
        });
        if answered.is_err() {
            report::note!(warn, "a later launch cannot reach this window");
        }
    }
    let forward = std::sync::Mutex::new(event_loop.create_proxy());
    let forward: held::Forward = Box::new(move |activation| {
        if let Ok(proxy) = forward.lock() {
            let _ = proxy.send_event(shell::Woke::Forward(activation, None));
        }
    });
    // The package a restart starts, once this run has let its window go.
    let handover = errands::Handover::default();
    let exiting = Arc::<shell::watchdog::Exit>::default();
    let outcome = {
        shell::run(event_loop, platform.clone(), |started| {
            // What an earlier run learned about signing in is read before the first picture, on the sign-in's own
            // runtime; a first run reads nothing, and `screens` alone decides whether a sign-in is worth starting.
            let (mailbox, inbox) = errands::mailbox::mailbox(started.told);
            // A picture's arrival wakes the window, which paints it where it is.
            let slot = || {
                let slot = call::Slot::new();
                let wake = Arc::clone(&started.wake);
                slot.wake_with(move || wake());
                let feed = Feed::default();
                feed.put(slot.clone());
                (slot, feed)
            };
            let (own, own_feed) = slot();
            let (own_camera, own_camera_feed) = slot();
            let screens = Screens {
                slots: Arc::default(),
                wake: Arc::clone(&started.wake),
                wake_out: Arc::clone(&started.wake_out),
                out: Arc::default(),
            };
            let feeds = Feeds { screens: screens.clone(), own: own_feed, own_camera: own_camera_feed };
            let held = Held::new(screens, own, own_camera, started.painting, forward);
            let mut runner = Runner::new(platform.clone(), mailbox, held)?;
            runner.warm();
            // The notes of this run, which a debug report holds.
            runner.keep_log(log, earlier);
            if let Some(exit) = exit {
                runner.mark_exit(exit);
            }

            let rows = runner.block_on(platform::state::servers(&platform));
            let open = runner.block_on(platform::state::opened(&platform));
            let (window, mut todo) = Window::launch(errands::opened_at(rows), open.as_ref(), Instant::now());
            let chosen = runner.block_on(platform::voice::choice(&platform));
            let share = runner.block_on(platform::share::quality(&platform));
            let look = runner.block_on(platform::look::look(&platform));
            let volumes = runner.block_on(platform::volume::kept(&platform));
            let startup = runner.block_on(platform::startup::startup(&platform));
            let camera = runner.block_on(platform::camera::settings(&platform));
            let window = window.remembering(
                choices::Remembered {
                    microphone: chosen.microphone,
                    speaker: chosen.speaker,
                    next_call: choices::State { mic_muted: chosen.muted, deafened: chosen.deafened },
                    share,
                    look,
                    event_sounds: chosen.event_sounds,
                    message_sounds: chosen.message_sounds,
                    popups: chosen.popups,
                    processing: chosen.processing,
                    startup,
                    camera,
                    language: runner.block_on(platform::language::chosen(&platform)).map(choices::LanguageTag::from),
                    auto_update: runner.block_on(platform::auto_update::auto_update(&platform)),
                    emoji: runner.block_on(platform::emoji::picks(&platform)),
                },
                volumes,
            );
            let window = window.folding(runner.block_on(platform::folds::kept(&platform)));
            runner.starts_calls(window.next_call());
            let mut window = window;
            let machine = screens::Machine {
                system: platform::system::section(&platform::system::read())
                    .unwrap_or_else(|_| report::Section::absent("System")),
                graphics: started.graphics,
            };
            window.update(WindowMessage::Machine(machine), errands::moment(Instant::now()));
            let languages = WindowMessage::SystemLanguages(platform::language::preferred());
            window.update(languages, errands::moment(Instant::now()));
            let tray = runner.show_tray(window.tray_menu());
            window.update(WindowMessage::TrayShown(tray), errands::moment(Instant::now()));
            if runner.earlier_run_ended_badly() {
                todo.extend(window.update(WindowMessage::EarlierRunEnded, errands::moment(Instant::now())));
            }
            let zone = platform::zone::local();
            window.update(WindowMessage::Zone(zone.clone()), errands::moment(Instant::now()));
            runner.keep_the_day(zone);
            if run == Run::AtSignIn && startup.is_some_and(|startup| startup.starts_hidden(tray)) {
                runner.host().hide();
            }
            runner.find_updates(handover.clone());
            runner.watch_session_end();
            #[cfg(unix)]
            runner.watch_terminate();
            for asked in todo {
                runner.start(asked);
            }
            let themed = Themed::new(Scheme::Dark, design::Accent::DEFAULT, design::Contrast::Standard);
            let tray_menu = window.tray_menu();
            let pictures = runner.host().pictures.clone();
            let handles = Box::new(move |file| pictures.get(file));
            Ok::<_, std::io::Error>(Product {
                window,
                runner,
                inbox,
                themed,
                feeds,
                tray_menu,
                launcher: screens::Badge::default(),
                awake: screens::Awake::default(),
                derived: None,
                frames: frames::Tally::default(),
                handles,
                frames_owed: 0,
                starting,
                exiting: Arc::clone(&exiting),
            })
        })
    };
    match (handover.take(), instance) {
        // The new run claims the window this one held; started while it is held, it would knock here and end.
        (Some(next), held) => {
            let start = || {
                if let Err(why) = next.start() {
                    report::note!(warn, "the updated Chatkord did not start", why = why);
                }
            };
            match held {
                Some(held) => held.hand_over(start),
                None => start(),
            }
        }
        (None, held) => drop(held),
    }
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(shell::Failure::Unhosted(why)) => {
            // Its sentence waits for the person, however long they take to read it.
            exiting.disarm();
            unhosted(why)
        }
        Err(shell::Failure::Loop(_)) => failed(),
    }
}

/// The window could not be hosted: said in the log for the operator and in one sentence for the person, and the run
/// ends in failure.
fn unhosted(why: shell::Unhosted) -> ExitCode {
    report::operator!(error, "the window could not be hosted", why = why);
    alert_start(why.start());
    ExitCode::from(2)
}

/// Tells the person in one plain sentence, in their language, why the run cannot start.
fn alert_start(failed: words::failure::Start) {
    let words = words::Words::new(words::Language::negotiate(&platform::language::preferred()));
    let body = words.said(&words::Sentence::Phrase(words::failure::start(failed)));
    platform::alert::fatal(channel::THIS.product, &body);
}

/// Ends the process now, in failure: what a deadline does once its time has passed, with nothing left that could end
/// it any better.
#[expect(clippy::exit, reason = "a deadline passed with the run stuck: nothing is left to unwind")]
fn end_run() {
    std::process::exit(1);
}

/// The window's loop could not run: said, and the run ends in failure.
fn failed() -> ExitCode {
    // The loop's error is the windowing system's own text; only that the loop failed is kept.
    report::note!(error, "the window's loop could not run");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{Run, run_from};
    use std::ffi::OsString;

    fn run(args: &[&str]) -> Option<Run> {
        run_from(args.iter().map(OsString::from))
    }

    #[test]
    fn a_launch_opens_the_window_or_forgets() {
        assert_eq!(run(&[]), Some(Run::Window));
        assert_eq!(run(&["--forget"]), Some(Run::Forget));
        assert_eq!(run(&["--autostart"]), Some(Run::AtSignIn));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn on_linux_any_other_words_are_the_command_line() {
        for words in [
            &["register", "example.invalid", "ada"][..],
            &["member", "list", "example.invalid", "ada"],
            &["--forget", "--forget"],
            &["--quit"],
            &["https://example.invalid/join#code"],
        ] {
            let args = words.iter().map(OsString::from).collect();
            assert_eq!(run(words), Some(Run::CommandLine(args)), "{words:?}");
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn elsewhere_any_other_words_are_refused() {
        assert_eq!(run(&["--forget", "--forget"]), None);
        assert_eq!(run(&["--quit"]), None);
        assert_eq!(run(&["https://example.invalid/join#code"]), None);
        assert_eq!(run(&["register", "example.invalid", "ada"]), None);
    }
}
