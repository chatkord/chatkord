//! What a window's host decides, as values in and out, so each decision is a test: the loop hands in each [`Event`]
//! with the time and the system's appearance, and carries out the [`Out`] that comes back.

use design::{
    Appearance, MotionPreference, POP_OUT_GRIP, POP_OUT_LEAST, POP_OUT_MOST, Preference, Scheme, WINDOW_LEAST, Zoom,
};
use iced_core::input_method::Purpose;
use iced_core::renderer::Scale;
use iced_core::time::Instant;
use iced_core::window::RedrawRequest;
use iced_core::{Point, Rectangle, Size, mouse};
use std::time::Duration;

/// The first wait after a mend that did not hold, doubled at each miss up to [`RETRY_MOST`].
const RETRY_FIRST: Duration = Duration::from_millis(100);
const RETRY_MOST: Duration = Duration::from_secs(1);

/// What happened to the window, or was asked of the loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The pointer moved, in physical pixels as the windowing system reports them.
    CursorMoved {
        /// From the window's left edge.
        x: f64,
        /// From the window's top edge.
        y: f64,
    },
    /// The pointer left the window, so nothing under it is hovered.
    CursorLeft,
    /// The window's inner size, in physical pixels.
    Resized {
        /// 0 while the window has no area, as when minimised.
        width: u32,
        /// 0 while the window has no area, as when minimised.
        height: u32,
    },
    /// The monitor's scale changed, as when the window moves to a monitor of another.
    ScaleFactorChanged {
        /// Physical pixels per logical pixel.
        scale_factor: f64,
    },
    /// The system's appearance, read now or changed.
    System(Appearance),
    /// The person's own choice of scheme.
    Preference(Preference),
    /// The person's zoom.
    Zoom(Zoom),
    /// What a turn of the interface asks for: a frame now, one later, or none until something happens.
    Updated {
        /// iced's own request, which [`Host`] keeps until it is due.
        redraw: RedrawRequest,
    },
    /// The loop woke at `now`.
    Woke {
        /// From the loop's clock, the one every deadline here is on.
        now: Instant,
    },
    /// What asking the surface for a frame came to, at `now`.
    Acquired {
        /// The surface's answer.
        acquired: Acquired,
        /// When it answered, from the loop's clock; a retry waits from here.
        now: Instant,
    },
    /// The windowing system hid the window (minimised, or wholly covered) or showed it again.
    Occluded(bool),
    /// What the scene wants: the window over the whole monitor, or as it was.
    FullScreen(bool),
    /// Whether the windowing system says the window is over the whole monitor, read after each resize.
    Fullness(bool),
}

/// What the loop does to the window's full screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullScreen {
    /// Ask the windowing system for the whole monitor.
    Enter,
    /// Give the window back as it was.
    Leave,
    /// The windowing system gave the window back by itself: tell the scene.
    Left,
}

/// Where the window's full screen stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fullness {
    Windowed,
    /// Asked for, and not yet said to be: a resize can come before the windowing system makes it so.
    Asked,
    Shown,
}

/// The surface's answer to a frame asked of it, one for each of the graphics library's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquired {
    /// A frame was given and is drawn into.
    Drawn,
    /// Drawn, on a surface that no longer matches the window.
    Suboptimal,
    /// No frame came in time.
    Timeout,
    /// The window is hidden, so no frame was given.
    Occluded,
    /// The window changed under the surface.
    Outdated,
    /// The surface is gone and must be made again.
    Lost,
    /// The graphics library refused the request.
    Invalid,
}

/// What the surface needs before the next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mend {
    /// Nothing: the next frame is asked for as it is.
    Keep,
    /// Configured again at the window's size.
    Configure,
    /// Made again from the window, then configured.
    Recreate,
}

