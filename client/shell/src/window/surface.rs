//! A pane's surface: keeping it configured, taking and presenting its frames, and what its host says to do.

use std::time::{Duration, Instant as Clock};

use iced_wgpu::{Renderer, wgpu};
use iced_winit::winit;
use ui::host::{self, Acquired, Mend};

use super::gpu::{Gpu, configure, engine, viewport};
use super::pane::Pane;
use crate::asks::{self, Look};
use crate::painter;

impl Pane {
    /// Takes what the host says to do: rebuild the viewport and the surface, draw now, or wake at a time. True when
    /// the windowing system gave the window back from the whole monitor by itself, which the scene is told.
    pub(super) fn carry_out(&mut self, gpu: &Gpu, out: host::Out) -> bool {
        match out.surface {
            Mend::Keep => {}
            Mend::Configure => self.configure_again(gpu),
            Mend::Recreate => match gpu.instance.create_surface(self.window.clone()) {
                Ok(surface) => {
                    self.surface = surface;
                    self.configure_again(gpu);
                }
                Err(_) => report::note!(warn, "the window's surface could not be made again"),
            },
        }
        if out.viewport {
            self.viewport = viewport(&self.window, self.host.scale(), self.limit);
            self.fit_surface(gpu, self.window.inner_size());
            self.hold_least(gpu);
        }
        let left = match out.full_screen {
            None => false,
            Some(host::FullScreen::Enter) => {
                // A maximized window is given back maximized by the system; a snapped one only as it was before
                // the snap, so its frame is kept here.
                self.before_full = match self.window.is_maximized() {
                    true => None,
                    false => self
                        .window
                        .outer_position()
                        .ok()
                        .map(|position| Frame { position, size: self.window.inner_size() }),
                };
                self.window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                false
            }
            Some(host::FullScreen::Leave) => {
                self.window.set_fullscreen(None);
                if let Some(frame) = self.before_full.take() {
                    self.window.set_outer_position(frame.position);
                    // A size the system refuses keeps the one it restored.
                    let _ = self.window.request_inner_size(frame.size);
                }
                false
            }
            // The windowing system's own state is brought to what the window shows, so the next Enter asks again; the
            // system placed the window itself.
            Some(host::FullScreen::Left) => {
                self.before_full = None;
                self.window.set_fullscreen(None);
                true
            }
        };
        if out.redraw {
            self.asking.whole();
        }
        left
    }

    /// Configures the surface at the window's size whatever it was configured at: the surface itself changed, or its
    /// configuration went stale. A window of no area leaves it to be configured before its next frame.
    pub(super) fn configure_again(&mut self, gpu: &Gpu) {
        self.configured = Configured::default();
        self.fit_surface(gpu, self.window.inner_size());
    }

    /// Configures the surface again only when its size changed: a new zoom or scale keeps the pixels, and a surface
    /// configured during a frame loses that frame. A window of no area keeps the last.
    pub(super) fn fit_surface(&mut self, gpu: &Gpu, size: winit::dpi::PhysicalSize<u32>) {
        if let Some(size) = self.configured.wanted(gpu.drawable(size)) {
            let outcome = configure(&self.surface, &gpu.device, gpu.format, gpu.present_mode, size);
            if outcome.is_err() {
                // wgpu's text can name the adapter: only that it came is kept.
                report::note!(warn, "the window's surface was refused its configuration");
            }
            self.configured = Configured::after(size, outcome);
        }
    }

    /// Gives the window the least size the zoom needs, and grows it to that where it is smaller, so a dialog still fits.
    fn hold_least(&mut self, gpu: &Gpu) {
        let least = self.host.least();
        if least == self.least {
            return;
        }
        self.least = least;
        self.window.set_min_inner_size(Some(winit::dpi::LogicalSize::new(least.width, least.height)));
        let now = self.window.inner_size().to_logical::<f32>(self.window.scale_factor());
        if now.width < least.width || now.height < least.height {
            let grown = winit::dpi::LogicalSize::new(now.width.max(least.width), now.height.max(least.height));
            // Applied at once where the system lets the program size it, with no resize to follow.
            if let Some(size) = self.window.request_inner_size(grown).map(|size| gpu.drawable(size)) {
                self.host.on(host::Event::Resized { width: size.width, height: size.height });
                self.viewport = viewport(&self.window, self.host.scale(), self.limit);
                self.fit_surface(gpu, size);
            }
        }
    }

