//! One's own share as the window shows it: drawn at the card's size into a ring the window's D3D12 device opened once,
//! fenced each way, so nothing is allocated per picture and the encoder never waits on the window.

use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device5,
    ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIResource1};
use windows::core::{Interface, PCWSTR};

use super::{Gpu, Output, Processor};
use crate::card::{Card, card};
use crate::{Frame, Native};

/// The window shows one picture and may hold the next; the third is the one being drawn.
const RING: usize = 3;

/// The ring of one's own share's pictures for the window, on the encoder's device, opened once on the window's.
pub struct Preview {
    gpu: Gpu,
    context: ID3D11DeviceContext4,
    window: ID3D12Device,
    ring: Option<Ring>,
    /// The size a ring was last refused at: tried again only at another.
    refused: Option<(u32, u32)>,
}

// SAFETY: every D3D11 object here belongs to a multithread-protected device, and the context is used only under the
// device's lock; the D3D12 objects are free-threaded.
unsafe impl Send for Preview {}

struct Ring {
    /// The size of every texture.
    size: (u32, u32),
    /// The processor for the last source and picture size, made again when either changes.
    drawing: Option<Drawing>,
    textures: Vec<Texture>,
    written: Fence,
    /// The last value "written" was signalled with.
    signalled: u64,
}

/// What the ring draws now: a source of one size into a picture of another, at the textures' top left.
struct Drawing {
    source: (u32, u32),
    picture: (u32, u32),
    processor: Processor,
}

/// A texture of the ring, both sides of it, and the value its "done" fence must reach before it is written again.
struct Texture {
    d3d11: ID3D11Texture2D,
    d3d12: ID3D12Resource,
    done: Fence,
    due: u64,
}

/// A fence shared by the two devices.
struct Fence {
    d3d11: ID3D11Fence,
    d3d12: ID3D12Fence,
}

/// One's own picture lent to the window: a BGRA resource the window's device already opened, the picture at its top
/// left, the fence and value that say it is written, and the fence and value the window signals when it lets it go.
/// Dropped without [`PreviewPicture::release_after`], it is given back at once: the window read none of it.
pub struct PreviewPicture {
    resource: ID3D12Resource,
    texture: (u32, u32),
    size: (u32, u32),
    written: (ID3D12Fence, u64),
    done: Option<(ID3D12Fence, u64)>,
}

// SAFETY: D3D12 resources and fences are free-threaded; the picture only reads them, and `release_after` takes it by
// value.
unsafe impl Send for PreviewPicture {}
// SAFETY: as above.
unsafe impl Sync for PreviewPicture {}

impl std::fmt::Debug for PreviewPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewPicture").field("size", &self.size).finish_non_exhaustive()
    }
}

impl PreviewPicture {
    /// Even, and never larger than the captured picture.
    pub fn width(&self) -> u32 {
        self.size.0
    }

    /// Even, and never larger than the captured picture.
    pub fn height(&self) -> u32 {
        self.size.1
    }

    /// The resource's own width and height, at least the picture's.
    pub fn texture_size(&self) -> (u32, u32) {
        self.texture
    }

    /// The resource, on the window's device.
    pub fn resource(&self) -> ID3D12Resource {
        self.resource.clone()
    }

    /// Makes `queue` wait, on the GPU, until the encoder's device has written the picture.
    pub fn wait_written(&self, queue: &ID3D12CommandQueue) -> bool {
        // SAFETY: a queue may wait on a fence its device opened.
        unsafe { queue.Wait(&self.written.0, self.written.1) }.is_ok()
    }

    /// Gives the texture back to the ring once `queue` has done what was submitted to it so far, its reads included.
    pub fn release_after(mut self, queue: &ID3D12CommandQueue) {
        if let Some((fence, value)) = self.done.take() {
            // SAFETY: as above. A signal that fails leaves the texture held, so the ring lends from the others.
            let _ = unsafe { queue.Signal(&fence, value) };
        }
    }
}

impl Drop for PreviewPicture {
    fn drop(&mut self) {
        if let Some((fence, value)) = self.done.take() {
            // SAFETY: a fence the window's device opened, signalled from the processor; nothing the window queued
            // reads the texture, since only `release_after` follows the window's reads.
            let _ = unsafe { fence.Signal(value) };
        }
    }
}

impl Preview {
    /// A preview of the share drawn on `gpu`, the session's device, for the window on `window`; none where the
    /// encoder's device cannot share with the window's.
    pub fn new(gpu: &Gpu, window: &ID3D12Device) -> Option<Self> {
        let context = gpu.context().cast().ok()?;
        Some(Self { gpu: gpu.clone(), context, window: window.clone(), ring: None, refused: None })
    }

