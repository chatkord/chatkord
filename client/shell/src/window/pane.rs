//! One window of the run: its input, title, input method and the events the windowing system sends it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use design::{Appearance, Preference, Scheme, Zoom};
use iced_wgpu::graphics::Viewport;
use iced_wgpu::{Engine, Renderer, wgpu};
use iced_winit::Clipboard;
use iced_winit::conversion;
use iced_winit::core::input_method::InputMethod;
use iced_winit::core::theme::Base;
use iced_winit::core::time::Instant;
use iced_winit::core::{self, Element, Event, Size, mouse, window};
use iced_winit::runtime::user_interface;
use iced_winit::winit::{self, event::WindowEvent, keyboard::ModifiersState};
use ui::host::{self, Host};

use super::gpu::{Gpu, engine, viewport};
use super::interface::{Interacted, Pass, Shown, Turn, Turned, interact};
use super::shape::{Shape, covers, resize_direction};
use super::unhosted::Unhosted;
use crate::access::{Heard, Unshown, press_at};
use crate::asks::{self, Asking, Due, Look, Said};

/// One window of the run and all that is its own: its surface, the engine that draws it, its host and the interface's
/// state. Each has an engine of its own, so what one window drew is never trimmed away by another's frame.
pub(super) struct Pane {
    pub(super) window: Arc<winit::window::Window>,
    /// How anything asks this window to draw.
    pub(super) asking: Arc<Asking>,
    /// What the last whole draw was made at: none for a new pane and after the renderer was made again.
    pub(super) drawn: Option<Look>,
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) renderer: Renderer,
    pub(super) events: Vec<Event>,
    /// What the window's host decides: the pointer, the scale, the scheme, when to wake.
    pub(super) host: Host,
    cache: user_interface::Cache,
    pub(super) viewport: Viewport,
    /// The least size the window was last given, in the system's logical pixels.
    pub(super) least: Size,
    /// The most a side of the surface or the viewport may be, the device's texture limit.
    pub(super) limit: u32,
    /// The size the surface was configured at, none until it is.
    pub(super) configured: super::surface::Configured,
    modifiers: ModifiersState,
    pub(super) redraws: u32,
    clipboard: Clipboard,
    /// Clipboard answers that arrived on another thread; they become events on the next turn.
    arrived: Arc<Mutex<Vec<Event>>>,
    /// Where the system was last told an input method composes.
    composing_at: host::InputMethodPlace,
    /// What it is composing, drawn over the interface until it is committed.
    preedit: Option<ui::Preedit<Renderer>>,
    /// Whether the pointer is hidden over the window, as the interface last asked.
    cursor_hidden: bool,
    /// How the window stands, for the next one if it goes to the tray.
    pub(super) shape: Shape,
    /// What a screen reader is handed of the window.
    access: crate::access::Access,
    /// What the window was last named; read here, since X11 tells winit nothing back.
    title: String,
    /// The focusable a screen reader asked to focus, for the next turn.
    focus_at: Option<usize>,
    /// What the last frame showed the widgets, none before the first.
    shown: Option<Shown>,
    /// When the interface last took a turn, none before the first.
    turned: Option<Instant>,
    /// One frame of the monitor the window is on.
    interval: Duration,
    /// Where the window stood before it went over the whole monitor, put back when it leaves.
    pub(super) before_full: Option<super::surface::Frame>,
}

impl Pane {
    /// A pane for `window` and its `surface`, drawn on `gpu` by an engine of its own; [`Pane::show`] shows it.
    pub(super) fn new(
        gpu: &Gpu,
        window: Unshown,
        surface: wgpu::Surface<'static>,
        host: Host,
        asking: Arc<Asking>,
    ) -> Pane {
        Self::with_renderer(gpu, Renderer::new(engine(gpu), ui::fonts::settings()), window, surface, host, asking)
    }

    /// The same, drawn by `engine`, made on `gpu` already.
    pub(super) fn drawn_by(
        gpu: &Gpu,
        engine: Engine,
        window: Unshown,
        surface: wgpu::Surface<'static>,
        host: Host,
        asking: Arc<Asking>,
    ) -> Pane {
        Self::with_renderer(gpu, Renderer::new(engine, ui::fonts::settings()), window, surface, host, asking)
    }

