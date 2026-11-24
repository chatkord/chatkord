//! Painting a picture on the GPU: from its planes in memory (three one-channel textures, or NV12's luma and its
//! two-channel chroma) or a plane view of an imported NV12 one, by the matrix and range the picture carries, or from
//! an RGB texture as it is. No RGBA buffer is ever made on the CPU.

use crate::conversion::{self, Conversion};
use media::{Colours, PictureLayout, Samples};

const SHADER: &str = include_str!("yuv.wgsl");

/// The pipeline for one target format; made once and shared by every picture size.
pub struct Painter {
    /// Three planes in memory.
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// NV12's two planes in memory.
    two_planes: Imports,
    framing: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    nv12: Option<Imports>,
    rgb: Imports,
    /// How each pair of colours becomes RGB, made once and bound by the picture's own, at [`Colours::index`].
    conversions: [wgpu::Buffer; Colours::ALL.len()],
    /// The format it paints into.
    #[cfg(target_os = "linux")]
    format: wgpu::TextureFormat,
    /// What a picture another API wrote needs of the device, where it can take one.
    #[cfg(target_os = "linux")]
    importer: Option<crate::dmabuf::Importer>,
}

/// The pipeline for one kind of picture beside the three planes in memory: two planes in memory, or an import. An
/// imported NV12 one only where the device was made with the features that texture needs.
struct Imports {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// A driver-owned picture ready to be drawn: the imported texture and the bind group over its views. Dropping it
/// frees the GPU's side of the import.
pub struct Imported {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    kind: Kind,
}

/// What an imported texture holds, which picks the pipeline that paints it.
#[derive(Clone, Copy)]
enum Kind {
    Nv12,
    Rgb,
}

/// The part of a picture a draw shows, each side a share of the picture's (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    /// From the picture's left edge.
    pub left: f32,
    /// From the picture's top edge.
    pub top: f32,
    /// Its width.
    pub width: f32,
    /// Its height.
    pub height: f32,
}

impl Crop {
    /// All of the picture.
    pub const WHOLE: Self = Self { left: 0.0, top: 0.0, width: 1.0, height: 1.0 };

    fn bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        for (chunk, value) in
            bytes.as_chunks_mut::<4>().0.iter_mut().zip([self.left, self.top, self.width, self.height])
        {
            *chunk = value.to_le_bytes();
        }
        bytes
    }
}

/// The rounded rectangle a picture is shown within, in the target's physical pixels: what lies outside it is not
/// painted, its corners' edges anti-aliased, so a picture takes its card's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// The left edge.
    pub left: f32,
    /// The top edge.
    pub top: f32,
    /// Its width.
    pub width: f32,
    /// Its height.
    pub height: f32,
    /// Its corners' radius, clamped to half the shorter side.
    pub radius: f32,
}

impl Shape {
    /// No shape: the picture fills its viewport square, as it was drawn before shapes.
    pub const NONE: Self = Self { left: -1.0e6, top: -1.0e6, width: 2.0e6, height: 2.0e6, radius: 0.0 };

    fn bytes(self) -> [u8; 32] {
        let mut bytes = [0; 32];
        let values = [self.left, self.top, self.width, self.height, self.radius, 0.0, 0.0, 0.0];
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            *chunk = value.to_le_bytes();
        }
        bytes
    }
}

/// Where in the framing's buffer the shape is written, after the crop.
const SHAPE_AT: u64 = 16;
/// The framing's buffer: the crop, then the shape.
const FRAMING_BYTES: u64 = 48;

/// The part of the picture one place on screen shows and the shape it is shown within, held by the GPU for its draws.
/// Each place keeps its own, since every place is prepared before the pass draws any of them.
pub struct Framing {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    crop: Crop,
    shape: Shape,
}

impl Framing {
    /// Show `crop` from the next draw on; the GPU is written only when it changed.
    pub fn set(&mut self, queue: &wgpu::Queue, crop: Crop) {
        if crop != self.crop {
            queue.write_buffer(&self.buffer, 0, &crop.bytes());
            self.crop = crop;
        }
    }

    /// Show it within `shape` from the next draw on; the GPU is written only when it changed.
    pub fn shape(&mut self, queue: &wgpu::Queue, shape: Shape) {
        if shape != self.shape {
            queue.write_buffer(&self.buffer, SHAPE_AT, &shape.bytes());
            self.shape = shape;
        }
    }
}

