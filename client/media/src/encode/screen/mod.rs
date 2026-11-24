//! A captured screen on its way to the encoder: the captured buffer is read on a device with no copy and converted
//! there by a [`Convert`] chosen apart from the encoder, the first in one order that leaves what the encoder reads.

#[cfg(windows)]
mod compute;
#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
mod direct;
#[cfg(target_os = "linux")]
mod exported;
#[cfg(target_os = "linux")]
mod imported;
#[cfg(target_os = "linux")]
mod kept;
#[cfg(target_os = "linux")]
mod node;
#[cfg(target_os = "linux")]
pub(crate) use node::vulkan_on;
#[cfg(target_os = "linux")]
mod onto_vaapi;
/// The DMA-BUF the screen tests paint, standing in for the compositor's.
#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "../../../tests/suite/common/dmabuf.rs"]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a test's numbers are its own fixtures, bounded where it writes them"
)]
mod painted;
#[cfg(target_os = "linux")]
mod pass;
#[cfg(target_os = "linux")]
mod surfaces;
#[cfg(target_os = "linux")]
mod va;
#[cfg(target_os = "linux")]
mod vaapi;
#[cfg(target_os = "linux")]
mod vulkan;

use super::{EncodeError, Settings};
use crate::av::{Frame, HwDevice, HwFrames};
#[cfg(target_os = "linux")]
use crate::probe::Device;
use crate::probe::{Backend, Encoder, Runs};
use capture::{Frame as Captured, Layout};

/// How a screen's captured picture becomes what its encoder reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Convert {
    /// Our pass on Vulkan (`rgb2yuv.comp`): the captured buffer to NV12, a scaled pixel the mean of what it covers;
    /// for a VA-API encoder, on a Vulkan device of the same GPU, into the encoder's own surfaces.
    #[cfg(target_os = "linux")]
    Area,
    /// VA-API's own scaler: the captured buffer made a VA surface, to NV12, halved first where it is over twice the
    /// stream.
    #[cfg(target_os = "linux")]
    VaScaler,
    /// A D3D12 compute pass on a queue of its own, the captured texture shared into it: to NV12, off the 3D engine a
    /// game's draws fill (`scale.hlsl`).
    #[cfg(windows)]
    Compute,
    /// D3D11's video processor: the captured texture to NV12.
    #[cfg(windows)]
    Processor,
    /// The captured BGRA texture itself, to an encoder that converts it; it never scales.
    #[cfg(windows)]
    Captured,
}

/// What a convert leaves, on the device of the kind it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leaves {
    Nv12(Backend),
    #[cfg(windows)]
    Bgra(Backend),
}

impl Convert {
    /// The converts a screen tries, best first: on Linux our area pass, which keeps thin lines at any scale, then
    /// VA-API's scaler; on Windows the captured texture, which no pass of ours waits on, then the video processor.
    #[cfg(target_os = "linux")]
    const ORDER: [Convert; 2] = [Convert::Area, Convert::VaScaler];
    /// The converts a screen tries, best first: on Linux our area pass, which keeps thin lines at any scale, then
    /// VA-API's scaler; on Windows the captured texture, which no pass of ours waits on, then our compute pass, which
    /// the game's draws do not hold up, then the video processor.
    #[cfg(windows)]
    const ORDER: [Convert; 3] = [Convert::Captured, Convert::Compute, Convert::Processor];

    /// What it leaves for an encoder, on each kind of device it runs for.
    const fn leaves(self) -> &'static [Leaves] {
        match self {
            #[cfg(target_os = "linux")]
            Self::Area => &[Leaves::Nv12(Backend::Vulkan), Leaves::Nv12(Backend::VaApi)],
            #[cfg(target_os = "linux")]
            Self::VaScaler => &[Leaves::Nv12(Backend::VaApi)],
            #[cfg(windows)]
            Self::Compute | Self::Processor => &[Leaves::Nv12(Backend::D3d11)],
            #[cfg(windows)]
            Self::Captured => &[Leaves::Bgra(Backend::D3d11)],
        }
    }

    /// Whether it leaves a picture smaller than it reads.
    const fn scales(self) -> bool {
        match self {
            #[cfg(target_os = "linux")]
            Self::Area | Self::VaScaler => true,
            #[cfg(windows)]
            Self::Compute | Self::Processor => true,
            #[cfg(windows)]
            Self::Captured => false,
        }
    }

    /// Those of [`Convert::ORDER`] whose picture `encoder` reads, best first, scaling where the stream is smaller than
    /// its source: a hardware encoder reads NV12 on its own device, and BGRA there where FFmpeg lists it; a software
    /// encoder reads NV12 on the device that converts for it, brought to memory after. A screen takes the first that
    /// opens.
    pub(crate) fn for_screen(encoder: &Encoder, scaled: bool) -> impl Iterator<Item = Self> {
        let reads = |leaves: &Leaves| match (&encoder.runs, *leaves) {
            (Runs::Gpu(device) | Runs::Cpu { convert: Some(device) }, Leaves::Nv12(on)) => device.backend == on,
            #[cfg(windows)]
            (Runs::Gpu(device), Leaves::Bgra(on)) => device.backend == on && direct::takes_bgra(encoder.name),
            #[cfg(windows)]
            (Runs::Cpu { .. }, Leaves::Bgra(_)) => false,
            (Runs::Cpu { convert: None }, Leaves::Nv12(_)) => false,
        };
        Self::ORDER
            .into_iter()
            .filter(move |convert| (convert.scales() || !scaled) && convert.leaves().iter().any(reads))
    }
}

