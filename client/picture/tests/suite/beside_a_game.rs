//! A share's GPU work beside a game: a fixed GPU-bound scene, drawn as fast as the GPU goes and timed on the GPU,
//! alone, then with a 4K60 share converting and encoding beside it, and the share's own pace each way; on Windows, with
//! each priority the share's process can take. The scene runs in a process of its own, this test started again, so a
//! priority the share takes is the share's alone, as a game's is its own. It measures, it asserts nothing about speed:
//! what it prints is what a share's queue and priority are chosen on.

use crate::common;

#[cfg(windows)]
#[path = "../../../media/tests/suite/common/d3d11.rs"]
mod d3d11;
#[cfg(target_os = "linux")]
use crate::dmabuf;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use capture::{Layout, Pixels};
use common::{Machine, machine_where, skipped};
use media::{Codec, SCREEN_ORDER, Session, Settings, survey};

/// The scene's size: a game at 1440p.
const SCENE: (u32, u32) = (2560, 1440);
/// How long each part runs.
const RUN: Duration = Duration::from_secs(5);
/// The share: a 4K screen at 60 a second, sent at its own size unless `GAME_SHARE_SHORT_SIDE` names a smaller stream
/// (1080 sends it at 1920x1080), which the convert scales it to.
const SCREEN: (usize, usize) = (3840, 2160);
const SHARE_FPS: u32 = 60;
/// A block of video memory a scene holds: one 8192 x 8192 RGBA texture, 256 MiB.
const BLOCK: u32 = 8192;
const BLOCK_BYTES: u64 = (BLOCK as u64) * (BLOCK as u64) * 4;
/// The large textures a bandwidth-bound scene reads each pixel from, 64 MiB each.
const READ: u32 = 4096;
/// This test, which the scene's process runs.
const TEST: &str = "beside_a_game::lab::timing::a_game_s_frames_and_a_share_s_pictures_are_timed_alone_and_together";
/// Set in the scene's process: the case it draws, by its place in [`CASES`].
const DRAWS: &str = "CHATKORD_SCENE_CASE";

/// How much of the GPU's video memory a scene holds while it draws, as a game holds its textures and buffers.
#[derive(Clone, Copy, Debug)]
enum Fill {
    /// This many GiB of its own.
    Own(u64),
    /// Blocks until the driver places no more of one in video memory, or refuses one.
    Most,
    /// That, and this many GiB more, which the driver must keep in system memory and move in and out.
    Over(u64),
}

/// One fixed scene: its shading, its reads and the memory it holds, standing in for a kind of game.
#[derive(Clone, Copy, Debug)]
struct Case {
    name: &'static str,
    /// Lights summed at every pixel: the shading's cost.
    lights: u32,
    /// Large textures read at every pixel (at most four): the memory traffic.
    reads: u32,
    fill: Fill,
}

/// The worst a game asks of the GPU, and the high frame rate a light one keeps, which a short stall costs most.
const CASES: [Case; 4] = [
    Case { name: "an esports game: light shading, 3 GiB held", lights: 96, reads: 1, fill: Fill::Own(3) },
    Case { name: "a AAA game: heaviest shading, 10 GiB held", lights: 1024, reads: 2, fill: Fill::Own(10) },
    Case { name: "the card as full as the driver lets it be", lights: 1024, reads: 2, fill: Fill::Most },
    Case { name: "2 GiB more than the card holds", lights: 1024, reads: 2, fill: Fill::Over(2) },
];

