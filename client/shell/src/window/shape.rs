//! How the main window stands and what a window is made as: its attributes, icon and the shape kept between runs.

use iced_winit::winit;
#[cfg(windows)]
use iced_winit::winit::platform::windows::WindowAttributesExtWindows;
#[cfg(target_os = "linux")]
use iced_winit::winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
use ui::host;

/// The application id a desktop groups the window under (Wayland's app id, X11's class), as the desktop entry will name it.
#[cfg(target_os = "linux")]
const APP_ID: &str = "chatkord";

/// A window's place and size, or a monitor's, in physical pixels.
pub(super) type Placed = (winit::dpi::PhysicalPosition<i32>, winit::dpi::PhysicalSize<u32>);

/// Whether a window whose outer frame is `outer` covers the whole of `monitor`: the same place and the same size.
/// Where winit reports the state last asked for, this tells a window the window manager gave back from one it did not.
pub(super) fn covers(outer: Placed, monitor: Placed) -> bool {
    outer == monitor
}

/// How the main window stands, which the window made when it is shown from the tray again takes on: the size and
/// place it has when neither maximized nor over the whole monitor, so it can be given back from either.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shape {
    /// Logical, since a new window learns its monitor's scale only once it is shown.
    pub(super) size: winit::dpi::LogicalSize<f64>,
    /// None where the system places windows itself, as Wayland does.
    pub(super) position: Option<winit::dpi::PhysicalPosition<i32>>,
    pub(super) maximized: bool,
}

impl Shape {
    /// The shape `window` has now.
    pub(super) fn of(window: &winit::window::Window) -> Self {
        Self {
            size: window.inner_size().to_logical(window.scale_factor()),
            position: window.outer_position().ok(),
            maximized: window.is_maximized(),
        }
    }

    /// The window was made `size` at `scale_factor`, kept unless it is maximized or over the whole monitor (`normal`
    /// false), whose size is the monitor's.
    pub(super) fn resized(&mut self, size: winit::dpi::PhysicalSize<u32>, scale_factor: f64, normal: bool) {
        if normal {
            self.size = size.to_logical(scale_factor);
        }
    }

    /// The window was moved to `position`, kept as [`Shape::resized`] keeps a size.
    pub(super) fn moved(&mut self, position: winit::dpi::PhysicalPosition<i32>, normal: bool) {
        if normal {
            self.position = Some(position);
        }
    }

    /// The shape kept for the next run, on the monitor `window` stands on.
    fn kept(self, window: &winit::window::Window) -> platform::window::Placement {
        platform::window::Placement {
            width: self.size.width,
            height: self.size.height,
            at: self.position.map(|position| (position.x, position.y)),
            maximized: self.maximized,
            monitor: window.current_monitor().as_ref().map(monitor),
        }
    }
}

impl From<platform::window::Placement> for Shape {
    fn from(placement: platform::window::Placement) -> Self {
        Self {
            size: winit::dpi::LogicalSize::new(placement.width, placement.height),
            position: placement.at.map(|(x, y)| winit::dpi::PhysicalPosition::new(x, y)),
            maximized: placement.maximized,
        }
    }
}

/// A monitor as `platform::window` keeps it.
fn monitor(handle: &winit::monitor::MonitorHandle) -> platform::window::Monitor {
    let (at, size) = (handle.position(), handle.size());
    platform::window::Monitor { x: at.x, y: at.y, width: size.width, height: size.height, scale: handle.scale_factor() }
}

/// The shape the last run's window went at, fitted to the monitors there now; none before a first run ends.
pub(super) fn last_shape(
    platform: &platform::Platform,
    event_loop: &winit::event_loop::ActiveEventLoop,
) -> Option<Shape> {
    let monitors: Vec<_> = event_loop.available_monitors().map(|handle| monitor(&handle)).collect();
    platform::window::placement(platform, &monitors).map(Shape::from)
}

/// Keeps the main window's `shape` for the next run; a failure only costs the next run its place.
pub(super) fn keep_shape(platform: &platform::Platform, shape: Shape, window: &winit::window::Window) {
    if platform::window::keep(platform, shape.kept(window)).is_err() {
        report::note!(warn, "the window's place could not be kept for the next run");
    }
}

/// What the window is: its title, its icon, the id a desktop knows it by, the least it may be made and, shown again,
/// the shape it had. Decorations are the system's (Windows, X11, a compositor that draws them), and client-side
/// (`wayland-decorations`) where it does not.
pub(super) fn window_attributes(shape: Option<Shape>, title: String) -> winit::window::WindowAttributes {
    let least = host::min_window();
    let attributes = winit::window::WindowAttributes::default()
        .with_title(title)
        .with_window_icon(icon(64))
        .with_min_inner_size(winit::dpi::LogicalSize::new(least.width, least.height));
    let attributes = match shape {
        Some(Shape { size, position: Some(position), maximized }) => {
            attributes.with_inner_size(size).with_position(position).with_maximized(maximized)
        }
        Some(Shape { size, position: None, maximized }) => attributes.with_inner_size(size).with_maximized(maximized),
        None => attributes,
    };
    // The taskbar shows its button larger than the title bar's icon.
    #[cfg(windows)]
    let attributes = WindowAttributesExtWindows::with_taskbar_icon(attributes, icon(256));
    #[cfg(target_os = "linux")]
    let attributes = {
        let attributes = WindowAttributesExtWayland::with_name(attributes, APP_ID, APP_ID);
        WindowAttributesExtX11::with_name(attributes, APP_ID, APP_ID)
    };
    attributes
}

