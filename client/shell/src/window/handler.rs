//! What the loop does with the windowing system's events: each window's, a redraw, and the moments between.

use std::time::Instant as Clock;

use iced_winit::core::time::Instant;
use iced_winit::winit::{
    self,
    event::{StartCause, WindowEvent},
    event_loop::ControlFlow,
};
use ui::host;

use super::interface::Pass;
use super::pane::Pane;
use super::runner::{Runner, Stage};
use super::{Closing, Input, Next, Redrawn, Scene, Started, Which, Woke};
use crate::asks::Redraw;
use crate::typing;

impl<S: Scene, E, F: FnOnce(Started) -> Result<S, E>> Runner<S, F> {
    /// A redraw of the main window: the scene followed, its interface given what is queued, drawn and presented, and
    /// what the scene says after it carried out. With no frame to draw, what is queued is given without one.
    fn redraw_main(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(r) = self.stage.ready() else { return };
        r.answered();
        r.follow_scene();
        let (frame, acquired) = r.main.frame(&r.gpu);
        let mut next = Next::Wait;
        if let Some(frame) = frame {
            // From here, the CPU's work: what waiting for the panel (above, and presenting below) costs is not counted.
            let frame_started = Clock::now();
            let look = r.main.look();
            let how = Redraw::of(r.main.asking.take_stale(), r.main.drawn, look);
            if how == Redraw::Whole {
                r.turn(Pass::Frame(Instant::now()));
                r.main.drawn = Some(look);
            } else if r.main.queued() {
                r.turn(Pass::Between(Instant::now()));
            }
            let cpu = r.main.present(&r.gpu, frame, frame_started);
            let redrawn = Redrawn { frames: r.main.redraws, cpu, laid_out: how == Redraw::Whole };
            let mut input = Input { host: &mut r.main.host, events: &mut r.main.events };
            next = r.scene.redrawn(redrawn, &mut input);
        } else if r.main.queued() {
            r.turn(Pass::Between(Instant::now()));
        }
        let out = r.main.host.on(host::Event::Acquired { acquired, now: Instant::now() });
        r.carry_out(out);
        match next {
            _ if self.limit.is_some_and(|limit| r.main.redraws >= limit) => {
                report::note!(info, "redraws done", redraws = r.main.redraws);
                event_loop.exit();
            }
            Next::Wait => {}
            Next::Again => r.main.asking.whole(),
            Next::Exit => event_loop.exit(),
        }
    }

    /// An event of the main window.
    fn main_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: &WindowEvent) {
        if *event == WindowEvent::CloseRequested {
            match self.stage.ready() {
                Some(r) => match r.scene.close_asked() {
                    Closing::Stay => r.redraw_all(),
                    Closing::Now => self.close(event_loop),
                },
                None => self.close(event_loop),
            }
            return;
        }
        if *event == WindowEvent::RedrawRequested {
            self.redraw_main(event_loop);
        }
        let Some(r) = self.stage.ready() else { return };
        if r.main.on_event(&r.gpu, event) {
            r.scene.full_screen_left();
        }
        if let WindowEvent::Occluded(occluded) = *event {
            r.occluded = occluded;
            r.tell_shown();
        }
        if let WindowEvent::Resized(_) = *event {
            r.tell_shown();
        }
        if typing::is_input(event) && r.typed.tell(Instant::now()) {
            r.scene.typed();
        }
        match event {
            WindowEvent::Focused(focused) => r.scene.focused(*focused),
            WindowEvent::HoveredFile(_) => r.scene.files_hovered(true),
            WindowEvent::HoveredFileCancelled => r.scene.files_hovered(false),
            WindowEvent::DroppedFile(path) => r.scene.file_dropped(path.clone()),
            _ => {}
        }
        r.follow_system();
        if r.scene.drain() {
            r.redraw_all();
        }
        r.follow_out(event_loop);
    }

    /// An event of the screen's own window: drawn like the main one, closed by the person or the system.
    fn out_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: &WindowEvent) {
        let Some(r) = self.stage.ready() else { return };
        if *event == WindowEvent::CloseRequested {
            r.scene.out_closed();
            r.follow_out(event_loop);
            return;
        }
        if *event == WindowEvent::RedrawRequested {
            r.follow_scene();
            let gpu = &r.gpu;
            let frame = r.out.as_mut().map(|out| out.frame(gpu));
            if let Some((frame, acquired)) = frame {
                if let Some(frame) = frame {
                    let started = Clock::now();
                    let how = r.out.as_ref().map(|out| {
                        let look = out.look();
                        (look, Redraw::of(out.asking.take_stale(), out.drawn, look))
                    });
                    match how {
                        Some((look, Redraw::Whole)) => {
                            r.turn_out(Pass::Frame(Instant::now()));
                            if let Some(out) = r.out.as_mut() {
                                out.drawn = Some(look);
                            }
                        }
                        Some((_, Redraw::Picture)) if r.out.as_ref().is_some_and(Pane::queued) => {
                            r.turn_out(Pass::Between(Instant::now()));
                        }
                        Some((_, Redraw::Picture)) | None => {}
                    }
                    if let Some(out) = r.out.as_mut() {
                        out.present(&r.gpu, frame, started);
                    }
                } else if r.out.as_ref().is_some_and(Pane::queued) {
                    r.turn_out(Pass::Between(Instant::now()));
                }
                if let Some(out) = r.out.as_mut() {
                    let said = out.host.on(host::Event::Acquired { acquired, now: Instant::now() });
                    out.carry_out(&r.gpu, said);
                }
            }
        }
        if typing::is_input(event) && r.typed.tell(Instant::now()) {
            r.scene.typed();
        }
        let Some(out) = r.out.as_mut() else { return };
        out.on_event(&r.gpu, event);
        r.follow_out(event_loop);
    }
}