/// Every pixel lit by `lights` lights and reading `reads` large textures: a game's fragment work, fixed.
const SHADER: &str = r"
override lights: u32;
override reads: u32;
@group(0) @binding(0) var big0: texture_2d<f32>;
@group(0) @binding(1) var big1: texture_2d<f32>;
@group(0) @binding(2) var big2: texture_2d<f32>;
@group(0) @binding(3) var big3: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i & 2u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    var colour = vec3<f32>(0.0);
    let p = at.xy / vec2<f32>(2560.0, 1440.0);
    for (var light = 0u; light < lights; light++) {
        let f = f32(light);
        let centre = vec2<f32>(fract(sin(f * 12.9898) * 43758.5453), fract(sin(f * 78.233) * 12543.1234));
        let d = distance(p, centre) + 0.01;
        colour += vec3<f32>(sin(f + p.x * 9.0), cos(f + p.y * 7.0), sin(f * 0.5)) * (0.002 / (d * d));
    }
    // Spread over the whole texture, so the reads miss the caches as a game's streaming does.
    let texel = vec2<i32>((vec2<u32>(at.xy) * vec2<u32>(1601u, 2843u)) % vec2<u32>(4096u));
    if reads > 0u { colour += textureLoad(big0, texel, 0).rgb; }
    if reads > 1u { colour += textureLoad(big1, texel, 0).rgb; }
    if reads > 2u { colour += textureLoad(big2, texel, 0).rgb; }
    if reads > 3u { colour += textureLoad(big3, texel, 0).rgb; }
    return vec4<f32>(colour / (colour + 1.0), 1.0);
}
";

/// A tiny draw that reads one texel of a held block, so every block is in each frame's work and must stay resident.
const TOUCH: &str = r"
@group(0) @binding(0) var block: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32(i & 1u) * 0.001, f32(i >> 1u) * 0.001, 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return textureLoad(block, vec2<i32>(0, 0), 0);
}
";

/// Where the scene's video memory is read: on Linux the card's (amdgpu's counters), on Windows the scene process's
/// own against the budget Windows gives it (`QueryVideoMemoryInfo`).
struct Memory {
    #[cfg(target_os = "linux")]
    node: String,
    #[cfg(windows)]
    adapter: windows::Win32::Graphics::Dxgi::IDXGIAdapter3,
}

impl Memory {
    #[cfg(target_os = "linux")]
    fn of(info: &wgpu::AdapterInfo) -> Option<Self> {
        Some(Self { node: picture::render_node(info)? })
    }

    #[cfg(windows)]
    fn of(info: &wgpu::AdapterInfo) -> Option<Self> {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        use windows::core::Interface;
        // SAFETY: no arguments; the factory is an owned COM reference.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        (0..)
            // SAFETY: the factory is live; an index past the last adapter is an error, which ends the search.
            .map_while(|index| unsafe { factory.EnumAdapters1(index) }.ok())
            .find(|adapter| {
                // SAFETY: the adapter is live; its description is copied out.
                unsafe { adapter.GetDesc1() }
                    .is_ok_and(|desc| desc.VendorId == info.vendor && desc.DeviceId == info.device)
            })
            .and_then(|adapter| adapter.cast().ok())
            .map(|adapter| Self { adapter })
    }

    /// The video memory the scene may hold and what is held now, in bytes: on Linux the card's whole and its use.
    #[cfg(target_os = "linux")]
    fn read(&self) -> Option<(u64, u64)> {
        let name = std::path::Path::new(&self.node).file_name()?.to_str()?;
        let read = |file: &str| {
            std::fs::read_to_string(format!("/sys/class/drm/{name}/device/{file}")).ok()?.trim().parse::<u64>().ok()
        };
        Some((read("mem_info_vram_total")?, read("mem_info_vram_used")?))
    }

    /// The same on Windows: the budget it gives this process in the card's own memory, and the process's use of it.
    #[cfg(windows)]
    fn read(&self) -> Option<(u64, u64)> {
        use windows::Win32::Graphics::Dxgi::{DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO};
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        // SAFETY: node 0 of a live adapter; `info` is the structure the call fills.
        unsafe { self.adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &raw mut info) }.ok()?;
        Some((info.Budget, info.CurrentUsage))
    }
}