/// The textures of one picture size and layout. A different size or layout makes a new one.
pub struct Gpu {
    size: (u32, u32),
    planes: Textures,
    /// What the shader paints them by, written again only when the picture's colours change.
    conversion: wgpu::Buffer,
    colours: Colours,
    bind_group: wgpu::BindGroup,
}

/// A picture's planes on the GPU, as its layout lays them.
enum Textures {
    /// Luma, and blue and red interleaved in two channels.
    Nv12([wgpu::Texture; 2]),
    /// Luma, blue and red, one channel each.
    I420([wgpu::Texture; 3]),
}

/// A buffer holding how samples in `colours` become RGB, for a bind group's `conversion`.
fn conversion_buffer(device: &wgpu::Device, colours: Colours) -> wgpu::Buffer {
    wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("picture conversion"),
            contents: &Conversion::of(colours).bytes(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        },
    )
}

/// How a YUV picture's samples become RGB: its matrix and range, as the shader's `conversion`.
fn conversion_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 8,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(conversion::BYTES),
        },
        count: None,
    }
}

fn plane_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The part of a texture its picture fills: a ring of one size holds pictures of many sizes, and a decoder's pool made
/// for a larger picture holds a smaller one.
fn shown_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 7,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(SHOWN_BYTES),
        },
        count: None,
    }
}

/// The shader's `Shown`: the picture's share of the texture, the last place a sample of its own may take, and the same
/// for a plane at half its size, NV12's chroma.
const SHOWN_BYTES: u64 = 32;

/// The buffer saying `texture` holds its picture, `shown` wide and high, at its top left; none on a lost device.
fn shown_buffer(device: &wgpu::Device, texture: &wgpu::Texture, shown: (u32, u32)) -> Option<wgpu::Buffer> {
    let (width, height) = (texture.width() as f32, texture.height() as f32);
    let (across, down) = (shown.0.min(texture.width()), shown.1.min(texture.height()));
    let (half_across, half_down) = (across.div_ceil(2) as f32, down.div_ceil(2) as f32);
    let (across, down) = (across as f32, down as f32);
    // Half a texel inside the picture's last, so a linear sample never takes the texture's next one, which the picture
    // did not write.
    let region = [
        across / width,
        down / height,
        (across - 0.5) / width,
        (down - 0.5) / height,
        (half_across - 0.5) / (width / 2.0),
        (half_down - 0.5) / (height / 2.0),
        0.0,
        0.0,
    ];
    let contents: Vec<u8> = region.iter().flat_map(|value| value.to_le_bytes()).collect();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("picture shown"),
        size: SHOWN_BYTES,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    // A device lost meanwhile makes the buffer invalid, which wgpu's own helper for this panics on.
    buffer.get_mapped_range_mut(..).ok()?.copy_from_slice(&contents);
    buffer.unmap();
    Some(buffer)
}

