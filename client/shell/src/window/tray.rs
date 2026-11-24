//! A run kept in the tray: its windows gone, its scene running, and the way back to a window.

use std::sync::{Arc, Mutex};

use design::Appearance;
use iced_wgpu::Renderer;
use iced_winit::winit;
use platform::appearance;
use ui::host::Host;

use super::Scene;
use super::gpu::{Gpu, Made};
use super::pane::Pane;
use super::ready::Ready;
use super::shape::{Shape, keep_shape, window_attributes};
use super::unhosted::Unhosted;
use crate::access::Unshown;
use crate::asks::Asking;
use crate::lost::Mends;
use crate::typing::Throttle;

/// A run whose scene is kept in the tray: everything of [`Ready`] but its windows, which Wayland cannot hide and so are
/// made again when the scene is shown. The interface's own cache goes with the window.
pub(super) struct Hidden<S> {
    pub(super) scene: S,
    /// The main window's host, which its next window takes on.
    host: Host,
    /// The main window's shape, which its next window opens at.
    shape: Shape,
    main_asking: Arc<Asking>,
    out_asking: Arc<Asking>,
    system: Arc<Mutex<Option<Appearance>>>,
    _watch: appearance::Watch,
    mends: Mends,
    /// The main window's renderer and so its engine, kept so a window coming back from the tray builds no new one.
    renderer: Renderer,
    /// The desktop's token to take the focus with, handed over while in the tray: the next window opens with it.
    pub(super) activation: Option<platform::activation::Activation>,
    platform: platform::Platform,
    /// Last, so it ends after everything that holds its device.
    gpu: Gpu,
}

impl<S: Scene> Hidden<S> {
    /// The run shown again in a new main window, on the same GPU, renderer and host; the screen's own window follows
    /// the scene on the next turn. A device lost in the meantime is made again, with a new renderer. A window that
    /// cannot be made leaves the run in the tray; a device lost again and again is the run's end.
    pub(super) fn shown(
        mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<Ready<S>, (Box<Hidden<S>>, Unhosted)> {
        let title = self.scene.title();
        let lost = self.gpu.losses.seen();
        if lost && !self.mends.allow(std::time::Instant::now()) {
            return Err((Box::new(self), Unhosted::DeviceLostAgain));
        }
        let attributes = with_activation(window_attributes(Some(self.shape), title.clone()), self.activation.take());
        let made = Unshown::made(event_loop, attributes).map_err(|_| Unhosted::NoWindow).and_then(|window| {
            let surface = self.gpu.instance.create_surface(window.window().clone()).map_err(|_| Unhosted::NoSurface)?;
            Ok((window, surface))
        });
        let (window, surface) = match made {
            Ok(made) => made,
            Err(why) => return Err((Box::new(self), why)),
        };
        let remade = if lost {
            match self.gpu.again(&surface) {
                Ok(made) => Some(made),
                Err(why) => return Err((Box::new(self), why)),
            }
        } else {
            None
        };
        let Hidden {
            gpu,
            mut scene,
            host,
            shape: _,
            main_asking,
            out_asking,
            system,
            _watch,
            mends,
            renderer,
            activation: _,
            platform,
        } = self;
        let (gpu, mut main) = match remade {
            Some(Made { gpu: new, node, .. }) => {
                drop(renderer);
                scene.repainted(picture::Painting::of(&new.device, node));
                drop(gpu);
                let main = Pane::new(&new, window, surface, host, main_asking);
                (new, main)
            }
            None => {
                let main = Pane::with_renderer(&gpu, renderer, window, surface, host, main_asking);
                (gpu, main)
            }
        };
        main.name(title);
        main.follow_look(&gpu, scene.preference(), scene.zoom());
        main.show();
        main.asking.whole();
        Ok(Ready {
            platform,
            gpu,
            scene,
            main,
            out: None,
            out_asking,
            system,
            _watch,
            occluded: false,
            stalled: false,
            typed: Throttle::default(),
            mends,
        })
    }
}

impl<S: Scene> Ready<S> {
    /// The run kept in the tray: both windows and their surfaces dropped, so the desktop shows none.
    pub(super) fn hidden(self) -> Hidden<S> {
        let Ready { gpu, scene, main, out, out_asking, system, _watch, mends, platform, .. } = self;
        out_asking.close();
        drop(out);
        let shape = Shape { maximized: main.window.is_maximized(), ..main.shape };
        keep_shape(&platform, shape, &main.window);
        let Pane { host, asking: main_asking, renderer, .. } = main;
        main_asking.close();
        Hidden {
            gpu,
            scene,
            host,
            shape,
            main_asking,
            out_asking,
            system,
            _watch,
            mends,
            renderer,
            activation: None,
            platform,
        }
    }
}

/// `attributes` with the desktop's token, so the window opens with the focus.
#[cfg(target_os = "linux")]
fn with_activation(
    attributes: winit::window::WindowAttributes,
    activation: Option<platform::activation::Activation>,
) -> winit::window::WindowAttributes {
    use winit::platform::startup_notify::WindowAttributesExtStartupNotify;
    match activation {
        Some(token) => attributes.with_activation_token(winit::window::ActivationToken::from_raw(token.token().into())),
        None => attributes,
    }
}

/// `attributes` as they are: Windows hands a window the foreground by its process, not a token.
#[cfg(not(target_os = "linux"))]
fn with_activation(
    attributes: winit::window::WindowAttributes,
    _activation: Option<platform::activation::Activation>,
) -> winit::window::WindowAttributes {
    attributes
}