fn texture(device: &wgpu::Device, side: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// Blocks of video memory held as `fill` asks, each cleared once so the driver places it.
fn hold(machine: &Machine, memory: &Memory, fill: Fill) -> Vec<wgpu::Texture> {
    let Machine { device, queue } = machine;
    let mut blocks = Vec::new();
    let Some((total, _)) = memory.read() else { return blocks };
    let mut elsewhere = 0;
    let (own, wanted) = match fill {
        Fill::Own(gib) => (gib * (1 << 30) / BLOCK_BYTES, total),
        Fill::Most | Fill::Over(_) => (u64::MAX, total),
    };
    let over = match fill {
        Fill::Over(gib) => gib * (1 << 30) / BLOCK_BYTES,
        _ => 0,
    };
    while let Some((_, before)) = memory.read() {
        if (over == 0 && before + BLOCK_BYTES > wanted) || blocks.len() as u64 >= own {
            break;
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let block = texture(device, BLOCK);
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &block.create_view(&Default::default()),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the clear finishes");
        if pollster::block_on(scope.pop()).is_some() {
            break;
        }
        blocks.push(block);
        let Some((_, after)) = memory.read() else { break };
        // Blocks the driver puts in system memory, three in a row: video memory is as full as it will be.
        elsewhere = if after < before + BLOCK_BYTES / 2 { elsewhere + 1 } else { 0 };
        if elsewhere == 3 + over {
            break;
        }
    }
    blocks
}

/// The scene of `case`'s GPU time per frame, each frame drawn once the last is done, until `until`.
fn draw_scene(machine: &Machine, case: Case, blocks: &[wgpu::Texture], until: Instant) -> Vec<f64> {
    let Machine { device, queue } = machine;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let pipeline = |source: &str, constants: &[(&str, f64)]| {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let options = wgpu::PipelineCompilationOptions { constants, ..Default::default() };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: options.clone(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: options,
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let scene = pipeline(SHADER, &[("lights", f64::from(case.lights)), ("reads", f64::from(case.reads))]);
    let touch = pipeline(TOUCH, &[]);
    let reads: Vec<wgpu::TextureView> =
        (0..4).map(|_| texture(device, READ).create_view(&Default::default())).collect();
    let read_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &scene.get_bind_group_layout(0),
        entries: &(0..4)
            .map(|n| wgpu::BindGroupEntry {
                binding: n,
                resource: wgpu::BindingResource::TextureView(&reads[n as usize]),
            })
            .collect::<Vec<_>>(),
    });
    let block_views: Vec<wgpu::TextureView> = blocks.iter().map(|b| b.create_view(&Default::default())).collect();
    let touches: Vec<wgpu::BindGroup> = block_views
        .iter()
        .map(|view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &touch.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
            })
        })
        .collect();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene"),
        size: wgpu::Extent3d { width: SCENE.0, height: SCENE.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let queries =
        device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 2 });
    let resolved = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let period = f64::from(queue.get_timestamp_period());
    let mut frames = Vec::new();
    while Instant::now() < until {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&scene);
            pass.set_bind_group(0, &read_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&touch);
            for group in &touches {
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
        encoder.copy_buffer_to_buffer(&resolved, 0, &readback, 0, 16);
        queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |mapped| mapped.expect("the timestamps map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the frame finishes");
        {
            let bytes = readback.slice(..).get_mapped_range().expect("mapped");
            let begin = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            let end = u64::from_le_bytes(bytes[8..].try_into().unwrap());
            frames.push(end.saturating_sub(begin) as f64 * period / 1e6);
        }
        readback.unmap();
    }
    frames
}

/// The adapter a game draws on, writing timestamps: Windows' games on DX12, Linux's on the first (Vulkan).
fn game_machine() -> Option<(Machine, wgpu::AdapterInfo)> {
    let suits = |info: &wgpu::AdapterInfo| cfg!(not(windows)) || info.backend == wgpu::Backend::Dx12;
    let (machine, info) = machine_where(wgpu::Features::TIMESTAMP_QUERY, suits)?;
    machine.device.features().contains(wgpu::Features::TIMESTAMP_QUERY).then_some((machine, info))
}

/// The scene's own process: holds the case's memory and says so, waits for a line on its input, then draws for
/// [`RUN`] and says each frame's GPU time.
fn scene_process(case: Case) {
    let Some((machine, info)) = game_machine() else { return skipped("the adapter writes no timestamps") };
    let Some(memory) = Memory::of(&info) else { return skipped("the adapter's video memory is unread") };
    let blocks = hold(&machine, &memory, case.fill);
    let held = memory.read().map_or_else(
        || "video memory unread".to_string(),
        |(total, used)| {
            let gib = |bytes: u64| bytes as f64 / 2.0f64.powi(30);
            format!("{:.1} of {:.1} GiB of video memory held", gib(used), gib(total))
        },
    );
    eprintln!("scene held {} blocks, {held}", blocks.len());
    eprintln!("scene ready");
    std::io::stdin().read_line(&mut String::new()).expect("told to draw");
    for frame in draw_scene(&machine, case, &blocks, Instant::now() + RUN) {
        eprintln!("scene frame {frame}");
    }
    eprintln!("scene done");
}

