//! A picture on the window's own D3D12 device, a decoder's or one's own share's, adopted as a texture with no copy. The
//! paint waits on its writer's fence on the GPU, and the picture goes back to its writer only after the window's reads.

use crate::yuv::{Imported, Painter};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use wgpu::hal::api::Dx12;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Resource};
use windows::core::Interface;

/// Why a picture could not be adopted: this device is not D3D12, or the writer's picture or fence went missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused;

/// The device a decoder should decode onto so its pictures are on `device`, where `device` is D3D12.
pub(crate) fn decoding_device(device: &wgpu::Device) -> Option<media::D3d12Device> {
    // SAFETY: the hal device is only read, for a reference to its D3D12 device.
    let hal = unsafe { device.as_hal::<Dx12>() }?;
    Some(media::D3d12Device(hal.raw_device().clone()))
}

/// A picture on the window's device, as its writer lends it.
#[derive(Debug)]
pub enum OnWindow {
    /// The decoder's, NV12.
    Decoded(media::D3d12Surface),
    /// One's own share's, BGRA at the card's size.
    Preview(media::PreviewPicture),
}

impl OnWindow {
    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Decoded(surface) => (surface.width(), surface.height()),
            Self::Preview(picture) => (picture.width(), picture.height()),
        }
    }

    /// The matrix and range its samples are in: a decoder's picture says; one's own share's is RGB and has none.
    pub fn colours(&self) -> Option<media::Colours> {
        match self {
            Self::Decoded(surface) => Some(surface.colours()),
            Self::Preview(_) => None,
        }
    }

    fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Decoded(_) => wgpu::TextureFormat::NV12,
            Self::Preview(_) => wgpu::TextureFormat::Bgra8Unorm,
        }
    }

    fn resource(&self) -> Option<ID3D12Resource> {
        match self {
            Self::Decoded(surface) => surface.resource(),
            Self::Preview(picture) => Some(picture.resource()),
        }
    }

    fn wait_written(&self, queue: &ID3D12CommandQueue) -> bool {
        match self {
            Self::Decoded(surface) => surface.wait_written(queue),
            Self::Preview(picture) => picture.wait_written(queue),
        }
    }

    fn release_after(self, queue: &ID3D12CommandQueue) {
        match self {
            Self::Decoded(surface) => surface.release_after(queue),
            Self::Preview(picture) => picture.release_after(queue),
        }
    }
}

/// A picture adopted by the window: given back to its writer, after the window's reads, when dropped.
pub struct Held {
    picture: Option<OnWindow>,
    queue: ID3D12CommandQueue,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(picture) = self.picture.take() {
            picture.release_after(&self.queue);
        }
    }
}

/// The window's queue, which a picture is waited for on and given back after.
fn queue_of(device: &wgpu::Device) -> Result<ID3D12CommandQueue, Refused> {
    // SAFETY: the hal device is only read, for its queue.
    Ok(unsafe { device.as_hal::<Dx12>() }.ok_or(Refused)?.raw_queue().clone())
}

/// `picture` as a texture of `device`, with no copy, and the hold that keeps it from its writer until dropped.
pub fn import(device: &wgpu::Device, picture: OnWindow) -> Result<(wgpu::Texture, Held), Refused> {
    let queue = queue_of(device)?;
    let resource = picture.resource().ok_or(Refused)?;
    if !picture.wait_written(&queue) {
        return Err(Refused);
    }
    let texture = adopt(device, resource, picture.format())?;
    Ok((texture, Held { picture: Some(picture), queue }))
}

/// A picture the window imported once and draws again whenever the decoder lends the same resource for a picture of
/// the same size, kept while the decoder lends from the pool it came from: the import is the texture and its bind
/// group, and has nothing to do with the lend, which each picture takes anew.
#[derive(Default)]
pub struct Imports {
    entries: HashMap<usize, Kept>,
    made: usize,
}

struct Kept {
    picture: Imported,
    colours: media::Colours,
    /// The picture's own size, which a pool made for a larger one holds at its resource's top left.
    size: (u32, u32),
    lender: Weak<media::Lender>,
}

impl Imports {
    /// How many imports were made since the window began, a repeat of a resource not counting.
    pub fn made(&self) -> usize {
        self.made
    }

    /// How many are kept now.
    pub fn kept(&self) -> usize {
        self.entries.len()
    }

    /// Drops every import whose pool no decoder lends from any more.
    pub fn trim(&mut self) {
        self.entries.retain(|_, kept| kept.lender.strong_count() > 0);
    }

    pub(crate) fn get(&self, key: usize) -> Option<&Imported> {
        self.entries.get(&key).map(|kept| &kept.picture)
    }

    /// Takes `surface` as the picture to draw: the import of its resource, made now or kept, under the key that finds
    /// it again, and the hold that keeps the surface from its decoder until dropped.
    pub(crate) fn lend(
        &mut self,
        device: &wgpu::Device,
        painter: &Painter,
        surface: media::D3d12Surface,
    ) -> Result<(usize, Held), Refused> {
        let queue = queue_of(device)?;
        let resource = surface.resource().ok_or(Refused)?;
        let key = resource.as_raw() as usize;
        if !surface.wait_written(&queue) {
            return Err(Refused);
        }
        let (lender, colours) = (Arc::downgrade(surface.lender()), surface.colours());
        let picture = OnWindow::Decoded(surface);
        let size = picture.size();
        let kept = self
            .entries
            .get(&key)
            .is_some_and(|kept| kept.lender.ptr_eq(&lender) && kept.colours == colours && kept.size == size);
        if !kept {
            let texture = adopt(device, resource, picture.format())?;
            let bound = painter.bind(device, texture, colours, size).ok_or(Refused)?;
            self.entries.insert(key, Kept { picture: bound, colours, size, lender });
            self.made += 1;
        }
        Ok((key, Held { picture: Some(picture), queue }))
    }
}

/// `resource`, a picture of `format`, as a texture of `device` at the resource's own size: a writer's picture may be
/// smaller, at its top left, which the draw takes alone.
fn adopt(
    device: &wgpu::Device,
    resource: ID3D12Resource,
    format: wgpu::TextureFormat,
) -> Result<wgpu::Texture, Refused> {
    // SAFETY: the description is only read, of a resource the caller holds.
    let described = unsafe { resource.GetDesc() };
    let width = u32::try_from(described.Width).map_err(|_| Refused)?;
    let size = wgpu::Extent3d { width, height: described.Height, depth_or_array_layers: 1 };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("picture on the window's device"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    // SAFETY: the resource is a picture of `descriptor`'s format on this device, one mip, one sample, and `size` is
    // its own description's.
    let raw = unsafe {
        wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            descriptor.format,
            descriptor.dimension,
            size,
            descriptor.mip_level_count,
            descriptor.sample_count,
        )
    };
    // SAFETY: the caller's hold keeps it from its writer until the reads are done; its writer leaves it in the common
    // state, which a read promotes.
    Ok(unsafe { device.create_texture_from_hal::<Dx12>(raw, &descriptor, wgpu::TextureUses::RESOURCE) })
}