    /// Takes the surface's next frame, or what kept it back. A surface not configured is configured at the window's
    /// size first, since wgpu ends the process when one gives a frame; a window of no area gives none.
    pub(super) fn frame(&mut self, gpu: &Gpu) -> (Option<wgpu::SurfaceTexture>, Acquired) {
        // A lost device refuses a configuration without telling any scope, so its surfaces give nothing until mended.
        if gpu.losses.seen() {
            return (None, Acquired::Occluded);
        }
        if !self.configured.ready() {
            self.fit_surface(gpu, self.window.inner_size());
        }
        if !self.configured.ready() {
            return (None, Acquired::Occluded);
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (Some(frame), Acquired::Drawn),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (Some(frame), Acquired::Suboptimal),
            wgpu::CurrentSurfaceTexture::Timeout => (None, Acquired::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => (None, Acquired::Occluded),
            wgpu::CurrentSurfaceTexture::Outdated => (None, Acquired::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => (None, Acquired::Lost),
            wgpu::CurrentSurfaceTexture::Validation => (None, Acquired::Invalid),
        }
    }

    /// The look a frame laid out now is drawn at.
    pub(super) fn look(&self) -> Look {
        asks::look(&self.viewport)
    }

    /// Presents `frame`, drawn by the last turn or by an earlier one, and counts it; the CPU's work since `started`,
    /// presenting not counted.
    pub(super) fn present(&mut self, gpu: &Gpu, frame: wgpu::SurfaceTexture, started: Clock) -> Duration {
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let rebuilt = painter::present(
            &mut self.renderer,
            &gpu.refusals,
            &gpu.losses,
            || Renderer::new(engine(gpu), ui::fonts::settings()),
            frame.texture.format(),
            &view,
            &self.viewport,
        );
        let cpu = started.elapsed();
        self.window.pre_present_notify();
        gpu.queue.present(frame);
        self.redraws += 1;
        if rebuilt {
            report::note!(warn, "the device refused a frame; its renderer is made again");
            self.drawn = None;
            self.asking.whole();
        }
        cpu
    }
}

/// The size a pane's surface was configured at: none for a surface made and not configured yet, or one to configure
/// again, which gives no frame until a window with an area configures it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Configured(Option<winit::dpi::PhysicalSize<u32>>);

impl Configured {
    /// The size to configure at for a window drawable at `size`: none when it is configured at that size already, or
    /// the window has no area.
    fn wanted(self, size: winit::dpi::PhysicalSize<u32>) -> Option<winit::dpi::PhysicalSize<u32>> {
        (self.0 != Some(size) && size.width > 0 && size.height > 0).then_some(size)
    }

    /// A surface configured at `size` with `outcome`: one the device refused is configured at nothing.
    fn after(size: winit::dpi::PhysicalSize<u32>, outcome: Result<(), wgpu::Error>) -> Self {
        Self(outcome.ok().map(|()| size))
    }

    /// Whether a frame may be taken from the surface.
    fn ready(self) -> bool {
        self.0.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalSize;

    const SIZE: PhysicalSize<u32> = PhysicalSize::new(1280, 800);

    #[test]
    fn a_surface_made_again_is_configured_at_the_size_the_old_one_had() {
        let old = Configured(Some(SIZE));
        assert_eq!(old.wanted(SIZE), None, "the same surface at the same size is left as it is");
        let made = Configured::default();
        assert!(!made.ready(), "a new surface gives no frame");
        assert_eq!(made.wanted(SIZE), Some(SIZE), "and is configured, though the old one had that size");
    }

    #[test]
    fn a_surface_waits_for_a_window_with_an_area() {
        let made = Configured::default();
        assert_eq!(made.wanted(PhysicalSize::new(0, 0)), None);
        assert_eq!(made.wanted(PhysicalSize::new(1280, 0)), None);
        assert!(!made.ready(), "so it gives no frame while the window has none");
    }

    #[test]
    fn a_surface_whose_configuration_was_refused_gives_no_frame_and_is_configured_again() {
        let refused = wgpu::Error::Validation { source: "refused".into(), description: String::new() };
        let after = Configured::after(SIZE, Err(refused));
        assert!(!after.ready(), "a refused surface gives no frame");
        assert_eq!(after.wanted(SIZE), Some(SIZE), "and its next frame configures it again");
        assert!(Configured::after(SIZE, Ok(())).ready());
    }
}

/// Where a window stood: its frame's corner and its inside's size, in the system's pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    position: winit::dpi::PhysicalPosition<i32>,
    size: winit::dpi::PhysicalSize<u32>,
}
