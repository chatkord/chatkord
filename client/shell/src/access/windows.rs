//! UI Automation, through AccessKit's adapter subclassing the window: the tree is built only while a client of it (a
//! screen reader) reads the window, and the reader's requests (focus a control, press it) go to the window's loop.

use super::heard::{self, Heeded};
use super::{Asked, Heard, Unshown};
use crate::asks::Asking;
use accesskit::{ActionRequest, TreeUpdate};
use accesskit_windows::{HWND, SubclassingAdapter};
use iced_winit::winit;
use iced_winit::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use ui::access::Controls;

/// A window's tree as the system's screen reader is handed it.
pub(crate) struct Access {
    /// The adapter in the window's procedure; none when the system named no handle for the window.
    adapter: Option<SubclassingAdapter>,
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
        let Some(hwnd) = handle(window) else {
            report::note!(warn, "the window has no handle, so no screen reader is told of it");
            return Access { adapter: None, requests, controls: Controls::new() };
        };
        Access {
            adapter: Some(SubclassingAdapter::new(hwnd, Asked(asking), heeded)),
            requests,
            controls: Controls::new(),
        }
    }

    /// Hands over the tree `make` builds, while a screen reader reads the window; with none, `make` is not called.
    pub(crate) fn update(&mut self, make: impl FnOnce() -> (TreeUpdate, Controls)) {
        let controls = &mut self.controls;
        let made = || {
            let (tree, made) = make();
            *controls = made;
            tree
        };
        if let Some(events) = self.adapter.as_mut().and_then(|adapter| adapter.update_if_active(made)) {
            events.raise();
        }
    }

    /// What the reader asked since the last call, as the loop carries it out.
    pub(crate) fn heard(&mut self) -> Vec<Heard> {
        heard::drain(&self.requests, &self.controls)
    }

    /// The adapter hears the window's focus in its procedure.
    pub(crate) fn focused(&mut self, _focused: bool) {}

    /// The adapter asks the window where it is when a reader wants a control's place.
    pub(crate) fn placed(&mut self, _window: &winit::window::Window) {}
}

/// The window's handle, as the system's own calls take it.
fn handle(window: &winit::window::Window) -> Option<HWND> {
    let RawWindowHandle::Win32(win32) = window.window_handle().ok()?.as_raw() else { return None };
    Some(HWND(std::ptr::with_exposed_provenance_mut(win32.hwnd.get().cast_unsigned())))
}