/// What the share's process is given beside the game, each raised and lowered from as is without administrator rights.
#[derive(Clone, Copy, Debug)]
enum Knob {
    AsIs,
    /// The encoder's D3D11 device's GPU thread priority (`IDXGIDevice::SetGPUThreadPriority`, -7 to 7).
    #[cfg(windows)]
    Thread(i32),
    /// The process's GPU scheduling class (`D3DKMTSetProcessSchedulingPriorityClass`).
    #[cfg(windows)]
    Scheduling(windows::Wdk::Graphics::Direct3D::D3DKMT_SCHEDULINGPRIORITYCLASS),
    /// The process's priority class (`SetPriorityClass`).
    #[cfg(windows)]
    Class(windows::Win32::System::Threading::PROCESS_CREATION_FLAGS),
}

#[cfg(target_os = "linux")]
const KNOBS: [Knob; 1] = [Knob::AsIs];

#[cfg(windows)]
const KNOBS: [Knob; 9] = {
    use windows::Wdk::Graphics::Direct3D::{
        D3DKMT_SCHEDULINGPRIORITYCLASS_ABOVE_NORMAL, D3DKMT_SCHEDULINGPRIORITYCLASS_BELOW_NORMAL,
        D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH,
    };
    use windows::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    };
    [
        Knob::AsIs,
        Knob::Thread(-7),
        Knob::Thread(7),
        Knob::Scheduling(D3DKMT_SCHEDULINGPRIORITYCLASS_BELOW_NORMAL),
        Knob::Scheduling(D3DKMT_SCHEDULINGPRIORITYCLASS_ABOVE_NORMAL),
        Knob::Scheduling(D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH),
        Knob::Class(BELOW_NORMAL_PRIORITY_CLASS),
        Knob::Class(ABOVE_NORMAL_PRIORITY_CLASS),
        Knob::Class(HIGH_PRIORITY_CLASS),
    ]
};

impl Knob {
    /// Gives this process the knob's setting, or says why it was refused; a device's is given in [`Knob::take_on`].
    #[cfg_attr(target_os = "linux", expect(clippy::unnecessary_wraps, reason = "Windows may refuse one"))]
    fn take(self) -> Result<(), String> {
        match self {
            Knob::AsIs => Ok(()),
            #[cfg(windows)]
            Knob::Thread(_) => Ok(()),
            #[cfg(windows)]
            Knob::Scheduling(class) => {
                use windows::Wdk::Graphics::Direct3D::D3DKMTSetProcessSchedulingPriorityClass;
                // SAFETY: takes nothing; the pseudo-handle needs no closing.
                let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
                // SAFETY: this process's pseudo-handle and a class the call defines.
                let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(process, class) };
                status.ok().map_err(|refused| refused.to_string())
            }
            #[cfg(windows)]
            Knob::Class(class) => {
                // SAFETY: takes nothing; the pseudo-handle needs no closing.
                let process = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
                // SAFETY: this process's pseudo-handle and a class the call defines.
                unsafe { windows::Win32::System::Threading::SetPriorityClass(process, class) }
                    .map_err(|refused| refused.to_string())
            }
        }
    }

    /// Gives this process back what it had before [`Knob::take`].
    fn undo(self) {
        #[cfg(windows)]
        {
            use windows::Wdk::Graphics::Direct3D::D3DKMT_SCHEDULINGPRIORITYCLASS_NORMAL;
            use windows::Win32::System::Threading::NORMAL_PRIORITY_CLASS;
            match self {
                Knob::Scheduling(_) => Knob::Scheduling(D3DKMT_SCHEDULINGPRIORITYCLASS_NORMAL).take().expect("normal"),
                Knob::Class(_) => Knob::Class(NORMAL_PRIORITY_CLASS).take().expect("normal"),
                Knob::AsIs | Knob::Thread(_) => {}
            }
        }
    }

    /// Gives the share's encoder device the knob's setting, or says why it was refused.
    #[cfg_attr(target_os = "linux", expect(clippy::unnecessary_wraps, reason = "Windows' devices may refuse one"))]
    fn take_on(self, session: &Session) -> Result<(), String> {
        match self {
            #[cfg(windows)]
            Knob::Thread(priority) => {
                use windows::Win32::Graphics::Dxgi::IDXGIDevice;
                use windows::core::Interface;
                let device = session.gpu().map(|gpu| gpu.device().clone()).ok_or("no D3D11 device")?;
                let device: IDXGIDevice = device.cast().map_err(|refused| refused.to_string())?;
                // SAFETY: a live DXGI device and a priority in the call's range.
                unsafe { device.SetGPUThreadPriority(priority) }.map_err(|refused| refused.to_string())
            }
            _ => {
                let _ = session;
                Ok(())
            }
        }
    }
}

