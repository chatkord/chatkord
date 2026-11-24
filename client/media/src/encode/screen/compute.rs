//! Windows' scaled screen off the 3D engine: a D3D12 compute pass on a queue of its own reads the captured BGRA texture,
//! shared by handle, and writes the NV12 texture the encoder reads. Beside a game that fills the 3D engine, the video
//! processor's pass waits behind the game's draws (a share scaled to 1440 beside a 64 ms scene sent every 66 ms,
//! against 16.7 at its own size, on the RX 9060 XT) where a compute queue's does not: a 4K picture to 1440p NV12 took
//! 0.34 ms at the median on the compute queue beside that scene, 8.7 ms on the direct queue.
//!
//! The pass writes two plane textures of its own and copies them into the encoder's, so nothing relies on a driver
//! writing the planes of NV12 directly. A picture it cannot share (a texture made without a handle to share it by) is
//! converted by the video processor instead, which every pool the pass is on is also made for.

use crate::av::{Frame, HwDevice, HwFrames};
use crate::encode::{EncodeError, Settings};
use crate::probe::nv12_pool_shared;
use capture::{Gpu, Layout, Output, Processor};
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_FENCE_FLAG_SHARED, ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D_ROOT_SIGNATURE_VERSION_1, D3D12_COMMAND_LIST_TYPE_COMPUTE, D3D12_COMMAND_QUEUE_DESC,
    D3D12_COMPUTE_PIPELINE_STATE_DESC, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
    D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
    D3D12_DESCRIPTOR_RANGE, D3D12_DESCRIPTOR_RANGE_TYPE_SRV, D3D12_DESCRIPTOR_RANGE_TYPE_UAV, D3D12_FENCE_FLAG_NONE,
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_BARRIER,
    D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER,
    D3D12_ROOT_CONSTANTS, D3D12_ROOT_DESCRIPTOR_TABLE, D3D12_ROOT_PARAMETER, D3D12_ROOT_PARAMETER_0,
    D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS, D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_SIGNATURE_DESC,
    D3D12_ROOT_SIGNATURE_FLAG_NONE, D3D12_SHADER_BYTECODE, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SHADER_VISIBILITY_ALL, D3D12_SRV_DIMENSION_TEXTURE2D, D3D12_TEX2D_SRV,
    D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_UAV_DIMENSION_TEXTURE2D, D3D12_UNORDERED_ACCESS_VIEW_DESC, D3D12CreateDevice, D3D12SerializeRootSignature,
    ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12PipelineState, ID3D12Resource, ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIResource1};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{IUnknown, Interface, PCWSTR};

/// The shader's compiled form, by `scripts/shaders.sh`: DXIL as 32-bit words.
const SHADER: &[u32] = &include!("shader/scale.dxil.inc");

/// The group the shader works in: its `GROUP`, which sizes the dispatch.
const GROUP: u32 = 8;

/// The longest the pass is waited for on the processor, once it was submitted: a GPU that hung is not waited on for good.
const WAIT_MS: u32 = 250;

/// How many textures of each side are kept shared at once: a capture's pool and the encoder's are a few.
const KEPT: usize = 8;

/// The pass, and the video processor it leaves a picture to when this one cannot be shared.
pub(crate) struct Path {
    gpu: Gpu,
    pool: HwFrames,
    processor: Processor,
    pass: Pass,
    /// Pictures the pass converted, and those it left to the processor.
    computed: u64,
    processed: u64,
}

// SAFETY: the D3D11 objects belong to a device FFmpeg made multithread-protected, every use of its immediate context is
// between the device's own lock and unlock, and the D3D12 objects are free-threaded; `&mut self` keeps one thread on a
// path at a time.
unsafe impl Send for Path {}