/// The app icon `side` pixels square. Wayland takes none from a window: its desktop entry names it.
fn icon(side: u32) -> Option<winit::window::Icon> {
    let drawn = ui::app_icon::rgba(side).map_err(|_| report::note!(error, "the window's icon was not drawn")).ok()?;
    // winit's error is its own text.
    winit::window::Icon::from_rgba(drawn, side, side)
        .map_err(|_| report::note!(error, "the window's icon was refused"))
        .ok()
}

/// The windowing system's name for an edge the host chose.
pub(super) fn resize_direction(edge: host::Edge) -> winit::window::ResizeDirection {
    use winit::window::ResizeDirection as To;
    match edge {
        host::Edge::North => To::North,
        host::Edge::South => To::South,
        host::Edge::East => To::East,
        host::Edge::West => To::West,
        host::Edge::NorthEast => To::NorthEast,
        host::Edge::NorthWest => To::NorthWest,
        host::Edge::SouthEast => To::SouthEast,
        host::Edge::SouthWest => To::SouthWest,
    }
}

/// What a watched screen's own window is: titled as the scene says, no frame, above the others where the system lets
/// a program ask (Windows, X11; Wayland lets it not), a quarter of the display it opens on, and its own least size.
pub(super) fn out_attributes(title: String, display: f32) -> winit::window::WindowAttributes {
    let size = host::pop_out_size(display);
    let least = design::POP_OUT_LEAST;
    let attributes = winit::window::WindowAttributes::default()
        .with_title(title)
        .with_window_icon(icon(64))
        .with_decorations(false)
        .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
        .with_inner_size(winit::dpi::LogicalSize::new(size.width, size.height))
        .with_min_inner_size(winit::dpi::LogicalSize::new(least.0, least.1));
    #[cfg(target_os = "linux")]
    let attributes = {
        let attributes = WindowAttributesExtWayland::with_name(attributes, APP_ID, APP_ID);
        WindowAttributesExtX11::with_name(attributes, APP_ID, APP_ID)
    };
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::{PhysicalPosition, PhysicalSize};

    #[test]
    fn a_window_covers_a_monitor_only_in_its_place_and_at_its_size() {
        let monitor = (PhysicalPosition::new(1920, 0), PhysicalSize::new(2560, 1440));
        assert!(covers(monitor, monitor));
        assert!(!covers((PhysicalPosition::new(1920, 0), PhysicalSize::new(1280, 800)), monitor), "a smaller window");
        assert!(!covers((PhysicalPosition::new(0, 0), PhysicalSize::new(2560, 1440)), monitor), "on another monitor");
        assert!(!covers((PhysicalPosition::new(1930, 10), PhysicalSize::new(2560, 1440)), monitor), "moved");
    }

    #[test]
    fn a_window_shown_again_opens_at_the_shape_it_had_before_it_was_maximized() {
        let (size, at) = (winit::dpi::LogicalSize::new(1000.0, 600.0), winit::dpi::PhysicalPosition::new(40, 30));
        let mut shape = Shape { size, position: Some(at), maximized: false };
        shape.resized(winit::dpi::PhysicalSize::new(3840, 2052), 1.5, false);
        shape.moved(winit::dpi::PhysicalPosition::new(0, 0), false);
        assert_eq!(shape, Shape { size, position: Some(at), maximized: false }, "maximized, the shape is kept");
        shape.resized(winit::dpi::PhysicalSize::new(1800, 1200), 1.5, true);
        shape.moved(winit::dpi::PhysicalPosition::new(90, 60), true);
        let (size, at) = (winit::dpi::LogicalSize::new(1200.0, 800.0), winit::dpi::PhysicalPosition::new(90, 60));
        assert_eq!(shape, Shape { size, position: Some(at), maximized: false });
        let again = window_attributes(Some(Shape { maximized: true, ..shape }), "Chatkord".into());
        assert_eq!(again.title, "Chatkord");
        assert_eq!(again.inner_size, Some(winit::dpi::Size::Logical(size)));
        assert_eq!(again.position, Some(winit::dpi::Position::Physical(at)));
        assert!(again.maximized);
        let placed = window_attributes(Some(Shape { position: None, ..shape }), String::new());
        assert_eq!(placed.position, None, "where the system places windows");
        let first = window_attributes(None, String::new());
        assert_eq!((first.inner_size, first.position, first.maximized), (None, None, false), "the system's");
    }
}
