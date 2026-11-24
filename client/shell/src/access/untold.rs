//! A system whose screen reader is not told of the window yet: nothing is built and nothing is sent.

use super::Unshown;
use iced_winit::winit;

/// A window's tree, which no screen reader is handed here.
pub(crate) struct Access;

impl Access {
    /// Nothing to hand anything to.
    pub(crate) fn new(_window: &Unshown, _asking: std::sync::Arc<crate::asks::Asking>) -> Access {
        Access
    }

    /// `make` is never called.
    pub(crate) fn update(&mut self, _make: impl FnOnce() -> (accesskit::TreeUpdate, ui::access::Controls)) {}

    /// Nothing is asked of a window no screen reader reads.
    pub(crate) fn heard(&mut self) -> Vec<super::Heard> {
        Vec::new()
    }

    /// Nothing is told.
    pub(crate) fn focused(&mut self, _focused: bool) {}

    /// Nothing is told.
    pub(crate) fn placed(&mut self, _window: &winit::window::Window) {}
}