impl Path {
    /// The pass that reads the top-left `read` of BGRA textures laid out as `source` on `device` (FFmpeg's D3D11VA
    /// device, which the capture draws into) and leaves NV12 of the settings' size, into a pool the encoder is opened
    /// on; `Screen` where there is no D3D12 device on its adapter, or none that takes the shader.
    pub(super) fn open(
        device: &HwDevice,
        settings: Settings,
        source: Layout,
        read: (u32, u32),
    ) -> Result<Self, EncodeError> {
        let side = |value: usize| u32::try_from(value).ok().filter(|v| *v > 0).ok_or(EncodeError::Settings);
        let size = (side(settings.width)?, side(settings.height)?);
        let input = (side(source.width as usize)?, side(source.height as usize)?);
        let gpu = device.gpu().ok_or(EncodeError::Device)?;
        let processor = Processor::new(&gpu, input, read, size, Output::Nv12).ok_or(EncodeError::Device)?;
        // A driver that makes no NV12 texture to share is one this pass is not for, which a first frame says.
        let pool = nv12_pool_shared(device, size).filter(|pool| pool.take().is_ok()).ok_or(EncodeError::Screen)?;
        let pass = Pass::new(&gpu, read, size).ok_or(EncodeError::Screen)?;
        Ok(Self { gpu, pool, processor, pass, computed: 0, processed: 0 })
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.pool.again().ok_or(EncodeError::Frames)
    }

    /// Converts `texture`, a BGRA texture of the source's size on this device, into a new NV12 frame of the pool,
    /// `pts` stamped, and returns when the GPU has written it.
    pub(super) fn blit(&mut self, texture: &ID3D11Texture2D, pts: i64) -> Result<Frame, EncodeError> {
        let mut converted = self.pool.take().map_err(|_| EncodeError::Frames)?;
        let output = converted.d3d11_texture().ok_or(EncodeError::Frames)?;
        if self.pass.run(&self.gpu, texture, &output) {
            self.computed += 1;
        } else if self.processor.blit(&self.gpu, texture, &output) {
            self.processed += 1;
            if self.processed == 1 {
                report::note!(
                    info,
                    "share: the compute pass skipped a picture, which the video processor converted",
                    computed = self.computed
                );
            }
        } else {
            return Err(EncodeError::Convert);
        }
        converted.set_pts(pts);
        converted.tag_bt709_limited();
        Ok(converted)
    }

    /// How many pictures the compute pass converted, as against the video processor.
    #[cfg(test)]
    fn computed(&self) -> u64 {
        self.computed
    }
}

/// A texture of the D3D11 device as the D3D12 one opens it, kept with the texture itself so its address names no other.
struct Shared {
    #[expect(dead_code, reason = "held so the address it was cached by stays this texture's")]
    texture: ID3D11Texture2D,
    resource: ID3D12Resource,
}

/// Textures already shared, by address; a texture that could not be shared, held so that its address names no other,
/// so that it is not tried every picture.
struct Textures {
    shared: HashMap<usize, Shared>,
    refused: Vec<ID3D11Texture2D>,
}

impl Textures {
    fn new() -> Self {
        Self { shared: HashMap::new(), refused: Vec::new() }
    }

    /// `texture` opened on `device`; none where it cannot be shared by handle.
    fn of(&mut self, device: &ID3D12Device, texture: &ID3D11Texture2D) -> Option<ID3D12Resource> {
        let key = texture.as_raw() as usize;
        if let Some(kept) = self.shared.get(&key) {
            return Some(kept.resource.clone());
        }
        if self.refused.iter().any(|refused| refused.as_raw() as usize == key) {
            return None;
        }
        // Textures are few and last as long as their pool; one more than the keeping allows starts the keeping afresh.
        if self.shared.len() + self.refused.len() >= KEPT {
            self.shared.clear();
            self.refused.clear();
        }
        let resource = share(device, texture);
        match &resource {
            Some(resource) => {
                self.shared.insert(key, Shared { texture: texture.clone(), resource: resource.clone() });
            }
            None => self.refused.push(texture.clone()),
        }
        resource
    }
}

/// `texture`, opened on the D3D12 `device` by a handle.
fn share(device: &ID3D12Device, texture: &ID3D11Texture2D) -> Option<ID3D12Resource> {
    let resource: IDXGIResource1 = texture.cast().ok()?;
    // SAFETY: a texture made shareable by NT handle, or an error that says it is not.
    let handle = unsafe { resource.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }.ok()?;
    open(device, handle)
}

