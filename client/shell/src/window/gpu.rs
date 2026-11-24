//! The GPU every window paints with, its start on the driver and the surface and report facts that come with it.

use iced_wgpu::graphics::{Shell, Viewport};
use iced_wgpu::{Engine, wgpu};
use iced_winit::core::{Size, renderer};
use iced_winit::winit;
#[cfg(target_os = "linux")]
use iced_winit::winit::platform::wayland::ActiveEventLoopExtWayland;
use picture::{EndsAlone, IMPORT_FEATURES};

use super::Wake;
use super::unhosted::Unhosted;
use crate::lost::Losses;
use crate::painter::{self, Refusals};

/// The viewport of `window` as far as `limit` lets it be drawn: the target it is drawn on holds no more.
pub(super) fn viewport(window: &winit::window::Window, scale: renderer::Scale, limit: u32) -> Viewport {
    let size = painter::drawable(window.inner_size(), limit);
    Viewport::with_physical_size(Size::new(size.width, size.height), scale)
}

/// The graphics API as a fact: a word the source wrote.
fn backend(backend: wgpu::Backend) -> report::Fact {
    use wgpu::Backend;
    report::Fact::Word(match backend {
        Backend::Noop => report::word!("noop"),
        Backend::Vulkan => report::word!("Vulkan"),
        Backend::Metal => report::word!("Metal"),
        Backend::Dx12 => report::word!("Direct3D 12"),
        Backend::Gl => report::word!("OpenGL"),
        Backend::BrowserWebGpu => report::word!("WebGPU"),
    })
}

/// The adapter's name, driver and backend as the debug report's "Graphics" section: a name passes only as a product
/// name, so a path a driver puts in one is withheld.
pub fn graphics(info: &wgpu::AdapterInfo, decodes_here: bool) -> report::Section {
    use wgpu::DeviceType;
    let device = report::Fact::Word(match info.device_type {
        DeviceType::Other => report::word!("other"),
        DeviceType::IntegratedGpu => report::word!("integrated"),
        DeviceType::DiscreteGpu => report::word!("discrete"),
        DeviceType::VirtualGpu => report::word!("virtual"),
        DeviceType::Cpu => report::word!("processor"),
    });
    let rows = (|| {
        let mut rows = report::Section::new("Graphics");
        rows.push("adapter", &report::Product::fact_of(&info.name))?;
        rows.push("driver", &report::Product::fact_of(&info.driver))?;
        rows.push("driver info", &report::Product::fact_of(&info.driver_info))?;
        rows.push("backend", &backend(info.backend))?;
        rows.push("kind", &device)?;
        rows.push("decodes on this adapter", &decodes_here)?;
        Ok::<_, report::Full>(rows)
    })();
    rows.unwrap_or_else(|_| report::Section::absent("Graphics"))
}

/// How frames are presented, as a fact.
fn present(mode: wgpu::PresentMode) -> report::Fact {
    use wgpu::PresentMode;
    report::Fact::Word(match mode {
        PresentMode::AutoVsync => report::word!("auto vsync"),
        PresentMode::AutoNoVsync => report::word!("auto no vsync"),
        PresentMode::Fifo => report::word!("fifo"),
        PresentMode::FifoRelaxed => report::word!("fifo relaxed"),
        PresentMode::Immediate => report::word!("immediate"),
        PresentMode::Mailbox => report::word!("mailbox"),
    })
}

/// How a frame reaches the screen. On Wayland a mailbox and the compositor's frame callbacks pace the redraws, since a
/// loop waiting on the compositor lags a resize behind the pointer; elsewhere the display's refresh paces them.
fn present_mode(wayland: bool, offered: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if wayland && offered.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::AutoVsync
    }
}