/// What the loop must do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    /// The viewport and the surface are out of date: rebuild them from [`Host::size`] and [`Host::scale`].
    pub viewport: bool,
    /// The scheme the interface is now drawn in, when it changed.
    pub scheme: Option<Scheme>,
    /// What the surface needs before the next frame.
    pub surface: Mend,
    /// Draw now. When to wake to draw later is [`Host::due`], asked before every sleep.
    pub redraw: bool,
    /// What to do to the window's full screen, when something is.
    pub full_screen: Option<FullScreen>,
}

impl Out {
    const NOTHING: Out = Out { viewport: false, scheme: None, surface: Mend::Keep, redraw: false, full_screen: None };
}

/// The smallest a window may be made, in logical pixels, unzoomed.
pub fn min_window() -> Size {
    Size::new(WINDOW_LEAST.0, WINDOW_LEAST.1)
}

/// A watched screen's own window as it opens on a display `display` logical pixels wide: a quarter of it, in the
/// picture's 16:9, between [`POP_OUT_LEAST`] and [`POP_OUT_MOST`].
pub fn pop_out_size(display: f32) -> Size {
    let width = (display / 4.0).clamp(POP_OUT_LEAST.0, POP_OUT_MOST.0);
    Size::new(width, width * 9.0 / 16.0)
}

/// What a press does in a window with no frame: inside, a drag moves the window; within [`POP_OUT_GRIP`] of an edge,
/// it resizes the window from there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// A drag moves the window.
    Move,
    /// A drag resizes the window from this edge or corner.
    Resize(Edge),
}

/// An edge or a corner of a window, by the compass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The top.
    North,
    /// The bottom.
    South,
    /// The right.
    East,
    /// The left.
    West,
    /// The top right.
    NorthEast,
    /// The top left.
    NorthWest,
    /// The bottom right.
    SouthEast,
    /// The bottom left.
    SouthWest,
}

/// What a press at `at` does in a frameless window of `size`, both in logical pixels. A window narrower than two grips
/// is resized from the left and the top first.
pub fn grip(at: Point, size: Size) -> Grip {
    let west = at.x < POP_OUT_GRIP;
    let east = !west && at.x >= size.width - POP_OUT_GRIP;
    let north = at.y < POP_OUT_GRIP;
    let south = !north && at.y >= size.height - POP_OUT_GRIP;
    Grip::Resize(match (north, south, west, east) {
        (true, _, true, _) => Edge::NorthWest,
        (true, _, _, true) => Edge::NorthEast,
        (_, true, true, _) => Edge::SouthWest,
        (_, true, _, true) => Edge::SouthEast,
        (true, ..) => Edge::North,
        (_, true, ..) => Edge::South,
        (.., true, _) => Edge::West,
        (.., true) => Edge::East,
        _ => return Grip::Move,
    })
}

/// What the host knows of its window, which each [`Event`] changes.
#[derive(Clone, Debug)]
pub struct Host {
    cursor: mouse::Cursor,
    scale_factor: f64,
    zoom: Zoom,
    size: Size<u32>,
    system: Appearance,
    preference: Preference,
    scheme: Scheme,
    wake_at: Option<Instant>,
    /// Frames the surface did not give since the last it did, and when the next is asked for.
    missed: u32,
    retry_at: Option<Instant>,
    occluded: bool,
    fullness: Fullness,
    /// The least the window may be made, unzoomed.
    least: Size,
}

impl Host {
    /// A window of `size` physical pixels at `scale_factor`, drawn in the system's scheme until the person's own
    /// choice is told.
    pub fn new(size: Size<u32>, scale_factor: f64, system: Appearance) -> Host {
        let preference = Preference::System;
        Host {
            cursor: mouse::Cursor::Unavailable,
            scale_factor,
            zoom: Zoom::DEFAULT,
            size,
            system,
            preference,
            scheme: preference.resolve(system),
            wake_at: None,
            missed: 0,
            retry_at: None,
            occluded: false,
            fullness: Fullness::Windowed,
            least: min_window(),
        }
    }

    /// The same, for a window that may be made as small as `least` logical pixels, unzoomed, where the main window's
    /// interface needs [`min_window`].
    pub fn with_least(self, least: Size) -> Host {
        Host { least, ..self }
    }