impl<S: Scene, E, F: FnOnce(Started) -> Result<S, E>> Runner<S, F> {
    /// A later launch's or a popup click's ask to come forward: on Wayland the desktop's token raises it, without one
    /// it only flashes; Windows lets it rise since that launch handed it the foreground. Or a scene has something to
    /// drain, which it does even while hidden.
    fn woken(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, woke: Woke) {
        match woke {
            Woke::Closed => return self.close(event_loop),
            Woke::Started => {
                if let Err(why) = self.finish() {
                    self.fail(event_loop, why);
                }
                return;
            }
            Woke::Lost => return self.mend(event_loop),
            Woke::Forward(..) | Woke::Told | Woke::Pictured(_) => {}
        }
        if let Stage::Hidden(hidden) = &mut self.stage {
            match woke {
                Woke::Forward(activation, _) => {
                    hidden.activation = activation;
                    hidden.scene.forwarded();
                }
                Woke::Told => {
                    hidden.scene.drain();
                }
                Woke::Closed | Woke::Started | Woke::Lost | Woke::Pictured(_) => {}
            }
            return self.follow_showing(event_loop);
        }
        let Some(ready) = self.stage.ready() else { return };
        match woke {
            Woke::Forward(activation, _) => {
                let window = &ready.main.window;
                let raised = activation.is_some_and(|token| platform::raise::raise(window.as_ref(), &token).is_ok());
                if !raised {
                    window.set_minimized(false);
                    window.focus_window();
                    window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
                }
            }
            Woke::Told => {
                if ready.scene.drain() {
                    ready.redraw_all();
                }
                ready.follow_out(event_loop);
            }
            Woke::Pictured(Which::Main) => ready.main.asking.picture(),
            Woke::Pictured(Which::Out) => ready.out_asking.picture(),
            Woke::Closed | Woke::Started | Woke::Lost => {}
        }
    }
}

impl<S: Scene, E, F: FnOnce(Started) -> Result<S, E>> winit::application::ApplicationHandler<Woke> for Runner<S, F> {
    fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, mut woke: Woke) {
        // The later launch that knocked is told once it was handled: it waits for that, and a window the loop cannot
        // reach never says it.
        let reply = match &mut woke {
            Woke::Forward(_, reply) => reply.take(),
            Woke::Told | Woke::Pictured(_) | Woke::Closed | Woke::Started | Woke::Lost => None,
        };
        self.woken(event_loop, woke);
        if let Some(reply) = reply {
            reply.done();
        }
    }

    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !matches!(self.stage, Stage::Waiting) {
            return;
        }
        if let Err(why) = self.begin(event_loop) {
            self.fail(event_loop, why);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(r) = self.stage.ready() else { return };
        if r.main.window.id() == id {
            self.main_event(event_loop, &event);
        } else if r.out.as_ref().is_some_and(|out| out.window.id() == id) {
            self.out_event(event_loop, &event);
        }
    }

    /// The loop woke: a redraw iced asked for at a time (a caret to blink, a tooltip to show) may be due, in either
    /// window.
    fn new_events(&mut self, _: &winit::event_loop::ActiveEventLoop, cause: StartCause) {
        let (Some(r), StartCause::ResumeTimeReached { .. }) = (self.stage.ready(), cause) else { return };
        let out = r.main.host.on(host::Event::Woke { now: Instant::now() });
        r.carry_out(out);
        if let Some(pane) = r.out.as_mut() {
            let out = pane.host.on(host::Event::Woke { now: Instant::now() });
            pane.carry_out(&r.gpu, out);
        }
    }

    /// Before the loop sleeps: each interface is handed what happened since its last turn, unless it waits out its frame
    /// interval, then the loop waits until the soonest time either window's host or a waiting queue asked to be woken,
    /// else until something happens.
    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let closing = match &self.stage {
            Stage::Ready(ready) => ready.scene.closing(),
            Stage::Hidden(hidden) => hidden.scene.closing(),
            Stage::Waiting | Stage::Starting(_) | Stage::Closed => return,
        };
        if closing == Closing::Now {
            return self.close(event_loop);
        }
        self.follow_showing(event_loop);
        let Some(r) = self.stage.ready() else { return event_loop.set_control_flow(ControlFlow::Wait) };
        let now = Instant::now();
        r.turn_queued(now);
        r.follow_title();
        r.follow_out(event_loop);
        r.follow_stall(now);
        let soonest =
            [r.main.host.due(), r.out.as_ref().and_then(|out| out.host.due()), r.stall_at(), r.turn_wake(now)]
                .into_iter()
                .flatten()
                .min();
        event_loop.set_control_flow(soonest.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}