    /// The same, drawn by `renderer`, made on `gpu` already: a window that came back from the tray keeps its engine.
    pub(super) fn with_renderer(
        gpu: &Gpu,
        renderer: Renderer,
        window: Unshown,
        surface: wgpu::Surface<'static>,
        host: Host,
        asking: Arc<Asking>,
    ) -> Pane {
        let access = crate::access::Access::new(&window, asking.clone());
        let window = window.into_window();
        asking.open(window.clone());
        let mut pane = Pane {
            asking,
            drawn: None,
            viewport: viewport(&window, host.scale(), gpu.drawable_limit()),
            limit: gpu.drawable_limit(),
            least: host.least(),
            configured: super::surface::Configured::default(),
            surface,
            renderer,
            events: Vec::new(),
            host,
            cache: user_interface::Cache::new(),
            modifiers: ModifiersState::default(),
            redraws: 0,
            clipboard: Clipboard::new(),
            arrived: Arc::default(),
            composing_at: host::InputMethodPlace::default(),
            preedit: None,
            cursor_hidden: false,
            shape: Shape::of(&window),
            access,
            title: String::new(),
            focus_at: None,
            shown: None,
            turned: None,
            interval: asks::FRAME_FALLBACK,
            before_full: None,
            window,
        };
        pane.fit_surface(gpu, pane.window.inner_size());
        pane.follow_monitor();
        pane
    }

    /// Takes the frame interval of the monitor the window is on now.
    fn follow_monitor(&mut self) {
        let rate = self.window.current_monitor().and_then(|monitor| monitor.refresh_rate_millihertz());
        self.interval = asks::frame_interval(rate);
    }

    /// Takes what a screen reader asked since the last pass: a focus waits for the next turn, a press is the pointer
    /// moved to the control and the events of a click.
    pub(super) fn heed_access(&mut self) {
        for heard in self.access.heard() {
            match heard {
                Heard::Focus(at) => self.focus_at = Some(at),
                Heard::Press(bounds) => {
                    let scale = f64::from(self.host.scale().total());
                    let centre = bounds.center();
                    self.host.on(host::Event::CursorMoved {
                        x: f64::from(centre.x) * scale,
                        y: f64::from(centre.y) * scale,
                    });
                    self.events.extend(press_at(bounds));
                }
            }
        }
    }

    /// Whether a turn has something to give the interface: events, or a focus a screen reader asked for.
    pub(super) fn queued(&self) -> bool {
        self.focus_at.is_some() || !self.events.is_empty()
    }

    /// When what is queued is turned at `now`: moves alone wait out a frame interval since the last turn.
    pub(super) fn due(&self, now: Instant) -> Due {
        asks::due(&self.events, self.focus_at.is_some(), self.turned, self.interval, now)
    }

    /// Whether a whole frame is asked for and a shown window will be given it, so what is queued waits for its turn.
    pub(super) fn owes_frame(&self) -> bool {
        self.asking.owed() && !self.host.hidden()
    }

    /// Draws on `gpu`, which replaced the device this pane was drawn on: a new surface and renderer, and the interface
    /// laid out again. The old renderer ends before the old device does, since the caller keeps `gpu`'s predecessor
    /// until this is done.
    pub(super) fn remake(&mut self, gpu: &Gpu) -> Result<(), Unhosted> {
        self.surface = gpu.instance.create_surface(self.window.clone()).map_err(|_| Unhosted::NoSurface)?;
        self.renderer = Renderer::new(engine(gpu), ui::fonts::settings());
        self.cache = user_interface::Cache::new();
        self.limit = gpu.drawable_limit();
        self.viewport = viewport(&self.window, self.host.scale(), self.limit);
        self.drawn = None;
        self.configure_again(gpu);
        self.asking.whole();
        Ok(())
    }

    /// Shows the window, made hidden so a screen reader's adapter was attached first.
    pub(super) fn show(&self) {
        self.window.set_visible(true);
    }

    /// Names the window `title`, telling the system only when the name changes.
    pub(super) fn name(&mut self, title: String) {
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
    }

    /// Neither maximized, minimized nor over the whole monitor: Windows reports a minimized window far off every
    /// monitor, which is no place to keep.
    fn normal(&self) -> bool {
        !self.window.is_maximized() && self.window.is_minimized() != Some(true) && self.window.fullscreen().is_none()
    }