/// What `handle` names, opened on `device`; the handle is closed either way.
fn open<T: Interface>(device: &ID3D12Device, handle: HANDLE) -> Option<T> {
    let mut opened: Option<T> = None;
    // SAFETY: a handle made by `CreateSharedHandle` of a texture or a fence; the out-pointer is a live `Option`.
    let result = unsafe { device.OpenSharedHandle(handle, &raw mut opened) };
    // SAFETY: the handle is ours and closed once; the opened object holds its own reference.
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;
    opened
}

/// The compute pass: a D3D12 device on the encoder's adapter, a compute queue of its own, and the fences that order its
/// work after the capture's and before the encoder's.
struct Pass {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    root: ID3D12RootSignature,
    pipeline: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    /// The planes the shader writes, which are then copied into the encoder's texture.
    luma: ID3D12Resource,
    chroma: ID3D12Resource,
    /// The capture's draw is done: signalled on the D3D11 side, waited on by the queue.
    ready: (ID3D11Fence, ID3D12Fence),
    /// The queue's work is done: signalled by the queue, waited on by the processor.
    done: ID3D12Fence,
    event: HANDLE,
    /// The last value of `ready` and of `done` taken.
    ready_at: u64,
    done_at: u64,
    read: (u32, u32),
    size: (u32, u32),
    inputs: Textures,
    outputs: Textures,
}