    /// Whether the window is over the whole monitor, or asked to be.
    pub fn full_screen(&self) -> bool {
        self.fullness != Fullness::Windowed
    }

    /// Where the pointer is, in logical pixels, or nowhere.
    pub fn cursor(&self) -> mouse::Cursor {
        self.cursor
    }

    /// The window's inner size, in physical pixels.
    pub fn size(&self) -> Size<u32> {
        self.size
    }

    /// The monitor's scale factor, as the windowing system last said it.
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// The person's zoom.
    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    /// What the interface is drawn at: the monitor's scale times the zoom, which the viewport, the pointer and the
    /// least window all read, so a move to a monitor of another scale keeps the zoom.
    #[expect(clippy::cast_possible_truncation, reason = "iced draws in f32, which holds any monitor's scale")]
    pub fn scale(&self) -> Scale {
        Scale { window: self.scale_factor as f32, application: self.zoom.factor() }
    }

    /// The smallest the window may be made, in the windowing system's logical pixels: what the interface needs, zoomed.
    pub fn least(&self) -> Size {
        self.least * self.zoom.factor()
    }

    /// The scheme the interface is drawn in now: the person's choice, else the system's.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// The person's own choice of scheme, as last told.
    pub fn preference(&self) -> Preference {
        self.preference
    }

    /// Whether the system asks for less motion.
    pub fn motion(&self) -> MotionPreference {
        self.system.motion
    }

    /// Whether the window shows nothing to draw: hidden, or of no area.
    pub fn hidden(&self) -> bool {
        self.occluded || self.size.width == 0 || self.size.height == 0
    }

    /// When the loop must wake by itself to draw: the soonest of iced's wake and the surface's next try, and never
    /// while hidden. The loop asks before every sleep, so no event's answer can forget a wake.
    pub fn due(&self) -> Option<Instant> {
        if self.hidden() {
            return None;
        }
        [self.wake_at, self.retry_at].into_iter().flatten().min()
    }

    /// What the loop must do after `event`; a hidden window is drawn only once it shows.
    pub fn on(&mut self, event: Event) -> Out {
        let out = self.decide(event);
        // Kept for when it shows: that redraws whatever was due.
        Out { redraw: out.redraw && !self.hidden(), ..out }
    }

    fn decide(&mut self, event: Event) -> Out {
        match event {
            Event::CursorMoved { x, y } => {
                let scale = f64::from(self.scale().total());
                #[expect(clippy::cast_possible_truncation, reason = "iced places the pointer in f32 logical pixels")]
                let at = Point::new((x / scale) as f32, (y / scale) as f32);
                self.cursor = mouse::Cursor::Available(at);
                Out::NOTHING
            }
            Event::CursorLeft => {
                self.cursor = mouse::Cursor::Unavailable;
                Out { redraw: true, ..Out::NOTHING }
            }
            Event::Resized { width, height } => {
                self.size = Size::new(width, height);
                Out { viewport: !self.hidden(), redraw: true, ..Out::NOTHING }
            }
            Event::Occluded(occluded) => {
                self.occluded = occluded;
                Out { redraw: true, ..Out::NOTHING }
            }
            Event::Acquired { acquired, now } => self.acquired(acquired, now),
            Event::FullScreen(wanted) => {
                let (fullness, told) = match (wanted, self.fullness) {
                    (true, Fullness::Windowed) => (Fullness::Asked, Some(FullScreen::Enter)),
                    (false, Fullness::Asked | Fullness::Shown) => (Fullness::Windowed, Some(FullScreen::Leave)),
                    (_, fullness) => (fullness, None),
                };
                self.fullness = fullness;
                Out { full_screen: told, ..Out::NOTHING }
            }
            Event::Fullness(full) => {
                let (fullness, told) = match (full, self.fullness) {
                    (true, Fullness::Asked) => (Fullness::Shown, None),
                    (false, Fullness::Shown) => (Fullness::Windowed, Some(FullScreen::Left)),
                    (_, fullness) => (fullness, None),
                };
                self.fullness = fullness;
                Out { full_screen: told, ..Out::NOTHING }
            }
            Event::ScaleFactorChanged { scale_factor } => {
                self.scale_factor = scale_factor;
                Out { viewport: true, redraw: true, ..Out::NOTHING }
            }
            Event::System(appearance) => {
                self.system = appearance;
                self.restyle()
            }
            Event::Preference(preference) => {
                self.preference = preference;
                self.restyle()
            }
            Event::Zoom(zoom) if zoom == self.zoom => Out::NOTHING,
            Event::Zoom(zoom) => {
                self.zoom = zoom;
                Out { viewport: true, redraw: true, ..Out::NOTHING }
            }
            Event::Updated { redraw } => match redraw {
                RedrawRequest::Wait => {
                    self.wake_at = None;
                    Out::NOTHING
                }
                RedrawRequest::NextFrame => {
                    self.wake_at = None;
                    Out { redraw: true, ..Out::NOTHING }
                }
                RedrawRequest::At(at) => {
                    self.wake_at = Some(at);
                    Out::NOTHING
                }
            },
            Event::Woke { now } => {
                let due = |at: Option<Instant>| at.filter(|at| *at <= now);
                if due(self.wake_at).or(due(self.retry_at)).is_none() {
                    return Out::NOTHING;
                }
                self.wake_at = self.wake_at.filter(|at| *at > now);
                self.retry_at = self.retry_at.filter(|at| *at > now);
                Out { redraw: true, ..Out::NOTHING }
            }
        }
    }