fn sampler_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 3,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn pipeline_for(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    [layout, framing]: [&wgpu::BindGroupLayout; 2],
    fragment: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("picture"),
        bind_group_layouts: &[Some(layout), Some(framing)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("picture"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            // Premultiplied: a fragment of a shape's corner covers what lies under it by its share.
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl Painter {
    /// `format` must not be an sRGB one: the picture's values are already display-encoded.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("picture"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture planes"),
            entries: &[plane_entry(0), plane_entry(1), plane_entry(2), sampler_entry(), conversion_entry()],
        });
        let framing = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture framing"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAMING_BYTES),
                },
                count: None,
            }],
        });
        let pipeline = pipeline_for(device, &module, [&layout, &framing], "fragment", format);
        let imports = |layout: wgpu::BindGroupLayout, fragment: &str| Imports {
            pipeline: pipeline_for(device, &module, [&layout, &framing], fragment, format),
            layout,
        };
        let two_planes = imports(
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("picture two planes"),
                entries: &[plane_entry(0), plane_entry(9), sampler_entry(), conversion_entry()],
            }),
            "fragment_two_planes",
        );
        let nv12 = device.features().contains(wgpu::Features::TEXTURE_FORMAT_NV12).then(|| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("picture nv12 planes"),
                entries: &[plane_entry(4), plane_entry(5), sampler_entry(), shown_entry(), conversion_entry()],
            });
            imports(layout, "fragment_nv12")
        });
        let rgb = {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("picture rgb"),
                entries: &[plane_entry(6), sampler_entry(), shown_entry()],
            });
            imports(layout, "fragment_rgb")
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            two_planes,
            framing,
            sampler,
            nv12,
            rgb,
            conversions: Colours::ALL.map(|colours| conversion_buffer(device, colours)),
            #[cfg(target_os = "linux")]
            format,
            #[cfg(target_os = "linux")]
            importer: crate::dmabuf::Importer::new(device),
        }
    }

    /// A texture of the window's own of `size` that a picture is drawn into and shown from, as one's own card is.
    #[cfg(target_os = "linux")]
    pub(crate) fn target(&self, device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture drawn again"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// Draws `picture` whole into the top left `shown` pixels of `into`, a texture [`Painter::target`] made, and
    /// submits it, so the picture's own texture may go once this returns.
    #[cfg(target_os = "linux")]
    pub(crate) fn draw_into(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        picture: &Imported,
        framing: &Framing,
        into: &wgpu::Texture,
        shown: (u32, u32),
    ) {
        let view = into.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("picture drawn again"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, shown.0 as f32, shown.1 as f32, 0.0, 1.0);
            self.draw_imported(&mut pass, picture, framing);
        }
        queue.submit([encoder.finish()]);
    }

    /// A framing for one place on screen, showing the whole picture until it is set.
    pub fn framing(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Framing {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("picture framing"),
            size: FRAMING_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &Crop::WHOLE.bytes());
        queue.write_buffer(&buffer, SHAPE_AT, &Shape::NONE.bytes());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picture framing"),
            layout: &self.framing,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        Framing { buffer, bind_group, crop: Crop::WHOLE, shape: Shape::NONE }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn importer(&self) -> Option<&crate::dmabuf::Importer> {
        self.importer.as_ref()
    }

    /// Bind an imported NV12 texture, its samples in `colours`, for drawing, by the painter's buffer for those colours,
    /// its picture the `shown` width and height at its top left, no larger than the texture: a decoder may write a
    /// smaller picture into a pool made for a larger one. `None` where this device cannot draw one.
    pub fn bind(
        &self,
        device: &wgpu::Device,
        texture: wgpu::Texture,
        colours: Colours,
        shown: (u32, u32),
    ) -> Option<Imported> {
        let nv12 = self.nv12.as_ref()?;
        let plane = |aspect| texture.create_view(&wgpu::TextureViewDescriptor { aspect, ..Default::default() });
        let (luma, chroma) = (plane(wgpu::TextureAspect::Plane0), plane(wgpu::TextureAspect::Plane1));
        let conversion = &self.conversions[colours.index()];
        let region = shown_buffer(device, &texture, shown)?;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picture nv12 planes"),
            layout: &nv12.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&luma) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&chroma) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: region.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: conversion.as_entire_binding() },
            ],
        });
        Some(Imported { _texture: texture, bind_group, kind: Kind::Nv12 })
    }

    /// Bind an imported RGB texture (a captured screen, or one's own share's picture) for drawing, its picture the
    /// `shown` width and height at its top left, no larger than the texture; none once the device is lost.
    pub fn bind_rgb(&self, device: &wgpu::Device, texture: wgpu::Texture, shown: (u32, u32)) -> Option<Imported> {
        let view = texture.create_view(&Default::default());
        let region = shown_buffer(device, &texture, shown)?;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picture rgb"),
            layout: &self.rgb.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 7, resource: region.as_entire_binding() },
            ],
        });
        Some(Imported { _texture: texture, bind_group, kind: Kind::Rgb })
    }

    /// Paint the part of an imported picture `framing` shows over the pass's whole viewport. An NV12 one is drawn only
    /// by a painter with that pipeline, which `bind` has already said by making the [`Imported`].
    pub fn draw_imported(&self, pass: &mut wgpu::RenderPass<'_>, picture: &Imported, framing: &Framing) {
        let imports = match picture.kind {
            Kind::Nv12 => self.nv12.as_ref(),
            Kind::Rgb => Some(&self.rgb),
        };
        if let Some(imports) = imports {
            pass.set_pipeline(&imports.pipeline);
            pass.set_bind_group(0, &picture.bind_group, &[]);
            pass.set_bind_group(1, &framing.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Paint the part of the picture `framing` shows over the pass's whole viewport, by the pipeline of its layout.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, gpu: &Gpu, framing: &Framing) {
        pass.set_pipeline(match gpu.layout() {
            PictureLayout::I420 => &self.pipeline,
            PictureLayout::Nv12 => &self.two_planes.pipeline,
        });
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.set_bind_group(1, &framing.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn plane_texture(
    device: &wgpu::Device,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// A picture with no area, or a side larger than the device's largest texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge;

/// Whether `device` can hold a texture of this size; wgpu would otherwise fail the whole device.
pub(crate) fn holds(device: &wgpu::Device, (width, height): (u32, u32)) -> bool {
    let max = device.limits().max_texture_dimension_2d;
    (1..=max).contains(&width) && (1..=max).contains(&height)
}

impl Gpu {
    /// Textures for three-plane pictures of `(width, height)` drawn by `painter`, or [`TooLarge`] where the device
    /// cannot hold one.
    pub fn new(device: &wgpu::Device, painter: &Painter, size: (u32, u32)) -> Result<Self, TooLarge> {
        Self::of_layout(device, painter, size, PictureLayout::I420)
    }

    /// Textures for pictures of `(width, height)` laid out as `layout`, drawn by `painter`, or [`TooLarge`] where the
    /// device cannot hold one.
    pub fn of_layout(
        device: &wgpu::Device,
        painter: &Painter,
        (width, height): (u32, u32),
        layout: PictureLayout,
    ) -> Result<Self, TooLarge> {
        if !holds(device, (width, height)) {
            return Err(TooLarge);
        }
        let half = (width.div_ceil(2), height.div_ceil(2));
        let one = wgpu::TextureFormat::R8Unorm;
        let luma = plane_texture(device, "luma", (width, height), one);
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let conversion = conversion_buffer(device, Colours::HD);
        let sampler = wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&painter.sampler) };
        let converted = wgpu::BindGroupEntry { binding: 8, resource: conversion.as_entire_binding() };
        let (planes, bind_group) = match layout {
            PictureLayout::I420 => {
                let planes = [luma, plane_texture(device, "blue", half, one), plane_texture(device, "red", half, one)];
                let views = planes.each_ref().map(view);
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("picture planes"),
                    layout: &painter.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[0]) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[1]) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[2]) },
                        sampler,
                        converted,
                    ],
                });
                (Textures::I420(planes), bind_group)
            }
            PictureLayout::Nv12 => {
                let planes = [luma, plane_texture(device, "chroma", half, wgpu::TextureFormat::Rg8Unorm)];
                let [luma, chroma] = planes.each_ref().map(view);
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("picture two planes"),
                    layout: &painter.two_planes.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&luma) },
                        wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&chroma) },
                        sampler,
                        converted,
                    ],
                });
                (Textures::Nv12(planes), bind_group)
            }
        };
        Ok(Self { size: (width, height), planes, conversion, colours: Colours::HD, bind_group })
    }

    /// The picture size these textures hold.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// How the pictures these textures hold are laid out.
    pub fn layout(&self) -> PictureLayout {
        match self.planes {
            Textures::Nv12(_) => PictureLayout::Nv12,
            Textures::I420(_) => PictureLayout::I420,
        }
    }

    /// Copy a three-plane picture of this size up from its luma, blue and red planes, as [`Gpu::upload_samples`].
    pub fn upload(&mut self, queue: &wgpu::Queue, planes: [(&[u8], u32); 3], colours: Colours) {
        self.upload_samples(queue, Samples::I420(planes), colours);
    }

    /// Copy a picture of this size and layout up from its planes, each with its bytes from one row to the next, as they
    /// lie in memory, to be painted as samples in `colours`: the GPU's buffer of how is written only when they differ
    /// from the last picture's. The caller makes a new `Gpu` when the size or the layout changes; a picture of another
    /// layout is not copied.
    pub fn upload_samples(&mut self, queue: &wgpu::Queue, samples: Samples<'_>, colours: Colours) {
        if colours != self.colours {
            queue.write_buffer(&self.conversion, 0, &Conversion::of(colours).bytes());
            self.colours = colours;
        }
        let write = |texture: &wgpu::Texture, (bytes, stride): (&[u8], u32)| {
            queue.write_texture(
                texture.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: None },
                texture.size(),
            );
        };
        match (&self.planes, samples) {
            (Textures::I420(textures), Samples::I420(planes)) => {
                textures.iter().zip(planes).for_each(|(t, p)| write(t, p))
            }
            (Textures::Nv12(textures), Samples::Nv12(planes)) => {
                textures.iter().zip(planes).for_each(|(t, p)| write(t, p))
            }
            (Textures::I420(_), Samples::Nv12(_)) | (Textures::Nv12(_), Samples::I420(_)) => {}
        }
    }
}