    /// `frame` drawn into the top left of a free texture of the ring at the size [`card`] gives it for `within`,
    /// so a shared window that changes size, or a card dragged, keeps the ring; none when every texture is still the
    /// window's, or the device refuses.
    pub fn lend(&mut self, frame: &Frame, within: (u32, u32)) -> Option<PreviewPicture> {
        let layout = frame.layout();
        let source = (layout.width, layout.height);
        let Card { texture: holds, picture: size } = card(source, within)?;
        if self.ring.as_ref().is_none_or(|ring| ring.size != holds) && self.refused != Some(holds) {
            self.ring = Ring::new(&self.gpu, &self.window, holds);
            self.refused = self.ring.is_none().then_some(holds);
        }
        let ring = self.ring.as_mut()?;
        if ring.drawing.as_ref().is_none_or(|drawing| (drawing.source, drawing.picture) != (source, size)) {
            let processor = Processor::into_region(&self.gpu, source, source, holds, size, Output::Bgra);
            ring.drawing = processor.map(|processor| Drawing { source, picture: size, processor });
        }
        let processor = &ring.drawing.as_ref()?.processor;
        // SAFETY: a fence of this device; reading its completed value waits on nothing.
        let free = ring
            .textures
            .iter()
            .position(|texture| unsafe { texture.done.d3d11.GetCompletedValue() } >= texture.due)?;
        let Native::D3d11(captured) = frame.native();
        if !processor.blit(&self.gpu, captured.texture(), &ring.textures[free].d3d11) {
            return None;
        }
        ring.signalled += 1;
        {
            let _locked = self.gpu.lock();
            // SAFETY: the fence is this device's; the context is held under the device's lock, and the signal follows
            // the blit on it.
            unsafe { self.context.Signal(&ring.written.d3d11, ring.signalled) }.ok()?;
            // Submitted now: with no encode after it (a share nobody watches), the window's queue would wait for one.
            // SAFETY: as above.
            unsafe { self.context.Flush() };
        }
        let texture = &mut ring.textures[free];
        texture.due += 1;
        Some(PreviewPicture {
            resource: texture.d3d12.clone(),
            texture: ring.size,
            size,
            written: (ring.written.d3d12.clone(), ring.signalled),
            done: Some((texture.done.d3d12.clone(), texture.due)),
        })
    }
}

impl Ring {
    fn new(gpu: &Gpu, window: &ID3D12Device, size: (u32, u32)) -> Option<Self> {
        let textures = (0..RING)
            .map(|_| {
                let d3d11 = shared_texture(gpu, size)?;
                let d3d12 = open(window, share_texture(&d3d11)?)?;
                Some(Texture { d3d11, d3d12, done: Fence::new(gpu, window)?, due: 0 })
            })
            .collect::<Option<Vec<_>>>();
        let Some(textures) = textures else {
            report::note!(
                info,
                "one's own share cannot be lent to the window's GPU",
                same_adapter = same_adapter(gpu, window) == Some(true),
                encoder_lost = gpu.is_lost(),
                // SAFETY: a live device; the call reads its state.
                window_lost = unsafe { window.GetDeviceRemovedReason() }.is_err()
            );
            return None;
        };
        Some(Self { size, drawing: None, textures, written: Fence::new(gpu, window)?, signalled: 0 })
    }
}

impl Fence {
    fn new(gpu: &Gpu, window: &ID3D12Device) -> Option<Self> {
        let made: ID3D11Device5 = gpu.device().cast().ok()?;
        let mut d3d11: Option<ID3D11Fence> = None;
        // SAFETY: the out-pointer is a live `Option` of the interface asked for.
        unsafe { made.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &raw mut d3d11) }.ok()?;
        let d3d11 = d3d11?;
        // SAFETY: a shared fence of this device; the handle is closed by `open`.
        let handle = unsafe { d3d11.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }.ok()?;
        Some(Self { d3d11, d3d12: open(window, handle)? })
    }
}

/// Whether the encoder's device and the window's are on one adapter, which a handle shared between them needs; none
/// where the encoder's cannot say.
fn same_adapter(gpu: &Gpu, window: &ID3D12Device) -> Option<bool> {
    let dxgi: IDXGIDevice = gpu.device().cast().ok()?;
    // SAFETY: a live device; the call gives its adapter.
    let adapter = unsafe { dxgi.GetAdapter() }.ok()?;
    // SAFETY: a live adapter; the call reads its description.
    let ours = unsafe { adapter.GetDesc() }.ok()?.AdapterLuid;
    // SAFETY: a live device; the call reads its adapter's id.
    let theirs = unsafe { window.GetAdapterLuid() };
    Some((ours.LowPart, ours.HighPart) == (theirs.LowPart, theirs.HighPart))
}

/// A BGRA texture of `size` the processor draws into and another device may open.
fn shared_texture(gpu: &Gpu, (width, height): (u32, u32)) -> Option<ID3D11Texture2D> {
    let described = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0) as u32,
        ..Default::default()
    };
    let mut made = None;
    // SAFETY: the description outlives the call.
    unsafe { gpu.device().CreateTexture2D(&raw const described, None, Some(&raw mut made)) }.ok()?;
    made
}

/// A handle another device opens `texture` by; `open` closes it.
fn share_texture(texture: &ID3D11Texture2D) -> Option<HANDLE> {
    let resource: IDXGIResource1 = texture.cast().ok()?;
    // SAFETY: a texture made shareable by NT handle.
    unsafe { resource.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }.ok()
}

/// What `handle` names, opened on the window's device; the handle is closed either way.
fn open<T: Interface>(window: &ID3D12Device, handle: HANDLE) -> Option<T> {
    let mut opened: Option<T> = None;
    // SAFETY: a handle made by `CreateSharedHandle` of a texture or a fence; the out-pointer is a live `Option`.
    let result = unsafe { window.OpenSharedHandle(handle, &raw mut opened) };
    // SAFETY: the handle is ours and closed once; the opened object holds its own reference.
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;
    opened
}