    /// Asked for again at once after a first miss, then later and later; mended as its outcome says.
    fn acquired(&mut self, acquired: Acquired, now: Instant) -> Out {
        let surface = match acquired {
            Acquired::Drawn | Acquired::Suboptimal => {
                self.missed = 0;
                self.retry_at = None;
                let surface = if acquired == Acquired::Suboptimal { Mend::Configure } else { Mend::Keep };
                return Out { surface, ..Out::NOTHING };
            }
            Acquired::Outdated => Mend::Configure,
            Acquired::Lost => Mend::Recreate,
            Acquired::Timeout | Acquired::Occluded | Acquired::Invalid => Mend::Keep,
        };
        self.missed = self.missed.saturating_add(1);
        if self.missed == 1 {
            return Out { surface, redraw: true, ..Out::NOTHING };
        }
        let wait = RETRY_FIRST.saturating_mul(1 << (self.missed - 2).min(4)).min(RETRY_MOST);
        self.retry_at = Some(now + wait);
        Out { surface, ..Out::NOTHING }
    }

    /// The scheme from the person's choice, else the system's, else dark; drawn again only if it changed.
    fn restyle(&mut self) -> Out {
        let scheme = self.preference.resolve(self.system);
        if scheme == self.scheme {
            return Out::NOTHING;
        }
        self.scheme = scheme;
        Out { scheme: Some(scheme), redraw: true, ..Out::NOTHING }
    }
}

/// Where an input method composes as the windowing system was last told: the focused field's caret and what it
/// is for, or nowhere. A window that tells it again every frame moves the system's candidate window for nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputMethodPlace {
    told: Option<(Rectangle, Purpose)>,
}

/// What to tell the windowing system now; `None` where it already knows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tell {
    /// Whether an input method may compose at all.
    pub allowed: Option<bool>,
    /// Where and for what it composes.
    pub place: Option<(Rectangle, Purpose)>,
}

impl InputMethodPlace {
    /// iced asked for an input method at `asked` after an update that was `drawn` or not. A field asks only as it is
    /// drawn, so an update that drew nothing says nothing: else every key would end what was being composed.
    pub fn asked(&mut self, drawn: bool, asked: Option<(Rectangle, Purpose)>) -> Tell {
        if !drawn {
            return Tell::default();
        }
        let allowed = (self.told.is_some() != asked.is_some()).then_some(asked.is_some());
        let place = asked.filter(|_| asked != self.told);
        self.told = asked;
        Tell { allowed, place }
    }
}
