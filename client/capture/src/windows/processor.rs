//! The device's video processor, which writes a captured BGRA texture resized as NV12 or BGRA: FFmpeg's `scale_d3d11`
//! sets no colour space and asks for an array the RX 9060 XT refuses.

use std::cell::RefCell;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Texture2D,
    ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorEnumerator1, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_TYPE, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12,
};
use windows::core::Interface;

use super::Gpu;

/// What the processor writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The encoder's NV12, BT.709 in limited range.
    Nv12,
    /// BGRA, as it was captured: a still to show.
    Bgra,
}

impl Output {
    fn format(self) -> DXGI_FORMAT {
        match self {
            Self::Nv12 => DXGI_FORMAT_NV12,
            Self::Bgra => DXGI_FORMAT_B8G8R8A8_UNORM,
        }
    }

    fn colour(self) -> DXGI_COLOR_SPACE_TYPE {
        match self {
            Self::Nv12 => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            Self::Bgra => DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
        }
    }
}

/// Rate conversion 0, the one every video processor has.
const RATE_CONVERSION: u32 = 0;

/// How many views of each kind a processor keeps: a capture's pool and the textures it writes are a handful each.
const VIEWS: usize = 8;

/// The views a processor made, by the texture they view, so a picture costs no new view. A view holds its texture, so
/// a texture's address is not reused while its view is kept.
struct Views<V> {
    kept: Vec<(*mut std::ffi::c_void, V)>,
}

impl<V: Clone> Views<V> {
    fn of(&mut self, texture: &ID3D11Texture2D, make: impl FnOnce() -> Option<V>) -> Option<V> {
        let key = texture.as_raw();
        if let Some((_, view)) = self.kept.iter().find(|(kept, _)| *kept == key) {
            return Some(view.clone());
        }
        let view = make()?;
        if self.kept.len() == VIEWS {
            self.kept.remove(0);
        }
        self.kept.push((key, view.clone()));
        Some(view)
    }
}

/// The device's video processor for one input size, output size and output.
pub struct Processor {
    video: ID3D11VideoDevice,
    context: ID3D11VideoContext1,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    source: (u32, u32),
    reads: RefCell<Views<ID3D11VideoProcessorInputView>>,
    writes: RefCell<Views<ID3D11VideoProcessorOutputView>>,
}

impl Processor {
    /// The processor that reads the top-left `read` of full-range BGRA textures of `source` on `gpu` and writes
    /// `output` of `size`, scaled where that is not `read`; none for a device that cannot, which is refused, never run
    /// on a guess.
    pub fn new(gpu: &Gpu, source: (u32, u32), read: (u32, u32), size: (u32, u32), output: Output) -> Option<Self> {
        Self::into_region(gpu, source, read, size, size, output)
    }

    /// The same, writing the picture at `drawn` into the top left of an output of `size`, which holds it.
    pub fn into_region(
        gpu: &Gpu,
        source: (u32, u32),
        read: (u32, u32),
        size: (u32, u32),
        drawn: (u32, u32),
        output: Output,
    ) -> Option<Self> {
        if drawn.0 > size.0 || drawn.1 > size.1 || read.0 > source.0 || read.1 > source.1 {
            return None;
        }
        let video: ID3D11VideoDevice = gpu.device().cast().ok()?;
        let context: ID3D11VideoContext1 = gpu.context().cast().ok()?;
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputWidth: source.0,
            InputHeight: source.1,
            OutputWidth: size.0,
            OutputHeight: size.1,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            ..Default::default()
        };
        // SAFETY: the description is a valid value and outlives the call.
        let enumerator = unsafe { video.CreateVideoProcessorEnumerator(&raw const content) }.ok()?;
        let converts = enumerator.cast::<ID3D11VideoProcessorEnumerator1>().is_ok_and(|enumerator| {
            // SAFETY: plain values in, a BOOL out.
            unsafe {
                enumerator.CheckVideoProcessorFormatConversion(
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                    output.format(),
                    output.colour(),
                )
            }
            .is_ok_and(|supported| supported.as_bool())
        });
        if !converts {
            return None;
        }
        // SAFETY: the enumerator belongs to this device and the index is one it has.
        let processor = unsafe { video.CreateVideoProcessor(&enumerator, RATE_CONVERSION) }.ok()?;
        let read = RECT { left: 0, top: 0, right: i32::try_from(read.0).ok()?, bottom: i32::try_from(read.1).ok()? };
        let region =
            RECT { left: 0, top: 0, right: i32::try_from(drawn.0).ok()?, bottom: i32::try_from(drawn.1).ok()? };
        {
            let _locked = gpu.lock();
            // SAFETY: the processor is this context's device's; the context is held under the device's lock.
            unsafe {
                context.VideoProcessorSetStreamColorSpace1(&processor, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709)
            };
            // SAFETY: as above.
            unsafe { context.VideoProcessorSetOutputColorSpace1(&processor, output.colour()) };
            // The driver's own enhancements would change the picture between machines.
            // SAFETY: as above.
            unsafe { context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false) };
            // SAFETY: as above; the rectangle outlives the call.
            unsafe { context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&raw const read)) };
            // SAFETY: as above.
            unsafe { context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&raw const region)) };
        }
        let (reads, writes) = (RefCell::new(Views { kept: Vec::new() }), RefCell::new(Views { kept: Vec::new() }));
        Some(Self { video, context, enumerator, processor, source, reads, writes })
    }

    /// Queues `input`, a BGRA texture of the source's size, drawn into `output`, a render target of the output's size
    /// and format, both on `gpu`'s device; what reads `output` next on the context waits for it. False when refused.
    pub fn blit(&self, gpu: &Gpu, input: &ID3D11Texture2D, output: &ID3D11Texture2D) -> bool {
        let mut described = Default::default();
        // SAFETY: a live texture fills a plain description.
        unsafe { input.GetDesc(&raw mut described) };
        if (described.Width, described.Height) != self.source || described.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return false;
        }
        let reading = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 },
            },
        };
        let writing = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
        };
        let reads = self.reads.borrow_mut().of(input, || {
            let mut made = None;
            // SAFETY: the texture is this device's and the description outlives the call.
            unsafe {
                self.video.CreateVideoProcessorInputView(
                    input,
                    &self.enumerator,
                    &raw const reading,
                    Some(&raw mut made),
                )
            }
            .ok()?;
            made
        });
        let writes = self.writes.borrow_mut().of(output, || {
            let mut made = None;
            // SAFETY: as above.
            unsafe {
                self.video.CreateVideoProcessorOutputView(
                    output,
                    &self.enumerator,
                    &raw const writing,
                    Some(&raw mut made),
                )
            }
            .ok()?;
            made
        });
        let (Some(reads), Some(writes)) = (reads, writes) else { return false };
        let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(reads)),
            ..Default::default()
        }];
        let done = {
            let _locked = gpu.lock();
            // SAFETY: the views and the processor are this device's; the context is held under the device's lock.
            unsafe { self.context.VideoProcessorBlt(&self.processor, &writes, 0, &streams) }
        };
        // SAFETY: the input view was moved into the stream for the call and is released here, once.
        unsafe { ManuallyDrop::drop(&mut streams[0].pInputSurface) };
        done.is_ok()
    }
}
