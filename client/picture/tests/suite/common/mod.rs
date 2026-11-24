//! What the picture's GPU tests share: a device, a skip that says so, and a render read back as pixels.

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(crate) use tier::skipped;

/// Asserts at its end that neither device's debug layer reported an error, unless the test already failed.
#[cfg(windows)]
pub(crate) struct Checked {
    pub gpu: Option<media::Gpu>,
    pub window: media::D3d12Device,
}

#[cfg(windows)]
impl std::ops::Deref for Checked {
    type Target = media::Gpu;
    fn deref(&self) -> &media::Gpu {
        self.gpu.as_ref().expect("a test checks the encoder's device it made")
    }
}

#[cfg(windows)]
impl Drop for Checked {
    fn drop(&mut self) {
        if std::thread::panicking() || media::Validation::of_this_run() == media::Validation::Off {
            return;
        }
        let encoder = self.gpu.as_ref().map(media::Gpu::reported).unwrap_or_default();
        assert_eq!(capture::validation::unknown(encoder), Vec::<String>::new(), "the encoder's D3D11 debug layer");
        assert_eq!(
            capture::validation::unknown(self.window.reported()),
            Vec::<String>::new(),
            "the window's D3D12 debug layer"
        );
    }
}

pub(crate) struct Machine {
    pub device: picture::EndsAlone<wgpu::Device>,
    pub queue: picture::EndsAlone<wgpu::Queue>,
}

/// A device with whichever of `wanted` the adapter has; the caller reads `device.features()` for what it got.
pub(crate) fn machine(wanted: wgpu::Features) -> Option<Machine> {
    machine_where(wanted, |_| true).map(|(machine, _)| machine)
}

/// The same on the first adapter `suits` accepts (the best one first), and that adapter's description.
pub(crate) fn machine_where(
    wanted: wgpu::Features,
    suits: impl Fn(&wgpu::AdapterInfo) -> bool,
) -> Option<(Machine, wgpu::AdapterInfo)> {
    tier::lab();
    // The window's instance, so a test loads what the window does and nothing it does not.
    let instance = picture::instance(None);
    let adapters = picture::adapters(&instance).to_vec();
    if adapters.is_empty() {
        skipped("no graphics adapter opens on this machine");
        return None;
    }
    let Some(adapter) = adapters.into_iter().find(|adapter| suits(&adapter.get_info())) else {
        skipped("no graphics adapter on this machine suits this test");
        return None;
    };
    // The window's own way to make a device, so a test's device can take what the window's takes.
    match picture::device(
        &adapter,
        &wgpu::DeviceDescriptor { required_features: adapter.features() & wanted, ..Default::default() },
    ) {
        Ok((device, queue)) => Some((Machine { device, queue }, adapter.get_info())),
        Err(refused) => {
            skipped(format!("the graphics adapter has no device to give ({refused})"));
            None
        }
    }
}

/// Run `draw` in a pass over a target of `(width, height)` and return its pixels as rows of RGBA.
pub(crate) fn render(
    machine: &Machine,
    (width, height): (u32, u32),
    draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
) -> Vec<Vec<[u8; 4]>> {
    let Machine { device, queue } = machine;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("picture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |mapped| mapped.expect("the readback buffer maps"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("the device finishes");
    let bytes = readback.slice(..).get_mapped_range().expect("the readback buffer is mapped");
    (0..height as usize)
        .map(|y| {
            (0..width as usize)
                .map(|x| {
                    let at = y * row as usize + x * 4;
                    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
                })
                .collect()
        })
        .collect()
}
