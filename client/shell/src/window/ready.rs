//! A run's windows with the GPU they paint with and the scene they show, and how each follows the scene.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use design::Appearance;
use iced_winit::core::Size;
use iced_winit::core::time::Instant;
use iced_winit::winit;
use platform::appearance;
use ui::host::{self, Host};

use super::Scene;
use super::gpu::Gpu;
use super::interface::Pass;
use super::pane::Pane;
use super::shape::out_attributes;
use crate::access::Unshown;
use crate::asks::{Asking, Due, Outcome};
use crate::lost::Mends;
use crate::typing::Throttle;

/// How long a redraw asked of the main window may go ungiven before the window is taken as hidden: Wayland tells a
/// client nothing of its minimise or of a workspace switch, it only stops giving it frames.
const STALL: Duration = Duration::from_secs(1);

/// The windows of a run, the GPU they paint with and the scene they show.
pub(super) struct Ready<S> {
    /// The folders the window's place is kept in.
    pub(super) platform: platform::Platform,
    pub(super) scene: S,
    pub(super) main: Pane,
    /// A watched screen's own window, while the scene wants one.
    pub(super) out: Option<Pane>,
    /// The screen's own window's asks, which each window of it opens and closes.
    pub(super) out_asking: Arc<Asking>,
    /// The system's appearance as it changes, read on another thread; folded into each host on the next turn.
    pub(super) system: Arc<Mutex<Option<Appearance>>>,
    pub(super) _watch: appearance::Watch,
    /// The windowing system said the main window is minimised or covered whole.
    pub(super) occluded: bool,
    /// A redraw asked of the main window went ungiven for [`STALL`]: it is taken as hidden until a frame comes.
    pub(super) stalled: bool,
    /// When the scene was last told of the person's input.
    pub(super) typed: Throttle,
    /// When the device was last made again.
    pub(super) mends: Mends,
    /// Last, so it ends after the windows and engines that hold its device.
    pub(super) gpu: Gpu,
}

impl<S: Scene> Ready<S> {
    /// Tells the scene whether the main window shows, as every way of knowing it says: Windows paints a minimised
    /// window when asked and says nothing of its minimise but its size.
    pub(super) fn tell_shown(&mut self) {
        let minimised = self.main.window.is_minimized() == Some(true);
        self.scene.shown(!self.occluded && !self.stalled && !minimised);
    }

    /// The main window was given a frame: every ask is answered, and a window taken as hidden shows again.
    pub(super) fn answered(&mut self) {
        self.main.asking.answered();
        if std::mem::take(&mut self.stalled) {
            self.tell_shown();
        }
    }

    /// When an unanswered redraw makes the main window hidden, while it does not yet.
    pub(super) fn stall_at(&self) -> Option<Instant> {
        let since = self.main.asking.since();
        since.filter(|_| !self.stalled).map(|since| since + STALL)
    }

    /// Takes the main window as hidden once a redraw asked of it went ungiven for [`STALL`].
    pub(super) fn follow_stall(&mut self, now: Instant) {
        if self.stall_at().is_some_and(|at| at <= now) {
            self.stalled = true;
            self.tell_shown();
        }
    }

    /// Takes the scheme, the zoom and the full screen the scene chose, for every window. Before a frame is taken,
    /// never during one: a new zoom can resize a window, and a surface changed under a taken frame loses it.
    pub(super) fn follow_scene(&mut self) {
        let (preference, zoom) = (self.scene.preference(), self.scene.zoom());
        if zoom != self.main.host.zoom() {
            report::note!(info, "zoom changed", percent = zoom.percent());
        }
        let mut left = self.main.follow_look(&self.gpu, preference, zoom);
        if let Some(out) = self.out.as_mut() {
            left |= out.follow_look(&self.gpu, preference, zoom);
        }
        let full = self.scene.full_screen();
        if full != self.main.host.full_screen() {
            let out = self.main.host.on(host::Event::FullScreen(full));
            left |= self.main.carry_out(&self.gpu, out);
        }
        if left {
            self.scene.full_screen_left();
        }
    }

    /// Carries out what the main window's host said, telling the scene when its full screen was left by itself.
    pub(super) fn carry_out(&mut self, out: host::Out) {
        if self.main.carry_out(&self.gpu, out) {
            self.scene.full_screen_left();
        }
    }

    /// One turn of the main window's interface for `pass`: what the widgets published goes back to the scene, and the
    /// frame the turn came to is asked for, of every window when the scene changed.
    pub(super) fn turn(&mut self, pass: Pass) {
        let size = self.main.viewport.logical_size();
        self.scene.room(size);
        self.scene.prepare(self.main.host.scheme(), self.main.host.motion());
        let turned = self.main.turn(self.scene.view(size), self.scene.theme(), pass);
        let changed = self.scene.update(turned.published);
        if changed && let Some(out) = &self.out {
            out.asking.whole();
        }
        if self.main.ask(&self.gpu, Outcome { said: turned.said, changed }.ask()) {
            self.scene.full_screen_left();
        }
    }