/// The screens a share's encoder reads: on Linux DMA-BUFs on the window's render node, as PipeWire's are.
#[cfg(target_os = "linux")]
fn screens(_session: &Session) -> Option<Vec<dmabuf::Surface>> {
    let (_, info) = machine_where(wgpu::Features::empty(), |_| true)?;
    let node = picture::render_node(&info)?;
    (0..3)
        .map(|n| {
            let mut screen = dmabuf::Surface::new(&node, SCREEN.0, SCREEN.1)?;
            screen.paint(&(0..SCREEN.0 * SCREEN.1).map(|i| ((i / 7 + n * 40) % 200 + 16) as u8).collect::<Vec<_>>());
            Some(screen)
        })
        .collect()
}

/// The same on Windows: textures on the encoder's own D3D11 device, as Windows Graphics Capture's are.
#[cfg(windows)]
fn screens(session: &Session) -> Option<Vec<d3d11::Surface>> {
    let gpu = session.gpu()?;
    (0..3)
        .map(|n| {
            let mut screen = d3d11::Surface::new(&gpu, SCREEN.0, SCREEN.1)?;
            screen.paint(&(0..SCREEN.0 * SCREEN.1).map(|i| ((i / 7 + n * 40) % 200 + 16) as u8).collect::<Vec<_>>());
            Some(screen)
        })
        .collect()
}

/// The channel order a share's screen comes in: PipeWire's BGRx, Windows Graphics Capture's BGRA.
const PIXELS: Pixels = if cfg!(windows) { Pixels::Bgra } else { Pixels::Bgrx };

