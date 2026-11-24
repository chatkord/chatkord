//! The scene a window shows: what it is asked each turn, the input it makes for itself and what a redraw measured.

use std::fmt;
use std::time::Duration;

use design::{MotionPreference, Preference, Scheme, Zoom};
use iced_wgpu::Renderer;
use iced_winit::core::theme::Base;
use iced_winit::core::{Element, Event, Point, Size, mouse};
use ui::host::{self, Host};

use super::{Closing, Next, Showing};

/// What the loop measured of the frames drawn so far: how many, and the CPU time of the last one (waiting for the
/// panel not counted).
#[derive(Clone, Copy, Debug)]
pub struct Redrawn {
    /// Frames drawn since the run began.
    pub frames: u32,
    /// The CPU time of the last frame: its update and its drawing, not waiting for the panel or presenting to it.
    pub cpu: Duration,
    /// Whether the interface was laid out again for this frame; false when only a new picture was presented in the
    /// layout the last whole frame had.
    pub laid_out: bool,
}

/// Input a scene makes for itself (a scripted test): the pointer, and events the next turn gives the widgets.
pub struct Input<'a> {
    pub(super) host: &'a mut Host,
    pub(super) events: &'a mut Vec<Event>,
}

impl Input<'_> {
    /// An event the widgets get on the next turn.
    pub fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Moves the pointer to `point` (logical pixels), as the host and the widgets both see it.
    pub fn move_to(&mut self, point: Point) {
        let scale = f64::from(self.host.scale().total());
        self.host.on(host::Event::CursorMoved { x: f64::from(point.x) * scale, y: f64::from(point.y) * scale });
        self.events.push(Event::Mouse(mouse::Event::CursorMoved { position: point }));
    }
}

/// What a window shows and how it answers: built once the window is up, then asked each turn.
pub trait Scene {
    /// What the scene's widgets publish.
    type Message: fmt::Debug;
    /// What the scene's widgets are styled by, with the colours of composing text.
    type Theme: Base + ui::Composing + 'static;

    /// What the main window is called, in the scene's language: a task switcher and a screen reader say it.
    fn title(&self) -> String;
    /// The scheme the person chose for this window; the system's unless the scene lets them choose.
    fn preference(&self) -> Preference {
        Preference::System
    }
    /// How much the person zoomed the window; none unless the scene lets them choose.
    fn zoom(&self) -> Zoom {
        Zoom::DEFAULT
    }
    /// Whether the window is to be over the whole monitor; never unless the scene shows something there.
    fn full_screen(&self) -> bool {
        false
    }
    /// The windowing system gave the window back from the whole monitor by itself.
    fn full_screen_left(&mut self) {}
    /// Whether the window shows on the display: not minimised, covered whole or kept in the tray.
    fn shown(&mut self, _shown: bool) {}
    /// The window took the keyboard's focus, or lost it.
    fn focused(&mut self, _focused: bool) {}
    /// The person used the window (a key, a click, a wheel turn, a pointer move or a touch), told about once a second.
    fn typed(&mut self) {}
    /// Files are dragged over the window, or have left it.
    fn files_hovered(&mut self, _hovered: bool) {}
    /// A file let go over the window.
    fn file_dropped(&mut self, _path: std::path::PathBuf) {}
    /// Before each turn, and before [`Scene::prepare`]: the room the window gives the view, in logical pixels after
    /// the zoom, for a scene that decides what folds away.
    fn room(&mut self, _room: Size) {}
    /// Before each turn: the scheme the window is in and the motion the system asks for, for the theme and whatever
    /// the view reads.
    fn prepare(&mut self, scheme: Scheme, motion: MotionPreference);
    /// The theme to draw with now, as `prepare` left it.
    fn theme(&self) -> &Self::Theme;
    /// What to draw in a window of `window` logical pixels.
    fn view(&self, window: Size) -> Element<'_, Self::Message, Self::Theme, Renderer>;
    /// What the widgets published; true when the scene has to be drawn again.
    fn update(&mut self, messages: Vec<Self::Message>) -> bool;
    /// What other threads told the scene since the last turn; true when it has to be drawn again.
    fn drain(&mut self) -> bool {
        false
    }
    /// Once, before the first picture.
    fn opened(&mut self, _input: &mut Input<'_>) {}
    /// After each redraw.
    fn redrawn(&mut self, _redrawn: Redrawn, _input: &mut Input<'_>) -> Next {
        Next::Wait
    }
    /// The person or the system asked to close the window: whether it closes now. One kept open closes later, when
    /// [`Scene::closing`] says so.
    fn close_asked(&mut self) -> Closing {
        Closing::Now
    }
    /// Whether the window is to close now, asked before the loop waits.
    fn closing(&self) -> Closing {
        Closing::Stay
    }
    /// The window closed.
    fn closed(&mut self) {}
    /// Whether the scene is to be shown in its window or kept in the tray, asked before the loop waits.
    fn showing(&self) -> Showing {
        Showing::Window
    }
    /// A later launch asked the window to come forward, whether it was shown or kept in the tray.
    fn forwarded(&mut self) {}
    /// The GPU the window paints with was made again after it was lost: a decoder decodes onto this one from now on.
    fn repainted(&mut self, _painting: picture::Painting) {}
    /// The title of a window of its own the scene wants beside the main one, while it wants one: a watched screen,
    /// with no frame, above the other windows where the system allows it.
    fn out(&self) -> Option<String> {
        None
    }
    /// What to draw in that window, of `window` logical pixels, while there is one.
    fn view_out(&self, _window: Size) -> Option<Element<'_, Self::Message, Self::Theme, Renderer>> {
        None
    }
    /// The person or the system closed that window.
    fn out_closed(&mut self) {}
}