impl Pass {
    /// The pass reading the top-left `read` of a picture and writing `size` planes; none where any part of it cannot be
    /// made on this adapter.
    fn new(gpu: &Gpu, read: (u32, u32), size: (u32, u32)) -> Option<Self> {
        let dxgi: IDXGIDevice = gpu.device().cast().ok()?;
        // SAFETY: a live device; the call gives its adapter.
        let adapter: IUnknown = unsafe { dxgi.GetAdapter() }.ok()?.cast().ok()?;
        let mut made: Option<ID3D12Device> = None;
        // SAFETY: the adapter is live and the out-pointer is a live `Option` of the interface asked for.
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &raw mut made) }.ok()?;
        let device = made?;
        // SAFETY: a plain description of a compute queue.
        let queue: ID3D12CommandQueue = unsafe {
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_COMPUTE,
                ..Default::default()
            })
        }
        .ok()?;
        let root = root_signature(&device)?;
        let bytes: Vec<u8> = SHADER.iter().flat_map(|word| word.to_le_bytes()).collect();
        let description = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            // SAFETY: borrowed for the call, which takes its own reference; nothing is released for it.
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() },
            ..Default::default()
        };
        // SAFETY: the description and the bytes it points to outlive the call.
        let pipeline: ID3D12PipelineState =
            unsafe { device.CreateComputePipelineState(std::ptr::from_ref(&description)) }.ok()?;
        // SAFETY: plain descriptions.
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 3,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })
        }
        .ok()?;
        let (half_width, half_height) = (size.0 / 2, size.1 / 2);
        let luma = plane(&device, size, DXGI_FORMAT_R8_UNORM)?;
        let chroma = plane(&device, (half_width, half_height), DXGI_FORMAT_R8G8_UNORM)?;
        // SAFETY: a live heap and device; the handle is read once.
        let start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        // SAFETY: a live device.
        let step = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) } as usize;
        for (slot, (resource, format)) in
            [(&luma, DXGI_FORMAT_R8_UNORM), (&chroma, DXGI_FORMAT_R8G8_UNORM)].into_iter().enumerate()
        {
            let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                Format: format,
                ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };
            let at = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + step * (slot + 1) };
            // SAFETY: a live resource that allows unordered access, a description of its format, and a slot of the heap.
            unsafe { device.CreateUnorderedAccessView(resource, None, Some(&raw const view), at) };
        }
        // SAFETY: a compute allocator of this device.
        let allocator: ID3D12CommandAllocator =
            unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COMPUTE) }.ok()?;
        // SAFETY: an allocator of the same type, and a pipeline of this device.
        let list: ID3D12GraphicsCommandList =
            unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_COMPUTE, &allocator, &pipeline) }.ok()?;
        // SAFETY: a list just made, open.
        unsafe { list.Close() }.ok()?;
        let ready = shared_fence(gpu, &device)?;
        // SAFETY: a plain fence.
        let done: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
        // SAFETY: a plain auto-reset event.
        let event = unsafe { CreateEventW(None, false, false, None) }.ok()?;
        Some(Self {
            device,
            queue,
            allocator,
            list,
            root,
            pipeline,
            heap,
            luma,
            chroma,
            ready,
            done,
            event,
            ready_at: 0,
            done_at: 0,
            read,
            size,
            inputs: Textures::new(),
            outputs: Textures::new(),
        })
    }

    /// Converts `input`, which the capture drew on `gpu`, into `output`, waiting until the GPU has written it; false
    /// where either texture cannot be shared or the pass did not finish, with `output` for the processor to write.
    fn run(&mut self, gpu: &Gpu, input: &ID3D11Texture2D, output: &ID3D11Texture2D) -> bool {
        // A pass that was not waited out (a GPU that hung) still holds its list and planes: none is recorded over it.
        // SAFETY: a live fence of this device.
        if unsafe { self.done.GetCompletedValue() } < self.done_at {
            return false;
        }
        let Some(input) = self.inputs.of(&self.device, input) else { return false };
        let Some(output) = self.outputs.of(&self.device, output) else { return false };
        let Ok(context) = gpu.context().cast::<ID3D11DeviceContext4>() else { return false };
        self.ready_at += 1;
        {
            let _locked = gpu.lock();
            // SAFETY: the fence is this device's; the context is held under the device's lock, and the signal follows
            // everything the capture drew on it.
            if unsafe { context.Signal(&self.ready.0, self.ready_at) }.is_err() {
                return false;
            }
            // Submitted now: nothing else on the context would flush it before the wait on the other side.
            // SAFETY: as above.
            unsafe { context.Flush() };
        }
        if !self.submit(&input, &output) {
            return false;
        }
        self.done_at += 1;
        // SAFETY: a queue and a fence of the same device.
        if unsafe { self.queue.Signal(&self.done, self.done_at) }.is_err() {
            return false;
        }
        // SAFETY: a live fence and an event nothing else waits on.
        if unsafe { self.done.SetEventOnCompletion(self.done_at, self.event) }.is_err() {
            return false;
        }
        // SAFETY: a live event.
        let waited = unsafe { WaitForSingleObject(self.event, WAIT_MS) };
        waited == WAIT_OBJECT_0
    }

    /// Records and submits the pass from `input` to `output`, behind the capture's draw.
    fn submit(&self, input: &ID3D12Resource, output: &ID3D12Resource) -> bool {
        // Each call is on the list, allocator, queue or a resource of this device, between the list's reset and its close,
        // none of its earlier work still running (each pass is waited for); the descriptors are written while nothing
        // uses them.
        macro_rules! on_the_device {
            ($call:expr) => {
                // SAFETY: as the macro's users say: a call on this device's own objects, in the order above.
                unsafe { $call }
            };
        }
        let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() },
            },
        };
        let at = on_the_device!(self.heap.GetCPUDescriptorHandleForHeapStart());
        on_the_device!(self.device.CreateShaderResourceView(input, Some(&raw const view), at));
        if on_the_device!(self.allocator.Reset()).is_err()
            || on_the_device!(self.list.Reset(&self.allocator, &self.pipeline)).is_err()
        {
            return false;
        }
        let table = on_the_device!(self.heap.GetGPUDescriptorHandleForHeapStart());
        on_the_device!(self.list.SetComputeRootSignature(&self.root));
        on_the_device!(self.list.SetDescriptorHeaps(&[Some(self.heap.clone())]));
        on_the_device!(self.list.SetComputeRootDescriptorTable(0, table));
        let extent = [self.read.0, self.read.1];
        on_the_device!(self.list.SetComputeRoot32BitConstants(1, 2, extent.as_ptr().cast(), 0));
        on_the_device!(self.list.ResourceBarrier(&[
            transition(input, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE),
            transition(output, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST),
            transition(&self.luma, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
            transition(&self.chroma, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
        ]));
        on_the_device!(self.list.Dispatch(self.size.0.div_ceil(GROUP), self.size.1.div_ceil(GROUP), 1));
        on_the_device!(self.list.ResourceBarrier(&[
            transition(&self.luma, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(&self.chroma, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE),
        ]));
        for (plane, source) in [(0, &self.luma), (1, &self.chroma)] {
            let (to, from) = (copy_location(output, plane), copy_location(source, 0));
            on_the_device!(self.list.CopyTextureRegion(&raw const to, 0, 0, 0, &raw const from, None));
        }
        on_the_device!(self.list.ResourceBarrier(&[
            transition(input, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON),
            transition(output, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COMMON),
        ]));
        if on_the_device!(self.list.Close()).is_err() {
            return false;
        }
        if on_the_device!(self.queue.Wait(&self.ready.1, self.ready_at)).is_err() {
            return false;
        }
        let Ok(list) = self.list.cast::<ID3D12CommandList>() else { return false };
        on_the_device!(self.queue.ExecuteCommandLists(&[Some(list)]));
        true
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        // What the queue still runs reads and writes what is released here: it is waited for, as long as a pass is.
        // SAFETY: a live fence of this device.
        if unsafe { self.done.GetCompletedValue() } < self.done_at
            // SAFETY: a live fence and an event nothing else waits on.
            && unsafe { self.done.SetEventOnCompletion(self.done_at, self.event) }.is_ok()
        {
            // SAFETY: a live event.
            let _ = unsafe { WaitForSingleObject(self.event, WAIT_MS) };
        }
        // SAFETY: the event is ours and closed once.
        let _ = unsafe { CloseHandle(self.event) };
    }
}

/// A resource barrier that moves all of `resource` from `before` to `after`.
fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: borrowed for the call that takes the barrier, which does not release it.
                pResource: unsafe { std::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Subresource `index` of `resource`, as a copy names it.
fn copy_location(resource: &ID3D12Resource, index: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        // SAFETY: borrowed for the call that takes the location, which does not release it.
        pResource: unsafe { std::mem::transmute_copy(resource) },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: index },
    }
}

