//! Where a run is, from its window not made yet to closed, and what moves it between stages.

use std::sync::{Arc, Mutex};

use design::Appearance;
use iced_winit::core::Size;
use iced_winit::winit;
use iced_winit::winit::event_loop::EventLoopProxy;
use platform::appearance;
use ui::host::Host;

use super::gpu::{Begun, on_wayland, start};
use super::pane::Pane;
use super::ready::Ready;
use super::shape::{Shape, keep_shape, last_shape, window_attributes};
use super::tray::Hidden;
use super::unhosted::Unhosted;
use super::{Input, Scene, Showing, Started, Which, Woke, waking};
use crate::access::Unshown;
use crate::asks::Asking;
use crate::lost::Mends;
use crate::typing::Throttle;

/// Where a run is: its window not made yet, starting, made, or closed by the person, after which nothing is drawn or
/// made again.
pub(super) enum Stage<S> {
    Waiting,
    /// The window is made, hidden, and its GPU is being made on the start's thread.
    Starting(Box<Starting>),
    Ready(Box<Ready<S>>),
    /// The scene runs in the tray, with no window.
    Hidden(Box<Hidden<S>>),
    Closed,
}

impl<S> Stage<S> {
    pub(super) fn ready(&mut self) -> Option<&mut Ready<S>> {
        match self {
            Self::Ready(ready) => Some(ready),
            Self::Waiting | Self::Starting(_) | Self::Hidden(_) | Self::Closed => None,
        }
    }
}

/// The main window while the start's thread makes its GPU, and where that thread leaves what it made.
pub(super) struct Starting {
    window: Unshown,
    made: Arc<Mutex<Option<Result<Begun, Unhosted>>>>,
}

pub(super) struct Runner<S, F> {
    pub(super) make: Option<F>,
    pub(super) stage: Stage<S>,
    pub(super) limit: Option<u32>,
    pub(super) proxy: EventLoopProxy<Woke>,
    /// The folders the window's place is kept in and read from.
    pub(super) platform: platform::Platform,
    /// Why the window could not be hosted, once it could not; the run returns it.
    pub(super) failed: Option<Unhosted>,
}

impl<S: Scene, E, F: FnOnce(Started) -> Result<S, E>> Runner<S, F> {
    /// Makes the window, hidden, and starts the GPU it paints with on a thread of its own, so the loop answers the
    /// system (an installer's close) while a start under load waits on the driver; `finish` makes the rest.
    pub(super) fn begin(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) -> Result<(), Unhosted> {
        // Named by its scene, in the scene's language, once that is made and before the window shows.
        let window =
            Unshown::made(event_loop, window_attributes(last_shape(&self.platform, event_loop), String::new()))
                .map_err(|_| Unhosted::NoWindow)?;
        // Two threads in the Vulkan loader at once can crash it: `picture` makes, lists and opens under one lock.
        let instance = picture::instance(Some(Box::new(event_loop.owned_display_handle())));
        let surface = instance.create_surface(window.window().clone()).map_err(|_| Unhosted::NoSurface)?;
        let wayland = on_wayland(event_loop);
        let made: Arc<Mutex<Option<Result<Begun, Unhosted>>>> = Arc::default();
        let (leave, proxy) = (made.clone(), self.proxy.clone());
        let lost = waking(&self.proxy, Woke::Lost);
        std::thread::Builder::new()
            .name("start".into())
            .spawn(move || {
                let begun = start(instance, &surface, wayland, lost).map(|(gpu, engine, node, graphics)| Begun {
                    gpu,
                    engine,
                    surface,
                    node,
                    graphics,
                });
                if let Ok(mut slot) = leave.lock() {
                    *slot = Some(begun);
                }
                let _ = proxy.send_event(Woke::Started);
            })
            .map_err(|_| Unhosted::NoThread)?;
        self.stage = Stage::Starting(Box::new(Starting { window, made }));
        Ok(())
    }