    /// Whether the window is over the whole of its monitor: winit says it is, and its frame is the monitor's. On
    /// Windows and X11 winit says only what was last asked for, so a window manager that gave the window back is
    /// seen by its size.
    fn over_monitor(&self) -> bool {
        if self.window.fullscreen().is_none() {
            return false;
        }
        match (self.window.outer_position(), self.window.current_monitor()) {
            (Ok(position), Some(monitor)) => {
                covers((position, self.window.outer_size()), (monitor.position(), monitor.size()))
            }
            // Where the system gives no place, as Wayland does, its own word stands.
            (Err(_), _) | (_, None) => true,
        }
    }

    /// What iced's own loop does with a finished update: the input method, the pointer, the clipboard; and what it
    /// said of drawing, for the turn to ask.
    fn apply(&mut self, state: user_interface::State, drawn: bool) -> Said {
        let said = Said::of(&state);
        let user_interface::State::Updated { input_method, mouse_interaction, clipboard, .. } = state else {
            return said;
        };
        self.input_method(input_method, drawn);
        if let Some(icon) = conversion::mouse_interaction(mouse_interaction) {
            self.window.set_cursor(icon);
        }
        let hidden = mouse_interaction == mouse::Interaction::Hidden;
        if hidden != self.cursor_hidden {
            self.cursor_hidden = hidden;
            self.window.set_cursor_visible(!hidden);
        }
        for kind in clipboard.reads {
            let (sink, asking) = (self.arrived.clone(), self.asking.clone());
            self.clipboard.read(kind, move |result| {
                if let Ok(mut events) = sink.lock() {
                    events.push(Event::Clipboard(core::clipboard::Event::Read(result.map(Arc::new))));
                }
                asking.whole();
            });
        }
        if let Some(content) = clipboard.write {
            let (sink, asking) = (self.arrived.clone(), self.asking.clone());
            self.clipboard.write(content, move |result| {
                if let Ok(mut events) = sink.lock() {
                    events.push(Event::Clipboard(core::clipboard::Event::Written(result)));
                }
                asking.whole();
            });
        }
        said
    }

    /// Asks for the frame a turn came to, as the host decides for a hidden window. True when the windowing system gave
    /// the window back from the whole monitor by itself.
    pub(super) fn ask(&mut self, gpu: &Gpu, redraw: window::RedrawRequest) -> bool {
        let out = self.host.on(host::Event::Updated { redraw });
        self.carry_out(gpu, out)
    }

