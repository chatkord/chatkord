//! What keeps a window's pictures true when the GPU refuses work: the size it is drawn at, which never passes the
//! device's limit, and the renderer made again after a refused frame, since its caches say uploaded what never was.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced_wgpu::graphics::Viewport;
use iced_wgpu::{Renderer, wgpu};
use iced_winit::core::Color;
use iced_winit::winit::dpi::PhysicalSize;

use crate::lost::Losses;

/// Whether the device refused work since it was last asked.
#[derive(Clone, Debug, Default)]
pub(crate) struct Refusals(Arc<AtomicBool>);

impl Refusals {
    /// Has `device` log each error, since wgpu's own answer is a panic, and remember it here.
    pub(crate) fn watch(&self, device: &wgpu::Device) {
        let refused = self.0.clone();
        device.on_uncaptured_error(Arc::new(move |_| {
            // The error is wgpu's own text, which can name an adapter or a path: only that it came is kept.
            report::note!(error, "the graphics device reported an error");
            refused.store(true, Ordering::Release);
        }));
    }

    /// Whether work was refused since the last ask.
    fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// The first error `work` made on `device`, caught here rather than told to the device's handler: what wgpu refused
/// is known only so, and a surface whose configuration it refused ends the process when asked for a frame.
pub(crate) fn caught(device: &wgpu::Device, work: impl FnOnce()) -> Option<wgpu::Error> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    work();
    // Scopes pop innermost first; on a native device each answer is ready when popped.
    [internal.pop(), memory.pop(), validation.pop()].map(pollster::block_on).into_iter().flatten().next()
}

/// `size` as far as a texture of `limit` pixels a side holds: a surface or a viewport past it is refused, and every
/// frame drawn with the viewport on a smaller target after it.
pub(crate) fn drawable(size: PhysicalSize<u32>, limit: u32) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.min(limit), size.height.min(limit))
}

