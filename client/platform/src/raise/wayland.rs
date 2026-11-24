//! `xdg_activation_v1.activate` on the window's own display: winit binds the protocol but activates only with a
//! token it asked for itself, which KWin's focus-stealing prevention refuses.

use crate::Unavailable;
use crate::activation::Activation;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

/// The queue's state: neither object it binds sends an event this module reads.
struct Raising;

pub(super) fn raise(window: &(impl HasWindowHandle + HasDisplayHandle), token: &Activation) -> Result<(), Unavailable> {
    let display = window.display_handle().map_err(|_| Unavailable)?;
    let surface = window.window_handle().map_err(|_| Unavailable)?;
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(surface)) = (display.as_raw(), surface.as_raw())
    else {
        return Err(Unavailable);
    };
    // SAFETY: the display is the window's, which the borrowed handle keeps open past the backend, dropped at the end.
    let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init::<Raising>(&connection).map_err(|_| Unavailable)?;
    let activation: XdgActivationV1 = globals.bind(&queue.handle(), 1..=1, ()).map_err(|_| Unavailable)?;
    // SAFETY: the surface is the window's `wl_surface`, which the borrowed handle keeps alive while the id is used.
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) };
    let surface = id.and_then(|id| WlSurface::from_id(&connection, id)).map_err(|_| Unavailable)?;
    activation.activate(token.token().to_owned(), &surface);
    activation.destroy();
    connection.flush().map_err(|_| Unavailable)
}

impl Dispatch<WlRegistry, GlobalListContents> for Raising {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for Raising {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