/// A plane the shader writes, left as a copy's source until a pass moves it.
fn plane(device: &ID3D12Device, (width, height): (u32, u32), format: DXGI_FORMAT) -> Option<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let description = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut made: Option<ID3D12Resource> = None;
    // SAFETY: the descriptions outlive the call and the out-pointer is a live `Option` of the interface asked for.
    unsafe {
        device.CreateCommittedResource(
            &raw const heap,
            D3D12_HEAP_FLAG_NONE,
            &raw const description,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            None,
            &raw mut made,
        )
    }
    .ok()?;
    made
}

/// The root signature the shader reads: its input and two planes in one table, and the picture's size as constants.
fn root_signature(device: &ID3D12Device) -> Option<ID3D12RootSignature> {
    let ranges = [
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        },
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
            NumDescriptors: 2,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 1,
        },
    ];
    let parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 2,
                    pDescriptorRanges: ranges.as_ptr(),
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 2 },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let description = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: 2,
        pParameters: parameters.as_ptr(),
        NumStaticSamplers: 0,
        pStaticSamplers: std::ptr::null(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    let mut blob = None;
    // SAFETY: the description and what it points to outlive the call.
    unsafe { D3D12SerializeRootSignature(&raw const description, D3D_ROOT_SIGNATURE_VERSION_1, &raw mut blob, None) }
        .ok()?;
    let blob = blob?;
    // SAFETY: a live blob; the call reads its pointer.
    let start = unsafe { blob.GetBufferPointer() };
    // SAFETY: a live blob; the call reads its size.
    let length = unsafe { blob.GetBufferSize() };
    // SAFETY: a blob's pointer and size describe its bytes, alive while it is.
    let bytes = unsafe { std::slice::from_raw_parts(start.cast::<u8>(), length) };
    // SAFETY: bytes a serializer made.
    unsafe { device.CreateRootSignature(0, bytes) }.ok()
}