    /// Makes the main window's pane from what the start's thread made, and the scene to show in it.
    pub(super) fn finish(&mut self) -> Result<(), Unhosted> {
        let starting = match std::mem::replace(&mut self.stage, Stage::Waiting) {
            Stage::Starting(starting) => starting,
            other => {
                self.stage = other;
                return Ok(());
            }
        };
        let Starting { window: unshown, made } = *starting;
        let begun = made.lock().ok().and_then(|mut slot| slot.take()).ok_or(Unhosted::NoThread)?;
        let Begun { gpu, engine, surface, node, graphics } = begun?;
        let told = waking(&self.proxy, Woke::Told);
        let make = self.make.take().ok_or(Unhosted::Scene)?;
        let window = unshown.window().clone();
        let size = gpu.drawable(window.inner_size());
        let host = Host::new(Size::new(size.width, size.height), window.scale_factor(), appearance::read());
        let main_asking = Arc::<Asking>::default();
        let mut main = Pane::drawn_by(&gpu, engine, unshown, surface, host, main_asking.clone());
        let wake = waking(&self.proxy, Woke::Pictured(Which::Main));
        let wake_out = waking(&self.proxy, Woke::Pictured(Which::Out));
        let painting = picture::Painting::of(&gpu.device, node);
        let scene = make(Started { wake, wake_out, told, painting, graphics }).map_err(|_| Unhosted::Scene)?;
        main.name(scene.title());
        let system: Arc<Mutex<Option<Appearance>>> = Arc::default();
        let watch = {
            let (system, main_asking) = (system.clone(), main_asking);
            appearance::watch(move |changed| {
                if let Ok(mut slot) = system.lock() {
                    *slot = Some(changed);
                }
                main_asking.whole();
            })
        };
        let mut ready = Box::new(Ready {
            platform: self.platform.clone(),
            gpu,
            scene,
            main,
            out: None,
            out_asking: Arc::default(),
            system,
            _watch: watch,
            occluded: false,
            stalled: false,
            typed: Throttle::default(),
            mends: Mends::default(),
        });
        let mut input = Input { host: &mut ready.main.host, events: &mut ready.main.events };
        ready.scene.opened(&mut input);
        // Made hidden, shown once the scene is ready to tell a close the system sends from a person's; a start into the
        // tray drops it unshown.
        if ready.scene.showing() == Showing::Tray {
            ready.scene.shown(false);
            self.stage = Stage::Hidden(Box::new(ready.hidden()));
        } else {
            ready.main.show();
            ready.main.asking.whole();
            self.stage = Stage::Ready(ready);
        }
        Ok(())
    }

    /// Follows where the scene wants to be: the windows go when it is kept in the tray, and are made again when it is
    /// shown.
    pub(super) fn follow_showing(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let showing = match &self.stage {
            Stage::Ready(ready) => ready.scene.showing(),
            Stage::Hidden(hidden) => hidden.scene.showing(),
            Stage::Waiting | Stage::Starting(_) | Stage::Closed => return,
        };
        let mut failed = None;
        self.stage = match (std::mem::replace(&mut self.stage, Stage::Closed), showing) {
            (Stage::Ready(mut ready), Showing::Tray) => {
                ready.scene.shown(false);
                Stage::Hidden(Box::new(ready.hidden()))
            }
            (Stage::Hidden(hidden), Showing::Window) => match hidden.shown(event_loop) {
                Ok(ready) => {
                    let mut ready = Box::new(ready);
                    ready.scene.shown(true);
                    Stage::Ready(ready)
                }
                Err((hidden, why)) => {
                    report::note!(error, "the window could not be made again", why = why);
                    failed = (why == Unhosted::DeviceLostAgain).then_some(why);
                    Stage::Hidden(hidden)
                }
            },
            (stage, _) => stage,
        };
        if let Some(why) = failed {
            self.fail(event_loop, why);
        }
    }

    /// Ends the run because its window cannot be hosted: the reason is kept for [`super::run`] to return.
    pub(super) fn fail(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, why: Unhosted) {
        report::note!(error, "cannot host", why = why);
        self.failed.get_or_insert(why);
        self.close(event_loop);
    }

    /// Makes the device the windows paint with again when it was lost; a run in the tray makes it again as it comes
    /// back.
    pub(super) fn mend(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(ready) = self.stage.ready() else { return };
        if !ready.lost() {
            return;
        }
        let mended = ready.mend(std::time::Instant::now());
        if let Err(why) = mended {
            self.fail(event_loop, why);
        }
    }

    /// Closes the window and ends the run. Nothing is drawn from here: what closing tells the scene (its connections
    /// parting) is not shown as something that happened to the person.
    pub(super) fn close(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.end();
        event_loop.exit();
    }

    /// The scene told its run is over, once, however the run ended.
    pub(super) fn end(&mut self) {
        match std::mem::replace(&mut self.stage, Stage::Closed) {
            Stage::Ready(mut ready) => {
                let main = &ready.main;
                keep_shape(
                    &ready.platform,
                    Shape { maximized: main.window.is_maximized(), ..main.shape },
                    &main.window,
                );
                ready.scene.closed();
            }
            // Its shape was kept as it went to the tray.
            Stage::Hidden(mut hidden) => hidden.scene.closed(),
            // The start's thread ends with the process; what it makes is never used.
            Stage::Waiting | Stage::Starting(_) | Stage::Closed => {}
        }
    }
}
