//! The GPU the window paints with, as a decoder needs to know it, so a picture is decoded where it can be painted: on
//! Linux the render node whose surfaces the window's device imports, on Windows the window's own D3D12 device.

/// The window's GPU, as a decoder takes it. The default is none: the decoder's own choice, then software.
#[derive(Clone, Debug, Default)]
pub struct Painting {
    #[cfg(target_os = "linux")]
    render_node: Option<String>,
    #[cfg(windows)]
    device: Option<media::D3d12Device>,
}

impl Painting {
    /// The window's: `node`, chosen with the adapter that made `device`.
    #[cfg_attr(
        windows,
        expect(clippy::needless_pass_by_value, reason = "Linux keeps `node`; Windows paints on `device`")
    )]
    pub fn of(device: &wgpu::Device, node: Option<String>) -> Self {
        #[cfg(target_os = "linux")]
        let _ = device;
        #[cfg(windows)]
        let _ = node;
        Self {
            #[cfg(target_os = "linux")]
            render_node: node,
            #[cfg(windows)]
            device: crate::d3d12::decoding_device(device),
        }
    }

    /// Where no window is (a command line, a test): the render node the window would choose, which lists every
    /// adapter. There is no window's device to hand over on Windows.
    pub fn found() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            render_node: crate::dmabuf::painting_render_node(),
            #[cfg(windows)]
            device: None,
        }
    }

    /// The render node a decoder opens on, where the window's device imports its surfaces.
    #[cfg(target_os = "linux")]
    pub fn render_node(&self) -> Option<&str> {
        self.render_node.as_deref()
    }

    /// The window's own D3D12 device, where it paints through DirectX 12.
    #[cfg(windows)]
    pub fn device(&self) -> Option<&media::D3d12Device> {
        self.device.as_ref()
    }

    /// Where a decoder looks, nearest first: this GPU, then the system's own device (Vulkan on Linux, D3D11 on
    /// Windows), then software.
    pub fn sources(&self) -> media::Sources {
        let mut sources =
            media::Sources { vulkan: cfg!(target_os = "linux"), d3d11: cfg!(windows), ..Default::default() };
        #[cfg(target_os = "linux")]
        sources.render_nodes.extend(self.render_node.iter().cloned());
        #[cfg(windows)]
        {
            sources.d3d12.clone_from(&self.device);
            sources.adapter = self.device.as_ref().map(media::D3d12Device::adapter);
        }
        sources
    }
}
