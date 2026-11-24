//! The window and its loop, made by hand because `iced::application` will not take a device of ours: one with the
//! import's features, on the adapter that decodes. A [`Scene`] decides what it shows; `ui::host` how it answers input.

mod gpu;
mod handler;
mod interface;
mod mend;
mod pane;
mod ready;
mod runner;
mod scene;
mod shape;
mod surface;
mod tray;
mod unhosted;

use std::sync::{Arc, Mutex};

use iced_winit::winit;
use iced_winit::winit::event_loop::{EventLoop, EventLoopProxy};

pub use gpu::graphics;
pub use scene::{Input, Redrawn, Scene};
pub use unhosted::{Failure, Unhosted};

use runner::{Runner, Stage};

/// Asks the window to draw again, from whatever thread has something new to show.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What a scene is given once the window is up: `wake` shows a new picture in the main window (nothing while hidden),
/// `wake_out` in the window of its own, `told` drains the scene even while hidden, and the GPU it paints with, for a
/// decoder to decode onto.
pub struct Started {
    /// Shows a new picture in the main window, in the layout it has; does nothing while the window is hidden.
    pub wake: Wake,
    /// The same in the window of its own, while there is one; does nothing otherwise.
    pub wake_out: Wake,
    /// Has the loop drain the scene, hidden or not.
    pub told: Wake,
    /// The GPU the window paints with, for a decoder to decode onto.
    pub painting: picture::Painting,
    /// The adapter the window paints with, as a debug report states it.
    pub graphics: report::Section,
}

/// What the event loop is woken for from another thread.
#[derive(Clone, Debug)]
pub enum Woke {
    /// A later launch or a click on a popup asked this window to come forward, with the desktop's token to take the
    /// focus where it gave one; a later launch is answered, through the reply, once the loop has handled it.
    Forward(Option<platform::activation::Activation>, Option<platform::instance::Reply>),
    /// A scene has something to drain.
    Told,
    /// A new picture waits for the window it names.
    Pictured(Which),
    /// The system closed the loop's own window, which no person sees: the run ends as a quit does.
    Closed,
    /// The GPU the window paints with was made on the start's thread.
    Started,
    /// The GPU device the windows paint with was lost.
    Lost,
}

/// Which of the scene's windows a new picture is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The main window.
    Main,
    /// The screen's own window.
    Out,
}

/// What the loop does after a redraw.
pub enum Next {
    /// Draw again when something asks.
    Wait,
    /// Draw again at once.
    Again,
    /// End the run.
    Exit,
}

/// Whether the scene is shown in its window or kept in the tray with no window at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Showing {
    /// In the main window.
    #[default]
    Window,
    /// In the tray: the windows are gone, the scene runs on.
    Tray,
}

/// Whether the main window closes now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Closing {
    /// It closes, and the run ends.
    Now,
    /// It stays open.
    #[default]
    Stay,
}

/// Redraws before a run ends by itself, from `SPIKE_FRAMES`: a window held up long enough to be photographed, or a
/// scripted run that ends on its own. Unset, a run ends when its window closes.
pub fn frames() -> Option<u32> {
    std::env::var("SPIKE_FRAMES").ok().and_then(|frames| frames.parse().ok())
}

/// A wake that sends `woke` to the loop from any thread.
fn waking(proxy: &EventLoopProxy<Woke>, woke: Woke) -> Wake {
    let proxy = Mutex::new(proxy.clone());
    Arc::new(move || {
        if let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(woke.clone());
        }
    })
}

/// The event loop a run hands to [`run`]; made first, so a later launch's ask can be wired to it, and its own window
/// kept from the system's close, which ends the run instead.
pub fn event_loop() -> Result<EventLoop<Woke>, winit::error::EventLoopError> {
    let event_loop = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    if platform::event_target::keep(move || {
        let _ = proxy.send_event(Woke::Closed);
    })
    .is_err()
    {
        report::note!(warn, "the event loop's own window could not be kept from the system's close");
    }
    Ok(event_loop)
}

/// Opens the window and runs the scene `make` builds in it until the window closes or the scene ends the run. A run
/// whose window cannot be hosted ends with the reason. Its place is kept in and read from `platform`'s folders.
pub fn run<S: Scene, E>(
    event_loop: EventLoop<Woke>,
    platform: platform::Platform,
    make: impl FnOnce(Started) -> Result<S, E>,
) -> Result<(), Failure> {
    let mut runner = Runner {
        make: Some(make),
        stage: Stage::Waiting,
        limit: frames(),
        proxy: event_loop.create_proxy(),
        platform,
        failed: None,
    };
    let ran = event_loop.run_app(&mut runner);
    // A desktop whose session ends closes its display under the loop, which then ends in an error with the scene
    // still running: it is told all the same, so its calls are left rather than dropped.
    if ran.is_err() {
        report::note!(warn, "the window's display went away; leaving every server");
    }
    runner.end();
    match (ran, runner.failed) {
        (_, Some(why)) => Err(Failure::Unhosted(why)),
        (Err(why), None) => Err(Failure::Loop(why)),
        (Ok(()), None) => Ok(()),
    }
}