/// A fence the D3D11 device signals and the D3D12 queue waits on.
fn shared_fence(gpu: &Gpu, device: &ID3D12Device) -> Option<(ID3D11Fence, ID3D12Fence)> {
    let made: ID3D11Device5 = gpu.device().cast().ok()?;
    let mut d3d11: Option<ID3D11Fence> = None;
    // SAFETY: the out-pointer is a live `Option` of the interface asked for.
    unsafe { made.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &raw mut d3d11) }.ok()?;
    let d3d11 = d3d11?;
    // SAFETY: a shared fence of this device; the handle is closed by `open`.
    let handle = unsafe { d3d11.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }.ok()?;
    let d3d12 = open(device, handle)?;
    Some((d3d11, d3d12))
}

#[cfg(test)]
mod tests {
    use super::super::d3d11::tests::{device, layout, limited, middle, settings};
    use super::*;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_SHARED,
        D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    };

    /// A BGRA texture of one colour, which another device may open when `shared`.
    fn colour(device: &HwDevice, (width, height): (u32, u32), [r, g, b]: [u8; 3], shared: bool) -> ID3D11Texture2D {
        let pixels: Vec<u8> = (0..width * height).flat_map(|_| [b, g, r, 255]).collect();
        let misc = if shared { D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0 } else { 0 };
        let described = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            MiscFlags: misc.cast_unsigned(),
            ..Default::default()
        };
        let data =
            D3D11_SUBRESOURCE_DATA { pSysMem: pixels.as_ptr().cast(), SysMemPitch: width * 4, SysMemSlicePitch: 0 };
        let mut made = None;
        // SAFETY: the description and the pixels outlive the call.
        unsafe {
            device.gpu().unwrap().device().CreateTexture2D(
                &raw const described,
                Some(&raw const data),
                Some(&raw mut made),
            )
        }
        .unwrap();
        made.unwrap()
    }

    fn is_close(got: [u8; 3], expected: [u8; 3]) -> bool {
        got.iter().zip(expected).all(|(g, e)| g.abs_diff(e) <= 2)
    }

    mod lab {
        use super::*;

        #[test]
        fn a_shared_picture_larger_than_the_stream_is_scaled_by_the_compute_pass_to_bt709_limited_nv12() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let source = (3840, 2160);
            let Ok(mut path) = Path::open(&device, settings((2560, 1440)), layout(source), source) else {
                return tier::skipped("no D3D12 device takes the pass here");
            };
            for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [128, 128, 128], [255, 255, 255], [0, 0, 0]] {
                let (got, size) = middle(&path.blit(&colour(&device, source, rgb, true), 0).expect("converted"));
                assert_eq!(size, (2560, 1440));
                let expected = limited(rgb, (0.2126, 0.0722));
                assert!(is_close(got, expected), "rgb {rgb:?}: {got:?} is not BT.709 limited {expected:?}");
            }
            assert_eq!(path.computed(), 6, "every picture was the pass's, none the video processor's");
        }

        #[test]
        fn a_picture_that_cannot_be_shared_is_left_to_the_video_processor_and_comes_out_the_same() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let source = (1920, 1080);
            let Ok(mut path) = Path::open(&device, settings((1280, 720)), layout(source), source) else {
                return tier::skipped("no D3D12 device takes the pass here");
            };
            let (got, _) = middle(&path.blit(&colour(&device, source, [0, 255, 0], false), 0).expect("converted"));
            assert_eq!(path.computed(), 0, "the processor's");
            assert!(is_close(got, limited([0, 255, 0], (0.2126, 0.0722))), "{got:?}");
        }
    }
}