/// The devices a screen's path is opened on.
pub(crate) struct On<'a> {
    /// The encoder's device, of which the converted pictures are frames.
    pub(crate) device: &'a HwDevice,
    /// What that device is.
    #[cfg(target_os = "linux")]
    pub(crate) named: &'a Device,
    /// The Vulkan device on its GPU an area pass ran on before, kept open across a reopen; none to open one.
    #[cfg(target_os = "linux")]
    pub(crate) pass: Option<&'a HwDevice>,
}

/// The path for one size, channel order and device.
pub(crate) enum Path {
    #[cfg(target_os = "linux")]
    VaApi(vaapi::Path),
    #[cfg(target_os = "linux")]
    Vulkan(vulkan::Path),
    /// Our area pass on a Vulkan device beside a VA-API encoder's.
    #[cfg(target_os = "linux")]
    AreaOnVaApi(onto_vaapi::Path),
    #[cfg(windows)]
    D3d11(d3d11::Path),
    /// The D3D12 compute pass, the video processor behind it.
    #[cfg(windows)]
    Compute(Box<compute::Path>),
    /// The captured texture itself, to an encoder that converts it.
    #[cfg(windows)]
    Direct(direct::Path),
}

impl Path {
    /// The path of `convert`, on `on`'s devices, that reads the top-left `read` of captured pictures laid out as
    /// `source` and leaves the settings' size, scaled where that is not `read`; `Screen` where this machine lacks what
    /// that convert needs, for the next to be tried.
    pub(crate) fn open(
        convert: Convert,
        on: &On<'_>,
        settings: Settings,
        source: Layout,
        read: (u32, u32),
    ) -> Result<Self, EncodeError> {
        let device = on.device;
        match convert {
            #[cfg(target_os = "linux")]
            Convert::Area => match on.named.backend {
                Backend::Vulkan => vulkan::Path::open(device, settings, source, read).map(Self::Vulkan),
                Backend::VaApi => onto_vaapi::Path::open(on, settings, source, read).map(Self::AreaOnVaApi),
            },
            #[cfg(target_os = "linux")]
            Convert::VaScaler => vaapi::Path::open(device, settings, source, read).map(Self::VaApi),
            #[cfg(windows)]
            Convert::Compute => {
                compute::Path::open(device, settings, source, read).map(|path| Self::Compute(Box::new(path)))
            }
            #[cfg(windows)]
            Convert::Processor => d3d11::Path::open(device, settings, source, read).map(Self::D3d11),
            #[cfg(windows)]
            Convert::Captured => direct::Path::open(device, source, read).map(Self::Direct),
        }
    }

    /// The Vulkan device our pass runs on where it is not the encoder's own, which a reopen on the same encoder
    /// device keeps; none where the convert runs on the encoder's device.
    #[cfg(target_os = "linux")]
    pub(crate) const fn pass_device(&self) -> Option<&HwDevice> {
        match self {
            Self::AreaOnVaApi(path) => Some(path.vulkan()),
            Self::VaApi(_) | Self::Vulkan(_) => None,
        }
    }

