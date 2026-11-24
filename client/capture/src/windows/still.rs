//! A still of a monitor or window for our picker: one picture drawn smaller on the GPU and read back as RGBA. Only
//! the small picture leaves the GPU; a share never reads its pictures this way.

use std::time::Duration;

use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use super::{Capture, Gpu, Output, Processor};
use crate::card::fit;
use crate::{CaptureError, Native};

/// How long a still waits for the source's picture: a window that draws nothing in that time is shown without one.
const WAIT: Duration = Duration::from_millis(500);

/// A small picture of a source, as RGBA rows top down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Still {
    /// Even, and never larger than the source's picture.
    pub width: u32,
    /// Even, and never larger than the source's picture.
    pub height: u32,
    /// `width * height` pixels, four bytes each, alpha opaque.
    pub rgba: Vec<u8>,
}

/// A still of `item` drawn on `gpu`, as large as fits inside `within` with its own shape kept. One device serves
/// every still of a picker.
pub fn still(item: &GraphicsCaptureItem, gpu: &Gpu, within: (u32, u32)) -> Result<Still, CaptureError> {
    let capture = Capture::open(item, gpu, false, None)?;
    let frame = capture.next(WAIT)?.ok_or(CaptureError::Ended)?;
    let layout = frame.layout();
    let size = fit((layout.width, layout.height), within).ok_or(CaptureError::Format)?;
    let processor =
        Processor::new(gpu, (layout.width, layout.height), (layout.width, layout.height), size, Output::Bgra)
            .ok_or(CaptureError::Format)?;
    let drawn = texture(gpu, size, false)?;
    let read = texture(gpu, size, true)?;
    let Native::D3d11(captured) = frame.native();
    if !processor.blit(gpu, captured.texture(), &drawn) {
        return Err(CaptureError::Stream);
    }
    drop(frame);
    let _locked = gpu.lock();
    // SAFETY: both textures are this device's, of one size and format; the context is held under the device's lock.
    unsafe { gpu.context().CopyResource(&read, &drawn) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: a staging texture the CPU may read; the copy above is waited for by the map.
    unsafe { gpu.context().Map(&read, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped)) }
        .map_err(|_| CaptureError::Stream)?;
    let row = size.0 as usize * 4;
    let mut rgba = Vec::with_capacity(row * size.1 as usize);
    for y in 0..size.1 as usize {
        // SAFETY: the mapped texture has `size.1` rows of `RowPitch` bytes.
        let start = unsafe { mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize) };
        // SAFETY: each row holds at least `row` bytes.
        let line = unsafe { std::slice::from_raw_parts(start, row) };
        rgba.extend(line.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, _]| [r, g, b, 255]));
    }
    // SAFETY: the texture mapped above, unmapped once.
    unsafe { gpu.context().Unmap(&read, 0) };
    Ok(Still { width: size.0, height: size.1, rgba })
}

/// A BGRA texture of `size`: one the processor draws into, or one the CPU reads.
fn texture(gpu: &Gpu, (width, height): (u32, u32), staging: bool) -> Result<ID3D11Texture2D, CaptureError> {
    let described = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
        BindFlags: if staging { 0 } else { D3D11_BIND_RENDER_TARGET.0 as u32 },
        CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
        ..Default::default()
    };
    let mut made = None;
    // SAFETY: the description outlives the call.
    unsafe { gpu.device().CreateTexture2D(&raw const described, None, Some(&raw mut made)) }
        .map_err(|_| CaptureError::Connect)?;
    made.ok_or(CaptureError::Connect)
}
