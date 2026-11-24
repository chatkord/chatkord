//! A D3D11 texture the test paints on the encoder's own device, standing in for Windows Graphics Capture's: BGRA,
//! bound and shared by handle as a capture's textures are. Painting it again makes a new texture, so a lent frame keeps what it showed.

#![allow(dead_code, reason = "each test binary that includes this fixture uses a different part of it")]

use media::{D3d11, Frame, Gpu, Layout, Native, Pixels};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_SHARED,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    ID3D11Device, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

pub(crate) struct Surface {
    device: ID3D11Device,
    texture: Option<ID3D11Texture2D>,
    width: usize,
    height: usize,
}

impl Surface {
    /// A `width`x`height` BGRA surface on `gpu`'s device, unpainted until [`Surface::paint_rgb`].
    pub(crate) fn new(gpu: &Gpu, width: usize, height: usize) -> Option<Self> {
        Some(Self { device: gpu.device().clone(), texture: None, width, height })
    }

    /// Paints the surface with `gray`, one byte for each pixel.
    pub(crate) fn paint(&mut self, gray: &[u8]) {
        let rgb: Vec<[u8; 3]> = gray.iter().map(|g| [*g; 3]).collect();
        self.paint_rgb(&rgb);
    }

    /// Paints the surface with `rgb`, one triple for each pixel, as a new texture made with it as its contents.
    pub(crate) fn paint_rgb(&mut self, rgb: &[[u8; 3]]) {
        assert_eq!(rgb.len(), self.width * self.height);
        let pixels: Vec<u8> = rgb.iter().flat_map(|[r, g, b]| [*b, *g, *r, 255]).collect();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.width as u32,
            Height: self.height as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32,
        };
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: (self.width * 4) as u32,
            SysMemSlicePitch: 0,
        };
        let mut texture = None;
        // SAFETY: `initial` points at `height` rows of `width * 4` bytes, alive until the call returns, which copies
        // them; `texture` is written on success.
        unsafe { self.device.CreateTexture2D(&raw const desc, Some(&raw const initial), Some(&raw mut texture)) }
            .expect("a texture on the encoder's device");
        self.texture = Some(texture.expect("the device made the texture it said it made"));
    }

    /// The surface as a frame the way the capture lends one: the same texture, on the device it was made on.
    pub(crate) fn frame(&self, pixels: Pixels) -> Frame {
        let texture = self.texture.clone().expect("the surface was painted before it was lent");
        Frame::new(
            Layout { pixels, width: self.width as u32, height: self.height as u32 },
            Native::D3d11(D3d11::new(texture)),
            std::time::Instant::now(),
        )
    }
}
