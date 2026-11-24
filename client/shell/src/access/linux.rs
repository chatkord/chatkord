//! AT-SPI, through AccessKit's adapter: the tree is built only while a screen reader reads the window, and the reader's
//! requests (focus a control, press it) go to the window's loop.

use super::heard::{self, Heeded};
use super::{Asked, Heard, Unshown};
use crate::asks::Asking;
use accesskit::{ActionRequest, DeactivationHandler, Rect, TreeUpdate};
use iced_winit::winit;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use ui::access::Controls;

/// A window's tree as the system's screen reader is handed it.
pub(crate) struct Access {
    adapter: accesskit_unix::Adapter,
    /// What the reader asked since the loop last looked.
    requests: Receiver<ActionRequest>,
    /// Where the controls of the last tree are, to carry the requests out.
    controls: Controls,
}

impl Access {
    /// The tree of `window`, handed over once a screen reader asks for it, which asks `asking` for a whole redraw.
    pub(crate) fn new(window: &Unshown, asking: Arc<Asking>) -> Access {
        let window = window.window();
        let (heeded, requests) = Heeded::channel(asking.clone());
        let adapter = accesskit_unix::Adapter::new(Asked(asking), heeded, Forgotten);
        let mut access = Access { adapter, requests, controls: Controls::new() };
        access.placed(window);
        access
    }

    /// Hands over the tree `make` builds, while a screen reader reads the window; with none, `make` is not called.
    pub(crate) fn update(&mut self, make: impl FnOnce() -> (TreeUpdate, Controls)) {
        let controls = &mut self.controls;
        self.adapter.update_if_active(|| {
            let (tree, made) = make();
            *controls = made;
            tree
        });
    }

    /// What the reader asked since the last call, as the loop carries it out.
    pub(crate) fn heard(&mut self) -> Vec<Heard> {
        heard::drain(&self.requests, &self.controls)
    }

    /// The window took or lost the keyboard focus.
    pub(crate) fn focused(&mut self, focused: bool) {
        self.adapter.update_window_focus_state(focused);
    }

    /// Where the window is on the screen, so a reader can point at a control. Wayland tells no window where it is:
    /// there a reader has the controls' places inside the window only.
    pub(crate) fn placed(&mut self, window: &winit::window::Window) {
        let (Ok(outer), Ok(inner)) = (window.outer_position(), window.inner_position()) else { return };
        let (outer, inner) = (frame(outer, window.outer_size()), frame(inner, window.inner_size()));
        self.adapter.set_root_window_bounds(outer, inner);
    }
}

/// The rectangle at `at`, `size` big, in the screen's pixels.
fn frame(at: winit::dpi::PhysicalPosition<i32>, size: winit::dpi::PhysicalSize<u32>) -> Rect {
    let (x, y) = (f64::from(at.x), f64::from(at.y));
    Rect::new(x, y, x + f64::from(size.width), y + f64::from(size.height))
}

/// The screen reader went: the tree is kept by no one, so nothing is dropped.
struct Forgotten;

impl DeactivationHandler for Forgotten {
    fn deactivate_accessibility(&mut self) {}
}