/// Draws the renderer's frame onto `target` and presents it to the queue. A frame the device refused leaves what the
/// renderer cached as uploaded (an image, a glyph) absent from the GPU and never sent again, so the renderer is
/// made again by `rebuild`; true then, and the frame is to be drawn again. A device that is lost is drawn on not at
/// all: its refusals say nothing but that it is gone, and the loop makes it again.
pub(crate) fn present(
    renderer: &mut Renderer,
    refusals: &Refusals,
    lost: &Losses,
    rebuild: impl FnOnce() -> Renderer,
    format: wgpu::TextureFormat,
    target: &wgpu::TextureView,
    viewport: &Viewport,
) -> bool {
    if lost.seen() {
        return false;
    }
    refusals.take();
    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        renderer.present(Some(Color::BLACK), format, target, viewport);
    }));
    // A reset is told inside a frame, and wgpu's staging belt then panics on buffers it could not map again: a frame
    // that could not be drawn is a lost device, which the loop makes again at most twice a minute.
    if drawn.is_err() {
        lost.unusable();
        return false;
    }
    let refused = refusals.take();
    if refused {
        *renderer = rebuild();
    }
    refused
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use iced_wgpu::Engine;
    use iced_wgpu::graphics::Shell;
    use iced_widget::shader;
    use iced_widget::{Theme, image};
    use iced_winit::core::renderer::Scale;
    use iced_winit::core::svg::{Handle, Renderer as _, Svg};
    use iced_winit::core::widget::{self, Widget};
    use iced_winit::core::{Element, Layout, Length, layout, mouse, renderer};
    use iced_winit::core::{Rectangle, Size};
    use iced_winit::runtime::user_interface::{Cache, UserInterface};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    pub(crate) const SIDE: u32 = 64;
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
    const MARK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##;

    #[test]
    fn a_size_past_the_devices_limit_is_cut_to_it() {
        let limit = 8192;
        assert_eq!(drawable(PhysicalSize::new(1578, 65479), limit), PhysicalSize::new(1578, limit));
        assert_eq!(drawable(PhysicalSize::new(9000, 600), limit), PhysicalSize::new(limit, 600));
        assert_eq!(drawable(PhysicalSize::new(800, 600), limit), PhysicalSize::new(800, 600));
    }

    /// A device on whatever adapter is here, its refusals watched.
    pub(crate) struct Gpu {
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    impl Gpu {
        pub(crate) fn new(refusals: &Refusals) -> Self {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("an adapter");
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
            refusals.watch(&device);
            Self { adapter, device, queue }
        }

        pub(crate) fn device(&self) -> &wgpu::Device {
            &self.device
        }

        pub(crate) fn renderer(&self) -> Renderer {
            let engine =
                Engine::new(&self.adapter, self.device.clone(), self.queue.clone(), FORMAT, None, Shell::headless());
            Renderer::new(engine, ui::fonts::settings())
        }

        /// A target `side` pixels square that can be drawn on and read back.
        pub(crate) fn target(&self, side: u32) -> wgpu::Texture {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        }

        /// Every pixel of `target`, which is `side` pixels square and a multiple of 64 wide: rows of blue, green, red,
        /// alpha.
        fn pixels(&self, target: &wgpu::Texture, side: u32) -> Vec<u8> {
            let row = side * 4;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * side),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
                },
                wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            );
            self.queue.submit([encoder.finish()]);
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the copy finished");
            let mapped = buffer.slice(..).get_mapped_range().expect("the copy is mapped");
            mapped.to_vec()
        }

        /// The centre pixel of `target`: blue, green, red, alpha.
        pub(crate) fn centre(&self, target: &wgpu::Texture) -> [u8; 4] {
            let row = 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * SIDE),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
                },
                wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
            );
            self.queue.submit([encoder.finish()]);
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the copy finished");
            let mapped = buffer.slice(..).get_mapped_range().expect("the copy is mapped");
            let at = (row * SIDE / 2 + SIDE / 2 * 4) as usize;
            [mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]]
        }

        /// Draws the mark over the whole of `target` through [`present`], at `viewport`; true when that was refused.
        pub(crate) fn draw(
            &self,
            renderer: &mut Renderer,
            refusals: &Refusals,
            lost: &Losses,
            target: &wgpu::Texture,
            viewport: &Viewport,
        ) -> bool {
            let whole = Rectangle::with_size(Size::new(SIDE as f32, SIDE as f32));
            renderer.draw_svg(Svg::new(Handle::from_memory(MARK)), whole, whole);
            let view = target.create_view(&wgpu::TextureViewDescriptor::default());
            present(renderer, refusals, lost, || self.renderer(), FORMAT, &view, viewport)
        }
    }

    mod lab {
        use super::*;

        #[test]
        fn what_the_device_refused_inside_a_scope_is_answered_there_and_not_to_its_handler() {
            tier::lab();
            let refusals = Refusals::default();
            let gpu = Gpu::new(&refusals);
            assert!(caught(gpu.device(), || drop(gpu.target(SIDE))).is_none(), "a texture it can make");
            assert!(caught(gpu.device(), || drop(gpu.target(0))).is_some(), "a texture of no area is refused");
            assert!(!refusals.take(), "and the handler was not told");
        }

        #[test]
        fn a_frame_the_device_could_not_draw_ends_nothing_and_has_the_device_mended() {
            tier::lab();
            let (refusals, losses) = (Refusals::default(), Losses::default());
            let gpu = Gpu::new(&refusals);
            let mut renderer = gpu.renderer();
            let target = gpu.target(SIDE);
            let fits = Viewport::with_physical_size(Size::new(SIDE, SIDE), Scale::default());
            let whole = Rectangle::with_size(Size::new(SIDE as f32, SIDE as f32));
            let draw = |renderer: &mut Renderer, colour: &str| {
                let icon = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="{colour}"/></svg>"#
                );
                renderer.draw_svg(Svg::new(Handle::from_memory(icon.into_bytes())), whole, whole);
                let view = target.create_view(&wgpu::TextureViewDescriptor::default());
                present(renderer, &refusals, &losses, || gpu.renderer(), FORMAT, &view, &fits)
            };
            // Each new icon is uploaded through the renderer's staging belt, mapped again once its frame is done.
            assert!(!draw(&mut renderer, "#ff0000"));
            assert!(!draw(&mut renderer, "#00ff00"));
            // The device goes with those maps out, and nothing yet has told the window: the next upload cannot be made.
            gpu.device().destroy();
            let _ = gpu.device().poll(wgpu::PollType::Poll);
            assert!(!losses.seen());
            assert!(!draw(&mut renderer, "#0000ff"), "no frame, and the run goes on");
            assert!(losses.seen(), "the device is mended as a lost one");
        }

        #[test]
        fn an_icon_draws_again_after_the_device_refused_the_frame_it_was_uploaded_in() {
            tier::lab();
            let refusals = Refusals::default();
            let gpu = Gpu::new(&refusals);
            let mut renderer = gpu.renderer();
            let target = gpu.target(SIDE);
            let tall = Viewport::with_physical_size(Size::new(SIDE, 65479), Scale::default());
            assert!(
                gpu.draw(&mut renderer, &refusals, &Losses::default(), &target, &tall),
                "a viewport past its target is refused"
            );
            let fits = Viewport::with_physical_size(Size::new(SIDE, SIDE), Scale::default());
            assert!(
                !gpu.draw(&mut renderer, &refusals, &Losses::default(), &target, &fits),
                "a viewport on its target is not"
            );
            assert_eq!(gpu.centre(&target), [0, 0, 255, 255], "the icon is drawn");
        }

        /// A widget that counts how often it is laid out.
        struct Counted(Arc<AtomicU32>);

        impl widget::Meta for Counted {}

        impl<M> Widget<M, Theme, Renderer> for Counted {
            fn size(&self) -> Size<Length> {
                Size::new(Length::Fixed(8.0), Length::Fixed(8.0))
            }

            fn layout(&mut self, tree: &mut widget::Tree, _: &Renderer, limits: &layout::Limits) {
                self.0.fetch_add(1, Ordering::Relaxed);
                tree.size = layout::atomic(limits, Length::Fixed(8.0), Length::Fixed(8.0));
            }

            fn draw(
                &self,
                _: &widget::Tree,
                _: &mut Renderer,
                _: &Theme,
                _: &renderer::Style,
                _: Layout,
                _: mouse::Cursor,
                _: &Rectangle,
            ) {
            }
        }

        /// A picture that counts how often it is prepared and where it was.
        #[derive(Debug)]
        struct Prepared {
            count: Arc<AtomicU32>,
            at: Arc<Mutex<Option<Rectangle>>>,
        }

        struct Nothing;

        impl shader::Pipeline for Nothing {
            fn new(_: &wgpu::Device, _: &wgpu::Queue, _: wgpu::TextureFormat) -> Self {
                Nothing
            }
        }

        impl shader::Primitive for Prepared {
            type Pipeline = Nothing;

            fn prepare(&self, _: &mut Nothing, _: &wgpu::Device, _: &wgpu::Queue, bounds: &Rectangle, _: &Viewport) {
                self.count.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut at) = self.at.lock() {
                    *at = Some(*bounds);
                }
            }
        }

        struct Picture {
            count: Arc<AtomicU32>,
            at: Arc<Mutex<Option<Rectangle>>>,
        }

        impl shader::Program<()> for Picture {
            type State = ();
            type Primitive = Prepared;

            fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> Prepared {
                Prepared { count: self.count.clone(), at: self.at.clone() }
            }
        }

        #[test]
        fn a_new_picture_is_presented_in_the_layout_the_last_frame_had() {
            tier::lab();
            const SIDE: u32 = 128;
            let refusals = Refusals::default();
            let gpu = Gpu::new(&refusals);
            let mut renderer = gpu.renderer();
            let target = gpu.target(SIDE);
            let view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let viewport = Viewport::with_physical_size(Size::new(SIDE, SIDE), Scale::default());
            let (laid_out, prepared, at) =
                (Arc::<AtomicU32>::default(), Arc::<AtomicU32>::default(), Arc::<Mutex<Option<Rectangle>>>::default());
            let pixels = vec![200_u8; 16 * 16 * 4];
            let element: Element<'_, (), Theme, Renderer> = iced_widget::column![
                iced_widget::text("Layout once"),
                iced_widget::image(image::Handle::from_rgba(16, 16, pixels)),
                Widget::_boxed(Counted(laid_out.clone())),
                shader(Picture { count: prepared.clone(), at: at.clone() }).width(Length::Fill).height(Length::Fill),
            ]
            ._boxed();
            let mut interface = UserInterface::build(element, viewport.logical_size(), Cache::new(), &mut renderer);
            let style = renderer::Style { text_color: Color::WHITE };
            interface.draw(&mut renderer, &Theme::Dark, &style, mouse::Cursor::Unavailable);
            // Its paragraphs live in the cache, as the window keeps it between frames.
            let _cache = interface.into_cache();

            let rebuilt = || panic!("the device refused nothing");
            assert!(!present(&mut renderer, &refusals, &Losses::default(), rebuilt, FORMAT, &view, &viewport));
            let first = gpu.pixels(&target, SIDE);
            assert!(!present(&mut renderer, &refusals, &Losses::default(), rebuilt, FORMAT, &view, &viewport));
            let second = gpu.pixels(&target, SIDE);

            assert_eq!(laid_out.load(Ordering::Relaxed), 1, "one layout for both frames");
            assert_eq!(prepared.load(Ordering::Relaxed), 2, "the picture is prepared in each frame");
            assert!(first.chunks(4).any(|pixel| pixel[..3] != [0, 0, 0]), "the first frame shows the text and image");
            let picture = at.lock().expect("the picture was prepared").expect("it knows where it was");
            let outside = |pixels: &[u8]| -> Vec<u8> {
                (0..SIDE * SIDE)
                    .filter(|at| {
                        let (x, y) = ((at % SIDE) as f32, (at / SIDE) as f32);
                        !picture.contains(iced_winit::core::Point::new(x, y))
                    })
                    .flat_map(|at| pixels[at as usize * 4..at as usize * 4 + 4].to_vec())
                    .collect()
            };
            assert!(outside(&first) == outside(&second), "everything outside the picture is as it was");
        }
    }
}