/// A 4K screen shared at 60 a second with `knob` until `stop`, at the stream's size the test was given, and each picture's time
/// from one sent to the next, in milliseconds.
fn share(knob: Knob, stop: &AtomicBool) -> Option<(String, Vec<f64>)> {
    let report = survey();
    let encoder = SCREEN_ORDER.into_iter().find_map(|backend| {
        report.encoders.iter().find(|e| e.codec() == Codec::H264 && e.device().is_some_and(|d| d.backend == backend))
    })?;
    let short_side = std::env::var("GAME_SHARE_SHORT_SIDE").ok().and_then(|side| side.parse::<usize>().ok());
    let (width, height) = short_side.map_or(SCREEN, |side| (SCREEN.0 * side / SCREEN.1, side));
    let settings =
        Settings { width, height, frames_per_second: SHARE_FPS, bits_per_second: 20_000_000, key_frame_every: None };
    let layout = Layout { pixels: PIXELS, width: SCREEN.0 as u32, height: SCREEN.1 as u32 };
    let mut session = Session::open_screen(encoder, settings, layout).ok()?;
    let screens = screens(&session)?;
    if let Err(refused) = knob.take_on(&session) {
        eprintln!("{knob:?}: refused by the device ({refused})");
    }
    let tick = Duration::from_secs(1) / SHARE_FPS;
    let (mut next, mut last, mut gaps) = (Instant::now(), None::<Instant>, Vec::new());
    let mut n = 0;
    while !stop.load(Ordering::Relaxed) {
        session.encode_frame(screens[n % 3].frame(PIXELS)).expect("a share's picture");
        let sent = Instant::now();
        if let Some(last) = last {
            gaps.push(sent.duration_since(last).as_secs_f64() * 1000.0);
        }
        last = Some(sent);
        n += 1;
        next += tick;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    let convert = session.convert().map_or_else(|| "memory".to_string(), |convert| format!("{convert:?}"));
    Some((format!("{encoder} through {convert}"), gaps))
}

/// A share with `knob` for as long as `run` takes, once its encoder runs: what `run` gave, and the share's encoder and
/// gaps; none when no encoder opens.
fn sharing<T>(knob: Knob, run: impl FnOnce() -> T) -> (T, Option<(String, Vec<f64>)>) {
    let stop = Arc::new(AtomicBool::new(false));
    let beside = {
        let stop = stop.clone();
        std::thread::spawn(move || share(knob, &stop))
    };
    std::thread::sleep(Duration::from_secs(1));
    let ran = run();
    stop.store(true, Ordering::Relaxed);
    (ran, beside.join().expect("the share"))
}

/// The scene of `CASES[index]` in a process of its own, drawn when `around` calls the draw it is given: what the scene
/// held and its frames' GPU times; none when it skipped.
fn scene(index: usize, around: impl FnOnce(&mut dyn FnMut() -> Vec<f64>) -> Vec<f64>) -> Option<(String, Vec<f64>)> {
    let mut child = Command::new(std::env::current_exe().expect("this test's binary"))
        .args([TEST, "--exact", "--nocapture"])
        .env(DRAWS, index.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the scene's process");
    let mut lines = BufReader::new(child.stderr.take().expect("its output")).lines().map_while(Result::ok);
    let held = lines.by_ref().find_map(|line| line.strip_prefix("scene held ").map(str::to_string));
    let ready = lines.by_ref().any(|line| line == "scene ready");
    let (Some(held), true) = (held, ready) else {
        child.wait().expect("the scene's process ends");
        return None;
    };
    let mut input = child.stdin.take().expect("its input");
    let mut draw = || -> Vec<f64> {
        writeln!(input, "draw").expect("the scene told to draw");
        lines
            .by_ref()
            .take_while(|line| line != "scene done")
            .filter_map(|line| line.strip_prefix("scene frame ")?.parse().ok())
            .collect()
    };
    let frames = around(&mut draw);
    assert!(child.wait().expect("the scene's process ends").success(), "the scene's process failed");
    Some((held, frames))
}

fn spread(label: &str, mut values: Vec<f64>) {
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values.get(((values.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(f64::NAN);
    eprintln!(
        "{label}: {} samples, median {:.2} ms, p99 {:.2} ms, worst {:.2} ms",
        values.len(),
        at(0.5),
        at(0.99),
        at(1.0)
    );
}

mod lab {
    use super::*;

    mod timing {
        use super::*;

        #[test]
        fn a_game_s_frames_and_a_share_s_pictures_are_timed_alone_and_together() {
            if let Some(index) = std::env::var(DRAWS).ok().and_then(|index| index.parse::<usize>().ok()) {
                return scene_process(CASES[index]);
            }
            if game_machine().is_none() {
                return skipped("the adapter writes no timestamps");
            }
            let ((), alone) = sharing(Knob::AsIs, || std::thread::sleep(RUN));
            let Some((encoder, gaps)) = alone else {
                return skipped("no hardware H.264 encoder takes a 4K screen here");
            };
            spread(&format!("{encoder} alone, time between pictures"), gaps);
            for (index, case) in CASES.into_iter().enumerate() {
                let Some((held, frames)) = scene(index, |draw| draw()) else {
                    return skipped("the scene's process skipped: see its output");
                };
                eprintln!("{}: {held}", case.name);
                spread(&format!("{}, alone: GPU time a frame", case.name), frames);
                for knob in KNOBS {
                    if let Err(refused) = knob.take() {
                        eprintln!("{}, {knob:?}: refused ({refused})", case.name);
                        continue;
                    }
                    let drawn = scene(index, |draw| {
                        let (frames, gaps) = sharing(knob, draw);
                        match gaps {
                            Some((_, gaps)) => spread(&format!("{}, {knob:?}, the share", case.name), gaps),
                            None => eprintln!("{}, {knob:?}: the share did not open beside it", case.name),
                        }
                        frames
                    });
                    knob.undo();
                    let (_, frames) = drawn.expect("the scene drew as it did alone");
                    spread(&format!("{}, {knob:?}, the game beside it: GPU time a frame", case.name), frames);
                }
            }
        }
    }
}