    /// Tells the system what changed of where an input method composes, and keeps what it is composing; an update
    /// that drew nothing changes neither.
    fn input_method(&mut self, input_method: InputMethod, drawn: bool) {
        let (asked, preedit) = match input_method {
            InputMethod::Disabled => (None, None),
            InputMethod::Enabled { cursor, purpose, preedit } => {
                (Some((cursor, purpose)), preedit.filter(|preedit| !preedit.content.is_empty()).map(|p| (cursor, p)))
            }
        };
        let tell = self.composing_at.asked(drawn, asked);
        if !drawn {
            return;
        }
        if let Some(allowed) = tell.allowed {
            self.window.set_ime_allowed(allowed);
        }
        if let Some((cursor, purpose)) = tell.place {
            // The interface's pixels are the system's times the zoom: told in the screen's own.
            let scale = self.host.scale().total();
            self.window.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(cursor.x * scale, cursor.y * scale),
                winit::dpi::PhysicalSize::new(cursor.width * scale, cursor.height * scale),
            );
            self.window.set_ime_purpose(conversion::ime_purpose(purpose));
        }
        self.preedit = preedit.map(|(cursor, preedit)| {
            let mut overlay = self.preedit.take().unwrap_or_default();
            overlay.update(cursor, &preedit, &self.renderer);
            overlay
        });
    }

    /// Takes the scheme and the zoom the person chose. Before a frame is taken, never during one: a new zoom can resize
    /// the window, and a surface changed under a taken frame loses it.
    pub(super) fn follow_look(&mut self, gpu: &Gpu, preference: Preference, zoom: Zoom) -> bool {
        let mut left = false;
        if preference != self.host.preference() {
            let out = self.host.on(host::Event::Preference(preference));
            left |= self.carry_out(gpu, out);
        }
        if zoom != self.host.zoom() {
            let out = self.host.on(host::Event::Zoom(zoom));
            left |= self.carry_out(gpu, out);
        }
        left
    }

    /// One turn of the interface for `pass`: `element` built and given what is queued, drawn for a frame.
    pub(super) fn turn<M, T: Base + ui::Composing + 'static>(
        &mut self,
        element: Element<'_, M, T, Renderer>,
        theme: &T,
        pass: Pass,
    ) -> Turned<M> {
        let events = std::mem::take(&mut self.events);
        self.turned = Some(pass.at());
        let cursor = self.host.cursor();
        let mut turn = Turn {
            renderer: &mut self.renderer,
            cache: &mut self.cache,
            window: &self.window,
            size: self.viewport.logical_size(),
            scale: self.host.scale().total(),
            cursor,
            access: &mut self.access,
            title: &self.title,
            focus_at: self.focus_at.take(),
            shown: self.shown,
        };
        let Interacted { primed, events: after_events, frame, published, statuses } =
            interact(element, theme, &mut turn, &events, pass);
        let pressed = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let pressed_through = events
            .iter()
            .zip(&statuses)
            .any(|(event, status)| *event == pressed && *status == core::event::Status::Ignored);
        let said = match frame {
            Some((at, frame)) => {
                let between = self.apply(after_events, false).answered();
                let said = between.and(self.apply(frame, true));
                self.shown = Some(Shown { at, cursor });
                if let Some(preedit) = &self.preedit {
                    let whole = core::Rectangle::with_size(self.viewport.logical_size());
                    preedit.draw(&mut self.renderer, &ui::Composing::preedit(theme), whole);
                }
                said
            }
            None => primed.and(self.apply(after_events, false)),
        };
        Turned { published, said, pressed_through }
    }

    /// What the windowing system said of the window, for its host and its widgets. True when the windowing system
    /// gave the window back from the whole monitor by itself.
    pub(super) fn on_event(&mut self, gpu: &Gpu, event: &WindowEvent) -> bool {
        let mut left = false;
        match *event {
            WindowEvent::Occluded(occluded) => {
                let out = self.host.on(host::Event::Occluded(occluded));
                left |= self.carry_out(gpu, out);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.host.on(host::Event::CursorMoved { x: position.x, y: position.y });
            }
            WindowEvent::CursorLeft { .. } => {
                let out = self.host.on(host::Event::CursorLeft);
                left |= self.carry_out(gpu, out);
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::Moved(position) => {
                self.shape.moved(position, self.normal());
                self.access.placed(&self.window);
                self.follow_monitor();
            }
            WindowEvent::Focused(focused) => self.access.focused(focused),
            WindowEvent::Resized(size) => {
                self.shape.resized(size, self.window.scale_factor(), self.normal());
                self.access.placed(&self.window);
                let size = gpu.drawable(size);
                let out = self.host.on(host::Event::Resized { width: size.width, height: size.height });
                left |= self.carry_out(gpu, out);
                let out = self.host.on(host::Event::Fullness(self.over_monitor()));
                left |= self.carry_out(gpu, out);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.follow_monitor();
                let out = self.host.on(host::Event::ScaleFactorChanged { scale_factor });
                left |= self.carry_out(gpu, out);
            }
            // A windowing system with no settings service still says its scheme here.
            WindowEvent::ThemeChanged(theme) => {
                let scheme = match theme {
                    winit::window::Theme::Light => Scheme::Light,
                    winit::window::Theme::Dark => Scheme::Dark,
                };
                let system = Appearance { scheme: Some(scheme), motion: self.host.motion() };
                let out = self.host.on(host::Event::System(system));
                left |= self.carry_out(gpu, out);
            }
            _ => {}
        }
        if let Some(event) = conversion::window_event(event.clone(), self.host.scale().total(), self.modifiers) {
            self.events.push(event);
        }
        if let Ok(mut arrived) = self.arrived.lock() {
            self.events.append(&mut arrived);
        }
        left
    }

    /// A press no widget took, in a window with no frame: its edge resizes the window, the rest moves it.
    pub(super) fn grip(&self) {
        let mouse::Cursor::Available(at) = self.host.cursor() else { return };
        let gripped = match host::grip(at, self.viewport.logical_size()) {
            host::Grip::Move => self.window.drag_window(),
            host::Grip::Resize(edge) => self.window.drag_resize_window(resize_direction(edge)),
        };
        if gripped.is_err() {
            report::note!(warn, "the screen's own window could not be moved or resized");
        }
    }
}