    /// The convert it is.
    pub(crate) const fn kind(&self) -> Convert {
        match self {
            #[cfg(target_os = "linux")]
            Self::VaApi(_) => Convert::VaScaler,
            #[cfg(target_os = "linux")]
            Self::Vulkan(_) | Self::AreaOnVaApi(_) => Convert::Area,
            #[cfg(windows)]
            Self::Compute(_) => Convert::Compute,
            #[cfg(windows)]
            Self::D3d11(_) => Convert::Processor,
            #[cfg(windows)]
            Self::Direct(_) => Convert::Captured,
        }
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(crate) fn frames(&self) -> Result<HwFrames, EncodeError> {
        match self {
            #[cfg(target_os = "linux")]
            Self::VaApi(path) => path.frames(),
            #[cfg(target_os = "linux")]
            Self::Vulkan(path) => path.frames(),
            #[cfg(target_os = "linux")]
            Self::AreaOnVaApi(path) => path.frames(),
            #[cfg(windows)]
            Self::Compute(path) => path.frames(),
            #[cfg(windows)]
            Self::D3d11(path) => path.frames(),
            #[cfg(windows)]
            Self::Direct(path) => path.frames(),
        }
    }

    /// Waits for every picture it still converts, so each captured buffer is its owner's again.
    pub(crate) fn settle(&mut self) -> Result<(), EncodeError> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Vulkan(path) => path.settle(),
            #[cfg(target_os = "linux")]
            Self::AreaOnVaApi(path) => path.settle(),
            // Each of these is done with its buffer when its convert returns, or once the GPU has done what it queued.
            #[cfg(target_os = "linux")]
            Self::VaApi(_) => Ok(()),
            #[cfg(windows)]
            Self::Compute(_) | Self::D3d11(_) | Self::Direct(_) => Ok(()),
        }
    }

    /// Reads `captured` on the GPU and returns the NV12 picture, `pts` stamped. The captured buffer goes back as soon
    /// as the GPU is done with it.
    pub(crate) fn convert(&mut self, captured: Captured, pts: i64) -> Result<Frame, EncodeError> {
        match self {
            #[cfg(target_os = "linux")]
            Self::VaApi(path) => path.convert(captured, pts),
            #[cfg(target_os = "linux")]
            Self::Vulkan(path) => path.convert(captured, pts),
            #[cfg(target_os = "linux")]
            Self::AreaOnVaApi(path) => path.convert(captured, pts),
            // The pass is done when it returns; the texture goes back when `captured` drops.
            #[cfg(windows)]
            Self::Compute(path) => match captured.native() {
                capture::Native::D3d11(texture) => path.blit(texture.texture(), pts),
            },
            // The texture goes back when `captured` drops, once the GPU has done the processor's pass it queued.
            #[cfg(windows)]
            Self::D3d11(path) => match captured.native() {
                capture::Native::D3d11(texture) => path.blit(texture.texture(), pts),
            },
            #[cfg(windows)]
            Self::Direct(path) => path.wrap(captured, pts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use crate::probe::Device;
    use crate::tuning::Name;
    use capture::Validation;

    fn on(backend: Backend) -> Device {
        Device { backend, node: None, vendor: None, validation: Validation::Off }
    }

    #[test]
    fn each_encoder_takes_the_first_convert_in_order_that_leaves_what_it_reads() {
        #[cfg(target_os = "linux")]
        {
            let gpu = |name, backend| Encoder { name, runs: Runs::Gpu(on(backend)), multiple: crate::Multiple::EVEN };
            let cpu = |backend| Encoder {
                name: Name::X264,
                runs: Runs::Cpu { convert: Some(on(backend)) },
                multiple: crate::Multiple::EVEN,
            };
            let tried = |encoder: &Encoder, scaled| Convert::for_screen(encoder, scaled).collect::<Vec<_>>();
            for scaled in [false, true] {
                assert_eq!(tried(&gpu(Name::H264Vulkan, Backend::Vulkan), scaled), [Convert::Area]);
                let vaapi = gpu(Name::H264Vaapi, Backend::VaApi);
                assert_eq!(tried(&vaapi, scaled), [Convert::Area, Convert::VaScaler], "our pass, then VA-API's scaler");
                assert_eq!(tried(&cpu(Backend::Vulkan), scaled), [Convert::Area]);
                assert_eq!(tried(&cpu(Backend::VaApi), scaled), [Convert::Area, Convert::VaScaler]);
            }
        }
        #[cfg(windows)]
        {
            let amf =
                Encoder { name: Name::H264Amf, runs: Runs::Gpu(on(Backend::D3d11)), multiple: crate::Multiple::EVEN };
            let first = |encoder: &Encoder, scaled| Convert::for_screen(encoder, scaled).next();
            assert_eq!(first(&amf, false), Some(Convert::Captured), "AMF takes the captured texture");
            assert_eq!(first(&amf, true), Some(Convert::Compute), "our compute pass scales it, ahead of the processor");
            assert_eq!(
                Convert::for_screen(&amf, true).collect::<Vec<_>>(),
                [Convert::Compute, Convert::Processor],
                "and the processor is the next if it cannot"
            );
            let x264 = Encoder {
                name: Name::X264,
                runs: Runs::Cpu { convert: Some(on(Backend::D3d11)) },
                multiple: crate::Multiple::EVEN,
            };
            assert_eq!(first(&x264, false), Some(Convert::Compute), "software reads NV12");
        }
        let memory = Encoder { name: Name::X264, runs: Runs::Cpu { convert: None }, multiple: crate::Multiple::EVEN };
        assert_eq!(Convert::for_screen(&memory, false).next(), None, "no device converts a screen for it");
    }
}
