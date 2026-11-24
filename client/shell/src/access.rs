//! A window's accessibility tree, handed to the system's screen reader by AccessKit's adapter, driven from our own
//! loop since `accesskit_winit` is built on a winit other than iced's fork: on Linux over AT-SPI, on Windows over UI
//! Automation. Other systems are not told yet.

use crate::asks::Asking;
use accesskit::{ActivationHandler, TreeUpdate};
use iced_winit::core::{Event, Rectangle, mouse};
use iced_winit::winit;
use std::sync::Arc;

#[cfg(any(target_os = "linux", windows))]
mod heard;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", windows)))]
mod untold;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::Access;
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) use untold::Access;
#[cfg(windows)]
pub(crate) use windows::Access;

/// A window the system has not shown yet: Windows' adapter can be attached to no other, so every window a tree is
/// handed for is made here.
pub(crate) struct Unshown(Arc<winit::window::Window>);

impl Unshown {
    /// A window of `attributes`, made hidden whatever they say; its pane shows it once the tree's adapter is attached.
    pub(crate) fn made(
        event_loop: &winit::event_loop::ActiveEventLoop,
        attributes: winit::window::WindowAttributes,
    ) -> Result<Unshown, winit::error::OsError> {
        event_loop.create_window(attributes.with_visible(false)).map(|window| Unshown(Arc::new(window)))
    }

    /// The window, for what is made of it before it shows (its surface, its size).
    pub(crate) fn window(&self) -> &Arc<winit::window::Window> {
        &self.0
    }

    /// The window, for the pane its tree's adapter was attached for.
    pub(crate) fn into_window(self) -> Arc<winit::window::Window> {
        self.0
    }
}

/// A screen reader asked for the tree: it is built on the loop's thread, at the window's next frame.
struct Asked(Arc<Asking>);

impl ActivationHandler for Asked {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.whole();
        None
    }
}

/// What the loop does for a screen reader's request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Heard {
    /// Focus the focusable at this place among all.
    Focus(usize),
    /// Press the control drawn at these bounds.
    Press(Rectangle),
}

/// The pointer events of a press at the centre of `bounds`: the pointer there, the main button down, then up.
pub(crate) fn press_at(bounds: Rectangle) -> [Event; 3] {
    [
        Event::Mouse(mouse::Event::CursorMoved { position: bounds.center() }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]
}