#[cfg(target_os = "linux")]
pub(super) fn on_wayland(event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    event_loop.is_wayland()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn on_wayland(_: &winit::event_loop::ActiveEventLoop) -> bool {
    false
}

/// Configures `surface` on `device` at `size`, or says why the device refused it.
pub(super) fn configure(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    present_mode: wgpu::PresentMode,
    size: winit::dpi::PhysicalSize<u32>,
) -> Result<(), wgpu::Error> {
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width,
        height: size.height,
        present_mode,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        // A mailbox holds one frame waiting; a queue that waits for the display may hold two.
        desired_maximum_frame_latency: if present_mode == wgpu::PresentMode::Mailbox { 1 } else { 2 },
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    painter::caught(device, || surface.configure(device, &config)).map_or(Ok(()), Err)
}

/// The adapter to paint with and, where that GPU also decodes, the render node to open the decoder on. A GPU that
/// cannot decode still gets its window, painted from memory; only a machine that cannot show one has none.
fn adapter(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
) -> Result<(wgpu::Adapter, Option<String>), Unhosted> {
    let asleep = picture::Asleep::now();
    let adapters = picture::adapters(instance);
    picture::painting_that_presents(&adapters, &asleep, surface).ok_or(Unhosted::NoGpu)
}

/// The GPU every window of the run paints with: one device, which the decoders decode onto too. Its handles end under
/// the loader's lock, so its owner keeps it past the windows and their engines, which hold the device too.
pub(super) struct Gpu {
    /// What a window's surface is made from, and made again from when it is lost.
    pub(super) instance: EndsAlone<wgpu::Instance>,
    /// What each window's engine is made on.
    adapter: EndsAlone<wgpu::Adapter>,
    pub(super) queue: EndsAlone<wgpu::Queue>,
    pub(super) device: EndsAlone<wgpu::Device>,
    pub(super) format: wgpu::TextureFormat,
    pub(super) present_mode: wgpu::PresentMode,
    /// What the device refused, which a window answers by making its renderer again.
    pub(super) refusals: Refusals,
    /// Whether the device was lost, which the run answers by making it again.
    pub(crate) losses: Losses,
    /// Whether the windows are on Wayland, which a device made again is presented for.
    wayland: bool,
    /// Wakes the loop when the device is lost.
    lost: Wake,
}

/// A GPU as the start or a mend made it: with the render node it also decodes on and the adapter as a report states it.
pub(super) struct Made {
    pub(super) gpu: Gpu,
    pub(super) node: Option<String>,
    pub(super) graphics: report::Section,
}

impl Gpu {
    /// The most a side of a texture of this device may be.
    pub(super) fn drawable_limit(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// `size` as far as the device's textures hold.
    pub(super) fn drawable(&self, size: winit::dpi::PhysicalSize<u32>) -> winit::dpi::PhysicalSize<u32> {
        painter::drawable(size, self.drawable_limit())
    }
}

/// The engine a window of `gpu` is drawn by: its shaders and pipelines, the longest part of a start after the device.
pub(super) fn engine(gpu: &Gpu) -> Engine {
    report::note!(info, "an engine was made");
    Engine::new(&gpu.adapter, gpu.device.clone(), gpu.queue.clone(), gpu.format, None, Shell::headless())
}

/// What the start's thread makes for the main window: the GPU, and the engine that draws the window.
pub(super) struct Begun {
    pub(super) gpu: Gpu,
    pub(super) engine: Engine,
    pub(super) surface: wgpu::Surface<'static>,
    /// Where that GPU also decodes, its render node.
    pub(super) node: Option<String>,
    /// The adapter, as a debug report states it.
    pub(super) graphics: report::Section,
}

/// The GPU, its main window's engine and the render node, made from `instance` for `surface`: every part of a start
/// that waits on the driver, which under load took seconds on Machine A.
pub(super) fn start(
    instance: EndsAlone<wgpu::Instance>,
    surface: &wgpu::Surface<'_>,
    wayland: bool,
    lost: Wake,
) -> Result<(Gpu, Engine, Option<String>, report::Section), Unhosted> {
    let Made { gpu, node, graphics } = Gpu::made(instance, surface, wayland, lost)?;
    // The typeface every screen of ours is drawn in, and the emoji: bundled, so a screen looks the same on every system.
    ui::fonts::install();
    let engine = engine(&gpu);
    Ok((gpu, engine, node, graphics))
}

impl Gpu {
    /// The adapter that paints `surface` with a device on it, made from `instance`; `lost` is woken when that device
    /// is lost.
    pub(super) fn made(
        instance: EndsAlone<wgpu::Instance>,
        surface: &wgpu::Surface<'_>,
        wayland: bool,
        lost: Wake,
    ) -> Result<Made, Unhosted> {
        let (adapter, node) = adapter(&instance, surface)?;
        let info = adapter.get_info();
        let graphics = graphics(&info, node.is_some());
        report::note!(
            info,
            "adapter",
            gpu = report::Product::fact_of(&info.name),
            backend = backend(info.backend),
            decodes_here = node.is_some()
        );
        let (device, queue) = picture::device(
            &adapter,
            &wgpu::DeviceDescriptor {
                label: Some("shell"),
                required_features: adapter.features() & IMPORT_FEATURES,
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            },
        )
        .map_err(|_| Unhosted::NoDevice)?;
        // wgpu's default answer to a validation error is a panic, which would end the window over one bad picture.
        let refusals = Refusals::default();
        refusals.watch(&device);
        let losses = Losses::default();
        losses.watch(&device, lost.clone());
        report::note!(
            info,
            "import features on the device we made",
            imports = device.features().contains(IMPORT_FEATURES)
        );
        // The painter wants a format whose values are not re-encoded on the way out.
        let offered = surface.get_capabilities(&adapter);
        let format = offered.formats.into_iter().find(|f| !f.is_srgb()).ok_or(Unhosted::NoFormat)?;
        let present_mode = present_mode(wayland, &offered.present_modes);
        report::note!(info, "frames presented", mode = present(present_mode));
        let gpu = Gpu {
            instance,
            adapter: EndsAlone::new(adapter),
            queue,
            device,
            format,
            present_mode,
            refusals,
            losses,
            wayland,
            lost,
        };
        Ok(Made { gpu, node, graphics })
    }

    /// The same, made again on the same instance for `surface`: the device was lost.
    pub(super) fn again(&self, surface: &wgpu::Surface<'_>) -> Result<Made, Unhosted> {
        Self::made(EndsAlone::new(self.instance.clone()), surface, self.wayland, self.lost.clone())
    }
}