    /// One turn of the screen's own window for `pass`, as the main window's: a press there that no control took
    /// moves or resizes it.
    pub(super) fn turn_out(&mut self, pass: Pass) {
        let Some(out) = self.out.as_mut() else { return };
        let size = out.viewport.logical_size();
        self.scene.prepare(out.host.scheme(), out.host.motion());
        let Some(element) = self.scene.view_out(size) else {
            out.events.clear();
            return;
        };
        let turned = out.turn(element, self.scene.theme(), pass);
        if turned.pressed_through {
            out.grip();
        }
        let changed = self.scene.update(turned.published);
        if changed {
            self.main.asking.whole();
        }
        out.ask(&self.gpu, Outcome { said: turned.said, changed }.ask());
    }

    /// Hands each window's interface what happened to it since its last turn, all at once, at `now`: pointer moves
    /// alone wait out a frame interval, so a pointer that reports a thousand moves a second costs a turn a frame. A
    /// window owed a frame keeps them for that frame's turn, so it is laid out once for both.
    pub(super) fn turn_queued(&mut self, now: Instant) {
        self.main.heed_access();
        if !self.main_waits() && self.main.due(now) == Due::Now {
            self.turn(Pass::Between(now));
        }
        if let Some(out) = self.out.as_mut() {
            out.heed_access();
        }
        if self.out.as_ref().is_some_and(|out| !out.owes_frame() && out.due(now) == Due::Now) {
            self.turn_out(Pass::Between(now));
        }
    }

    /// Whether the main window's queue waits for the frame it is owed, which a hidden or stalled window is not given.
    fn main_waits(&self) -> bool {
        self.main.owes_frame() && !self.stalled
    }

    /// When the loop must wake to turn moves that wait out their frame interval, in either window; never for a window
    /// whose queue waits for a frame, which turns it.
    pub(super) fn turn_wake(&self, now: Instant) -> Option<Instant> {
        let at = |due: Due| match due {
            Due::At(at) => Some(at),
            Due::Idle | Due::Now => None,
        };
        let main = at(self.main.due(now)).filter(|_| !self.main_waits());
        let out = self.out.as_ref().filter(|out| !out.owes_frame()).and_then(|out| at(out.due(now)));
        main.into_iter().chain(out).min()
    }

    /// Names the main window again when the scene's title changes: its language did.
    pub(super) fn follow_title(&mut self) {
        self.main.name(self.scene.title());
    }

    /// Opens the screen's own window when the scene wants one, closes it when it no longer does, and keeps its title.
    pub(super) fn follow_out(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        match (self.scene.out(), &mut self.out) {
            (Some(title), None) => match self.open_out(event_loop, title) {
                Ok(pane) => {
                    pane.asking.whole();
                    self.out = Some(pane);
                }
                Err(_) => {
                    report::note!(error, "the screen's own window could not be made");
                    self.scene.out_closed();
                }
            },
            (Some(title), Some(out)) => out.name(title),
            (None, Some(_)) => {
                self.out_asking.close();
                self.out = None;
                self.main.asking.whole();
            }
            (None, None) => {}
        }
    }

    /// Makes the screen's own window, a quarter of the display the main window is on, drawn on the same GPU.
    fn open_out(&self, event_loop: &winit::event_loop::ActiveEventLoop, title: String) -> Result<Pane, String> {
        let display = self
            .main
            .window
            .current_monitor()
            .map_or(0.0, |monitor| monitor.size().to_logical::<f32>(monitor.scale_factor()).width);
        let window = Unshown::made(event_loop, out_attributes(title.clone(), display))
            .map_err(|why| format!("no window: {why}"))?;
        let surface =
            self.gpu.instance.create_surface(window.window().clone()).map_err(|why| format!("no surface: {why}"))?;
        let (size, scale) = (self.gpu.drawable(window.window().inner_size()), window.window().scale_factor());
        let least = Size::new(design::POP_OUT_LEAST.0, design::POP_OUT_LEAST.1);
        let host = Host::new(Size::new(size.width, size.height), scale, appearance::read()).with_least(least);
        let mut pane = Pane::new(&self.gpu, window, surface, host, self.out_asking.clone());
        pane.name(title);
        pane.follow_look(&self.gpu, self.scene.preference(), self.scene.zoom());
        pane.show();
        Ok(pane)
    }

    /// Folds what the system changed of its appearance into every window's host.
    pub(super) fn follow_system(&mut self) {
        let changed = self.system.lock().ok().and_then(|mut slot| slot.take());
        let Some(changed) = changed else { return };
        let out = self.main.host.on(host::Event::System(changed));
        self.carry_out(out);
        if let Some(pane) = self.out.as_mut() {
            let out = pane.host.on(host::Event::System(changed));
            pane.carry_out(&self.gpu, out);
        }
    }

    /// Every window drawn again: what the scene shows changed.
    pub(super) fn redraw_all(&self) {
        self.main.asking.whole();
        if let Some(out) = &self.out {
            out.asking.whole();
        }
    }
}
