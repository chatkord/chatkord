//! Which encoders really start on this machine. What FFmpeg was built with proves nothing about the GPU, so each
//! hardware encoder is counted only if it opens on a device, by the same calls an encode makes first; a software
//! encoder is counted if it opens, and comes after every hardware one of its codec.

use crate::av::{CodecContext, HwDevice, HwFrames, PixelFormat};
use crate::multiple::Multiple;
use crate::tuning::{self, Measured, Name};
use capture::Validation;
use std::fmt;

/// A video codec the client can send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Codec {
    /// Sent as High, which the router offers.
    H264,
    /// AV1.
    Av1,
}

/// What drives the encoder or decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, report::Public)]
pub enum Backend {
    /// Linux's VA-API, on a render node.
    VaApi,
    /// Vulkan's default device (Linux).
    Vulkan,
    /// Windows' D3D11 device, which every Windows encoder (AMF, NVENC, Quick Sync, Media Foundation) takes.
    #[cfg(windows)]
    D3d11,
    /// The window's own D3D12 device, which decodes a picture where it is painted (Windows); no encoder takes it.
    #[cfg(windows)]
    D3d12,
}

/// How a stream is coded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Path {
    /// On a device of this kind.
    Hardware(#[public] Backend),
    /// On the processor: FFmpeg's own decoder, or a software encoder.
    Software,
}

impl Backend {
    /// The format a frame on this backend's device has.
    pub(crate) const fn pixel_format(self) -> PixelFormat {
        crate::av::device_format(self)
    }
}

/// A pool of NV12 frames of `size` on `device` of `backend`, the shape each backend's encoders take.
pub(crate) fn nv12_pool(device: &HwDevice, backend: Backend, size: (u32, u32)) -> Option<HwFrames> {
    pool(device, backend, PixelFormat::Nv12, size)
}

/// A pool of NV12 frames of `size` on the D3D11 `device`, whose textures another device opens by handle (Windows).
#[cfg(windows)]
pub(crate) fn nv12_pool_shared(device: &HwDevice, size: (u32, u32)) -> Option<HwFrames> {
    HwFrames::shared_nv12(device, size)
}

/// A pool of frames of `size` on `device` of `backend` that hold pictures laid out as `layout` in memory: what a
/// picture is uploaded into as it is.
pub(crate) fn pool(device: &HwDevice, backend: Backend, layout: PixelFormat, size: (u32, u32)) -> Option<HwFrames> {
    HwFrames::new(device, backend, layout, size, POOL_FRAMES)
}

/// How many frames a pool makes at once.
const POOL_FRAMES: u32 = 8;

/// The PCI id of a GPU's vendor, which says whose driver and encoder a device has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciVendor(pub u16);

impl PciVendor {
    /// AMD's (ATI's) GPUs.
    pub const AMD: Self = Self(0x1002);
    /// Intel's GPUs.
    pub const INTEL: Self = Self(0x8086);
    /// NVIDIA's GPUs.
    pub const NVIDIA: Self = Self(0x10de);
}

/// A video device on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Its kind.
    pub backend: Backend,
    /// The render node: VA-API's own, or the GPU's whose Vulkan device it is; on Windows the adapter's id (an
    /// `AdapterId`, as it is written); none for a backend's default device.
    pub node: Option<String>,
    /// Its GPU's vendor, as the survey read it; none where it could not be read (Vulkan's default device on Linux).
    pub vendor: Option<PciVendor>,
    /// Whether it is opened under its API's debug layer.
    pub validation: Validation,
}

/// Where an encoder codes its pictures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runs {
    /// On this device's own encoder.
    Gpu(Device),
    /// On the processor. `convert` reads and converts a screen's pictures for it; with none it takes pictures from
    /// memory only.
    Cpu {
        /// The device a screen's pictures are read on.
        convert: Option<Device>,
    },
}

/// An encoder that opened on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoder {
    /// Which encoder it is.
    pub name: Name,
    /// Where it codes its pictures.
    pub runs: Runs,
    /// What its stream's size is a multiple of, so no padding shows.
    pub multiple: Multiple,
}

impl Encoder {
    /// What it encodes.
    pub const fn codec(&self) -> Codec {
        self.name.codec()
    }

    /// The device a session of it opens: its own, or the one that converts a screen for it.
    pub fn device(&self) -> Option<&Device> {
        match &self.runs {
            Runs::Gpu(device) => Some(device),
            Runs::Cpu { convert } => convert.as_ref(),
        }
    }

    /// Where it stands among its codec's encoders: measured only on a GPU of a vendor its family's row was measured on.
    pub fn rank(&self) -> Rank {
        match (&self.runs, self.name.family().measured()) {
            (Runs::Gpu(device), Measured::On(gpus))
                if gpus.iter().any(|(vendor, _)| device.vendor == Some(*vendor)) =>
            {
                Rank::Measured
            }
            (Runs::Gpu(_), Measured::On(_) | Measured::OnProcessor(_) | Measured::Untested) => Rank::Untested,
            (Runs::Cpu { .. }, _) => Rank::Software,
        }
    }

    /// The format a software encoder is given its pictures in; none for one on a GPU, which takes its device's frames.
    pub(crate) fn software_format(&self) -> Option<PixelFormat> {
        match self.runs {
            Runs::Cpu { .. } => self.name.family().software_format(),
            Runs::Gpu(_) => None,
        }
    }

    /// How it codes a stream, as a card reports it.
    pub fn path(&self) -> Path {
        match &self.runs {
            Runs::Gpu(device) => Path::Hardware(device.backend),
            Runs::Cpu { .. } => Path::Software,
        }
    }
}

impl fmt::Display for Encoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.runs {
            Runs::Gpu(device) => write!(f, "{} ({:?})", self.name, device.backend),
            Runs::Cpu { convert: Some(device) } => write!(f, "{} (software, from {:?})", self.name, device.backend),
            Runs::Cpu { convert: None } => write!(f, "{} (software)", self.name),
        }
    }
}

/// Where an encoder stands among its codec's, best first: a GPU's whose family was measured on its vendor's GPUs, a
/// GPU's whose family nobody measured there, then the processor's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// On a GPU, by a family measured on the test setup on a GPU of the same vendor.
    Measured,
    /// On a GPU, by a family nobody measured on its vendor's GPUs: tried before software, which is slower, and said to
    /// be untested.
    Untested,
    /// On the processor.
    Software,
}

#[cfg(not(windows))]
const CANDIDATES: [(Name, Backend); 4] = [
    (Name::H264Vaapi, Backend::VaApi),
    (Name::Av1Vaapi, Backend::VaApi),
    (Name::H264Vulkan, Backend::Vulkan),
    (Name::Av1Vulkan, Backend::Vulkan),
];

/// Windows' encoders all take the D3D11 device, and each opens only on its own vendor's GPU. Media Foundation's are
/// not offered: its first H.264 encoder is Microsoft's software one, and its hardware one writes baseline without
/// the constrained flag the session description promises (both seen on the RX 9060 XT).
#[cfg(windows)]
const CANDIDATES: [(Name, Backend); 6] = [
    (Name::H264Amf, Backend::D3d11),
    (Name::H264Nvenc, Backend::D3d11),
    (Name::H264Qsv, Backend::D3d11),
    (Name::Av1Amf, Backend::D3d11),
    (Name::Av1Nvenc, Backend::D3d11),
    (Name::Av1Qsv, Backend::D3d11),
];

/// The software encoders, tried after every hardware encoder of their codec: SVT-AV1 (BSD-3-Clause-Clear) and x264
/// (GPL-2.0-or-later, as the client's licence takes).
const SOFTWARE: [Name; 2] = [Name::SvtAv1, Name::X264];

/// Why an encoder did not open here, plainly, so a machine that reports none is understood from its report alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// There is no device to open: no render node, or no Vulkan loader.
    NoDevice,
    /// The driver refused to open the device.
    DeviceRefused,
    /// This FFmpeg was not built with the encoder.
    NotBuilt,
    /// The device cannot make frames of the size the encoder takes.
    FramesRefused,
    /// The driver does not support encoding this codec.
    EncoderRefused,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoDevice => "no device to open",
            Self::DeviceRefused => "the driver refused the device",
            Self::NotBuilt => "not built into this FFmpeg",
            Self::FramesRefused => "the device makes no frames of the size it takes",
            Self::EncoderRefused => "the driver does not encode this codec",
        })
    }
}

/// An encoder that did not open, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    /// Which encoder it is.
    pub name: Name,
    /// How it would have coded: on the device kind it was tried on, or in software.
    pub path: Path,
    /// What stopped it, at the first step that failed.
    pub why: Why,
}

impl Unavailable {
    /// What it would have encoded.
    pub const fn codec(&self) -> Codec {
        self.name.codec()
    }
}

/// What this machine can encode, and what it cannot and why. Every candidate is in exactly one list, a software
/// encoder once for each device that converts a screen for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The whole list a call may be offered from, by codec; when it is empty nothing is offered.
    pub encoders: Vec<Encoder>,
    /// Every candidate that did not open.
    pub unavailable: Vec<Unavailable>,
}

impl Report {
    /// The encoders to try for a codec on pictures in memory, a camera's, best first: every hardware one in
    /// [`Report::screen_order`]'s order, then each software one once, since one in memory needs no device to convert on.
    pub fn memory_order(&self, codec: Codec) -> impl Iterator<Item = &Encoder> {
        let hardware = self.screen_order(codec).filter(|encoder| encoder.path() != Path::Software);
        let software = self.encoders.iter().enumerate().filter_map(move |(at, encoder)| {
            let first = !self.encoders[..at].iter().any(|before| before.name == encoder.name);
            (encoder.codec() == codec && encoder.path() == Path::Software && first).then_some(encoder)
        });
        hardware.chain(software)
    }

    /// The encoders to try for a codec on the screen's pictures, best first: by [`Rank`], and within one on Linux
    /// Vulkan, then VA-API as the failover, and on Windows D3D11. The first that opens for the screen is the one to
    /// use, and the next takes over when it fails. An encoder with no device takes no screen.
    pub fn screen_order(&self, codec: Codec) -> impl Iterator<Item = &Encoder> {
        [Rank::Measured, Rank::Untested, Rank::Software].into_iter().flat_map(move |rank| {
            SCREEN_ORDER.into_iter().flat_map(move |backend| {
                self.encoders.iter().filter(move |encoder| {
                    encoder.codec() == codec
                        && encoder.rank() == rank
                        && encoder.device().is_some_and(|device| device.backend == backend)
                })
            })
        })
    }
}

/// The device kinds that take a screen's pictures here, best first: what [`Report::screen_order`] tries in turn.
#[cfg(not(windows))]
pub const SCREEN_ORDER: [Backend; 2] = [Backend::Vulkan, Backend::VaApi];
/// The device kinds that take a screen's pictures here, best first: what [`Report::screen_order`] tries in turn.
#[cfg(windows)]
pub const SCREEN_ORDER: [Backend; 1] = [Backend::D3d11];

/// Where the probe looks. The system by default; tests narrow it.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    /// Render nodes to try for VA-API, in order.
    pub render_nodes: Vec<String>,
    /// Whether to try Vulkan.
    pub vulkan: bool,
    /// The render node whose GPU's Vulkan device is tried; none: Vulkan's default device.
    pub vulkan_node: Option<String>,
    /// Whether to try a D3D11 device (Windows): on `adapter` where one is named, else the default adapter.
    pub d3d11: bool,
    /// The adapter the D3D11 device is made on, the one that paints the window; none: the default adapter.
    #[cfg(windows)]
    pub adapter: Option<capture::AdapterId>,
    /// The window's D3D12 device, tried before D3D11 so a picture is decoded where it is painted.
    #[cfg(windows)]
    pub d3d12: Option<crate::D3d12Device>,
    /// Whether the D3D11 device is opened under its debug layer.
    pub validation: Validation,
}

/// The device a hardware encoder or decoder opens on.
#[derive(Clone, Debug)]
pub(crate) enum On {
    /// The backend's default device.
    Default,
    /// The backend's default device, under its API's debug layer.
    #[cfg(windows)]
    Debug,
    /// A VA-API render node.
    Node(String),
    /// The window's own D3D12 device.
    #[cfg(windows)]
    Window(crate::D3d12Device),
}

impl On {
    /// Makes the device; where that enters the Vulkan loader, [`HwDevice::open`] takes its lock.
    pub(crate) fn open(&self, backend: Backend) -> Option<HwDevice> {
        match self {
            Self::Default => device_on(backend, None, Validation::Off),
            #[cfg(windows)]
            Self::Debug => device_on(backend, None, Validation::On),
            Self::Node(node) => device_on(backend, Some(node), Validation::Off),
            #[cfg(windows)]
            Self::Window(device) => HwDevice::on_d3d12(&device.0),
        }
    }

    /// Whether the device is opened under its API's debug layer.
    fn validation(&self) -> Validation {
        match self {
            #[cfg(windows)]
            Self::Debug => Validation::On,
            _ => Validation::Off,
        }
    }

    /// The vendor of the GPU it opens on: a render node's as the kernel lists it.
    #[cfg(target_os = "linux")]
    fn vendor(&self) -> Option<PciVendor> {
        self.node().and_then(crate::drivers::node_vendor)
    }

    /// The vendor of the GPU it opens on: a named adapter's, or the default adapter's, as DXGI describes it.
    #[cfg(windows)]
    fn vendor(&self) -> Option<PciVendor> {
        match self {
            Self::Default | Self::Debug => crate::survey::adapter_vendor(None),
            Self::Node(adapter) => {
                adapter.parse().ok().and_then(|adapter| crate::survey::adapter_vendor(Some(adapter)))
            }
            Self::Window(_) => None,
        }
    }

    /// No GPU's vendor is read here.
    #[cfg(not(any(target_os = "linux", windows)))]
    fn vendor(&self) -> Option<PciVendor> {
        None
    }

    /// The render node, where it is one.
    pub(crate) fn node(&self) -> Option<&str> {
        match self {
            Self::Node(node) => Some(node),
            Self::Default => None,
            #[cfg(windows)]
            Self::Debug => None,
            #[cfg(windows)]
            Self::Window(_) => None,
        }
    }
}

impl Sources {
    /// The hardware devices these sources name, in the order they are tried: VA-API's render nodes, Vulkan, the
    /// window's D3D12 device, then D3D11.
    pub(crate) fn devices(&self) -> Vec<(Backend, On)> {
        let mut devices: Vec<(Backend, On)> =
            self.render_nodes.iter().map(|node| (Backend::VaApi, On::Node(node.clone()))).collect();
        if self.vulkan {
            devices.push((Backend::Vulkan, self.vulkan_node.clone().map_or(On::Default, On::Node)));
        }
        #[cfg(windows)]
        if let Some(device) = &self.d3d12 {
            devices.push((Backend::D3d12, On::Window(device.clone())));
        }
        #[cfg(windows)]
        if self.d3d11 {
            let on = match (self.adapter, self.validation) {
                (Some(adapter), _) => On::Node(adapter.to_string()),
                (None, Validation::Off) => On::Default,
                (None, Validation::On) => On::Debug,
            };
            devices.push((Backend::D3d11, on));
        }
        devices
    }

    /// The default D3D11 device: FFmpeg is built on Windows without Vulkan.
    #[cfg(windows)]
    pub fn system() -> Self {
        Self { d3d11: true, ..Self::default() }
    }

    /// Every render node under `/dev/dri`, and Vulkan.
    #[cfg(not(windows))]
    pub fn system() -> Self {
        let mut render_nodes: Vec<String> = std::fs::read_dir("/dev/dri")
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path().display().to_string())
                    .filter(|path| path.contains("renderD"))
                    .collect()
            })
            .unwrap_or_default();
        render_nodes.sort();
        Self { render_nodes, vulkan: true, ..Self::default() }
    }

    /// VA-API and Vulkan on `node`'s GPU alone, the one that paints the window: a share's pictures are there, and
    /// another GPU's encoder would cost a copy between them (and wake that GPU from sleep). None: software alone.
    #[cfg(not(windows))]
    pub fn painting(node: Option<&str>) -> Self {
        let render_nodes: Vec<String> = node.into_iter().map(str::to_string).collect();
        Self { vulkan: node.is_some(), vulkan_node: node.map(str::to_string), render_nodes, ..Self::default() }
    }

    /// The D3D11 device on the adapter `node` names, the one that paints the window, for the same reasons as on Linux.
    /// A window that does not say (it paints through Vulkan) leaves the default adapter, and a `node` that names no
    /// adapter is software alone.
    #[cfg(windows)]
    pub fn painting(node: Option<&str>) -> Self {
        match node.map(str::parse) {
            None => Self::system(),
            Some(Ok(adapter)) => Self { d3d11: true, adapter: Some(adapter), ..Self::default() },
            Some(Err(_)) => Self::default(),
        }
    }
}

/// The device of `backend` on `node`'s GPU, or its default device: VA-API names the node itself, Vulkan the GPU whose
/// node it is.
pub(crate) fn device_on(backend: Backend, node: Option<&str>, validation: Validation) -> Option<HwDevice> {
    match (backend, node) {
        #[cfg(target_os = "linux")]
        (Backend::Vulkan, Some(node)) => crate::encode::vulkan_on(node, validation).ok(),
        #[cfg(windows)]
        (Backend::D3d11, node) => {
            let adapter = node.map(str::parse).transpose().ok()?;
            capture::Gpu::open_on(adapter, validation).and_then(|gpu| HwDevice::on_d3d11(&gpu))
        }
        #[cfg(windows)]
        (Backend::D3d12, _) => None,
        _ => HwDevice::open(backend, node, validation),
    }
}

/// Which hardware encoders really open on this machine.
pub fn survey() -> Report {
    survey_on(&Sources::system())
}

/// [`survey`] over chosen sources: with none, the software encoders alone. Each device's open takes the loader's lock
/// as its own, never the walk or an encoder's.
pub fn survey_on(sources: &Sources) -> Report {
    let mut report = Report::default();
    let mut tried_backend = Vec::new();
    let mut opened = Vec::new();
    // A device no encoder takes (the window's D3D12) is not made.
    let plan = sources.devices().into_iter().filter(|(backend, _)| CANDIDATES.iter().any(|(_, b)| b == backend));
    for (backend, on) in plan {
        tried_backend.push(backend);
        let node = on.node().map(str::to_string);
        let validation = on.validation();
        let vendor = on.vendor();
        let device = on.open(backend);
        if device.is_some() && SCREEN_ORDER.contains(&backend) {
            opened.push(Device { backend, node: node.clone(), vendor, validation });
        }
        for (name, candidate_backend) in CANDIDATES {
            // One encoder per name: a later device of the same backend (another render node) is tried only for the
            // names that have not opened yet.
            if candidate_backend != backend || report.encoders.iter().any(|e| e.name == name) {
                continue;
            }
            let outcome = match &device {
                Some(device) => opens(name, backend, device),
                None => Err(Why::DeviceRefused),
            };
            report.unavailable.retain(|u| u.name != name);
            match outcome {
                Ok(multiple) => {
                    let runs = Runs::Gpu(Device { backend, node: node.clone(), vendor, validation });
                    report.encoders.push(Encoder { name, runs, multiple });
                }
                Err(why) => report.unavailable.push(Unavailable { name, path: Path::Hardware(backend), why }),
            }
        }
    }
    // A backend with nothing to try is not silently absent: its encoders are listed as missing a device.
    for (name, backend) in CANDIDATES {
        if !tried_backend.contains(&backend) {
            report.unavailable.push(Unavailable { name, path: Path::Hardware(backend), why: Why::NoDevice });
        }
    }
    let (encoders, unavailable) = software(&opened, opens_software);
    report.encoders.extend(encoders);
    report.unavailable.extend(unavailable);
    // By codec, keeping the candidates' order within one: on Windows that is the order they are offered in.
    report.encoders.sort_by_key(Encoder::codec);
    report
}

/// The software encoders, each once for every device of `devices` that converts a screen for it, or once with none
/// when no device opened; `opens` says whether one opens here.
fn software(devices: &[Device], opens: impl Fn(Name) -> Result<Multiple, Why>) -> (Vec<Encoder>, Vec<Unavailable>) {
    let mut encoders = Vec::new();
    let mut unavailable = Vec::new();
    for name in SOFTWARE {
        match opens(name) {
            Err(why) => unavailable.push(Unavailable { name, path: Path::Software, why }),
            Ok(multiple) if devices.is_empty() => {
                encoders.push(Encoder { name, runs: Runs::Cpu { convert: None }, multiple });
            }
            Ok(multiple) => encoders.extend(devices.iter().map(|device| Encoder {
                name,
                runs: Runs::Cpu { convert: Some(device.clone()) },
                multiple,
            })),
        }
    }
    (encoders, unavailable)
}

/// Opens the software encoder `name` at the rate control a session uses; what its stream is a multiple of.
fn opens_software(name: Name) -> Result<Multiple, Why> {
    let found = name.found().ok_or(Why::NotBuilt)?;
    let format = name.family().software_format().ok_or(Why::EncoderRefused)?;
    let opened = tuning::open(found, name, |target| {
        target.set_pixel_format(format);
        probe_size(target);
        tuning::hold_rate(target, name.family(), 4_000_000, PROBE_RATE, tuning::Buffer::Frame)
    });
    opened.map(|context| measured(name, &context)).ok_or(Why::EncoderRefused)
}

/// Opens the encoder on this device with a frame pool of the size it would encode; what its stream is a multiple of.
fn opens(name: Name, backend: Backend, device: &HwDevice) -> Result<Multiple, Why> {
    let found = name.found().ok_or(Why::NotBuilt)?;
    let hardware_format = backend.pixel_format();
    let pool = nv12_pool(device, backend, Multiple::PROBE).ok_or(Why::FramesRefused)?;
    let opened = tuning::open(found, name, |target| {
        if !target.set_hw_frames(&pool) {
            return false;
        }
        target.set_pixel_format(hardware_format);
        probe_size(target);
        // The rate control a session asks for (its profile is `tuning::open`'s), so a codec is offered only if it opens
        // as it is used.
        tuning::hold_rate(target, name.family(), 4_000_000, PROBE_RATE, tuning::Buffer::Frame)
    });
    opened.map(|context| measured(name, &context)).ok_or(Why::EncoderRefused)
}

/// What the stream of `name`, open at the probe's size as `context`, is a multiple of.
fn measured(name: Name, context: &CodecContext) -> Multiple {
    Multiple::of(name.codec(), context.padded_size())
}

/// The pictures a second the probe opens at.
const PROBE_RATE: i32 = 30;

/// Gives `target` the probe's size, which encodes nothing, and rate.
fn probe_size(target: &mut CodecContext) {
    let (width, height) = Multiple::PROBE;
    target.set_size(width.cast_signed(), height.cast_signed());
    target.set_frame_rate(PROBE_RATE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_carries_no_hevc_nor_vp9() {
        let carried = crate::av::unwanted_codec_names();
        assert!(carried.is_empty(), "codecs nothing sends: {carried:?}");
    }

    #[cfg(not(windows))]
    const NO_SUCH_NODE: &str = "/dev/dri/renderD999";

    /// A device of `backend` on `node`, of an AMD GPU: the vendor every measured row names.
    fn on_amd(backend: Backend, node: Option<&str>) -> Device {
        let node = node.map(str::to_string);
        Device { backend, node, vendor: Some(PciVendor::AMD), validation: Validation::Off }
    }

    /// An encoder of `codec` on a device of `backend`: that backend's first candidate of the codec.
    fn gpu(codec: Codec, backend: Backend) -> Encoder {
        let device = on_amd(backend, None);
        let name = CANDIDATES.iter().find(|(n, b)| n.codec() == codec && *b == backend).map(|(n, _)| *n);
        Encoder {
            name: name.expect("a candidate of the codec on the backend"),
            runs: Runs::Gpu(device),
            multiple: crate::Multiple::EVEN,
        }
    }

    #[test]
    fn a_machine_with_no_device_offers_software_alone_for_memory_and_says_why_for_every_hardware_candidate() {
        let report = survey_on(&Sources::default());
        let memory = |name| Encoder { name, runs: Runs::Cpu { convert: None }, multiple: crate::Multiple::EVEN };
        let software = [memory(Name::X264), memory(Name::SvtAv1)];
        assert_eq!(report.encoders, software, "{:?}", report.unavailable);
        assert_eq!(report.unavailable.len(), CANDIDATES.len());
        assert!(report.unavailable.iter().all(|u| u.why == Why::NoDevice));
        assert_eq!(report.screen_order(Codec::Av1).count(), 0, "with no device to convert on, no screen");
    }

    #[test]
    fn a_software_encoder_is_offered_beside_hardware_of_its_codec_once_per_device() {
        let vulkan = on_amd(Backend::Vulkan, None);
        let vaapi = on_amd(Backend::VaApi, Some("128"));
        let opens = |_: Name| Ok(crate::Multiple::EVEN);
        let (encoders, unavailable) = software(std::slice::from_ref(&vulkan), opens);
        assert!(unavailable.is_empty(), "a hardware encoder that may fail leaves its codec a software successor");
        assert_eq!(
            encoders.iter().map(|e| (e.codec(), e.runs.clone())).collect::<Vec<_>>(),
            [
                (Codec::Av1, Runs::Cpu { convert: Some(vulkan.clone()) }),
                (Codec::H264, Runs::Cpu { convert: Some(vulkan.clone()) })
            ]
        );
        let devices = [vaapi.clone(), vulkan.clone()];
        let (encoders, unavailable) = software(&devices, opens);
        assert!(unavailable.is_empty());
        let converts: Vec<_> = encoders.iter().map(|e| (e.codec(), e.runs.clone())).collect();
        assert_eq!(
            converts,
            [
                (Codec::Av1, Runs::Cpu { convert: Some(vaapi.clone()) }),
                (Codec::Av1, Runs::Cpu { convert: Some(vulkan.clone()) }),
                (Codec::H264, Runs::Cpu { convert: Some(vaapi) }),
                (Codec::H264, Runs::Cpu { convert: Some(vulkan) })
            ]
        );
        assert!(encoders.iter().all(|e| e.path() == Path::Software));
        let (encoders, unavailable) = software(&devices, |_| Err(Why::NotBuilt));
        assert!(encoders.is_empty());
        assert_eq!(unavailable.iter().map(|u| u.why).collect::<Vec<_>>(), [Why::NotBuilt, Why::NotBuilt]);
    }

    #[cfg(not(windows))]
    #[test]
    fn devices_are_tried_render_node_by_node_then_vulkan() {
        let sources = Sources {
            render_nodes: vec!["128".into(), "129".into()],
            vulkan: true,
            d3d11: false,
            ..Sources::default()
        };
        let order: Vec<_> =
            sources.devices().iter().map(|(backend, on)| (*backend, on.node().map(str::to_string))).collect();
        assert_eq!(
            order,
            [(Backend::VaApi, Some("128".into())), (Backend::VaApi, Some("129".into())), (Backend::Vulkan, None)]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn the_painting_gpu_alone_is_tried_and_with_none_no_device_is() {
        let order = |sources: Sources| -> Vec<_> {
            sources.devices().iter().map(|(backend, on)| (*backend, on.node().map(str::to_string))).collect()
        };
        let node = "/dev/dri/renderD129";
        assert_eq!(
            order(Sources::painting(Some(node))),
            [(Backend::VaApi, Some(node.into())), (Backend::Vulkan, Some(node.into()))],
            "VA-API and Vulkan on that GPU, never another's"
        );
        assert_eq!(order(Sources::painting(None)), [], "no GPU paints: software alone");
    }

    #[test]
    fn a_picture_in_memory_tries_hardware_first_and_each_software_encoder_once() {
        let x264 = |convert| Encoder { name: Name::X264, runs: Runs::Cpu { convert }, multiple: crate::Multiple::EVEN };
        let backend = SCREEN_ORDER[0];
        let device = on_amd(backend, None);
        let on_gpu = |codec| gpu(codec, backend);
        let report = Report {
            encoders: vec![x264(Some(device.clone())), on_gpu(Codec::H264), x264(None), on_gpu(Codec::Av1)],
            unavailable: vec![],
        };
        let order: Vec<_> = report.memory_order(Codec::H264).map(Encoder::path).collect();
        assert_eq!(order, [Path::Hardware(backend), Path::Software], "the software encoder once");
        assert_eq!(survey_on(&Sources::default()).memory_order(Codec::H264).count(), 1, "no device: x264 alone");
    }

    #[test]
    fn an_untested_family_is_tried_after_every_measured_one_and_before_software() {
        let backend = SCREEN_ORDER[0];
        let device = on_amd(backend, None);
        let untested =
            Encoder { name: Name::H264Nvenc, runs: Runs::Gpu(device.clone()), multiple: crate::Multiple::EVEN };
        let x264 =
            Encoder { name: Name::X264, runs: Runs::Cpu { convert: Some(device) }, multiple: crate::Multiple::EVEN };
        let measured = gpu(Codec::H264, backend);
        let report = Report { encoders: vec![untested.clone(), x264.clone(), measured.clone()], unavailable: vec![] };
        let tried = [measured, untested, x264];
        assert_eq!(report.screen_order(Codec::H264).cloned().collect::<Vec<_>>(), tried);
        assert_eq!(report.memory_order(Codec::H264).cloned().collect::<Vec<_>>(), tried);
        assert_eq!(tried.map(|e| e.rank()), [Rank::Measured, Rank::Untested, Rank::Software]);
    }

    #[test]
    fn a_measured_family_on_another_vendor_ranks_untested() {
        let on = |vendor| {
            let mut encoder = gpu(Codec::H264, SCREEN_ORDER[0]);
            if let Runs::Gpu(device) = &mut encoder.runs {
                device.vendor = vendor;
            }
            encoder
        };
        assert!(matches!(on(None).name.family().measured(), Measured::On(_)), "a family measured somewhere");
        assert_eq!(on(Some(PciVendor::AMD)).rank(), Rank::Measured);
        for vendor in [Some(PciVendor::INTEL), Some(PciVendor::NVIDIA), Some(PciVendor(0x1af4)), None] {
            assert_eq!(on(vendor).rank(), Rank::Untested, "{vendor:?}");
        }
        let mut intel = on_amd(SCREEN_ORDER[0], None);
        intel.vendor = Some(PciVendor::INTEL);
        let x264 =
            Encoder { name: Name::X264, runs: Runs::Cpu { convert: Some(intel) }, multiple: crate::Multiple::EVEN };
        assert_eq!(x264.rank(), Rank::Software, "software whatever the GPU that converts for it");
    }

    #[cfg(not(windows))]
    #[test]
    fn the_screen_tries_software_av1_after_every_hardware_one() {
        let vulkan = on_amd(Backend::Vulkan, None);
        let vaapi = on_amd(Backend::VaApi, Some("128"));
        let svt = |device: &Device| Encoder {
            name: Name::SvtAv1,
            runs: Runs::Cpu { convert: Some(device.clone()) },
            multiple: crate::Multiple::EVEN,
        };
        let report = Report {
            encoders: vec![
                gpu(Codec::Av1, Backend::Vulkan),
                svt(&vulkan),
                gpu(Codec::Av1, Backend::VaApi),
                svt(&vaapi),
            ],
            unavailable: vec![],
        };
        let paths: Vec<_> = report.screen_order(Codec::Av1).map(Encoder::path).collect();
        assert_eq!(
            paths,
            [Path::Hardware(Backend::Vulkan), Path::Hardware(Backend::VaApi), Path::Software, Path::Software]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn the_screen_tries_vulkan_before_its_failover() {
        let report = Report {
            encoders: vec![
                gpu(Codec::H264, Backend::VaApi),
                gpu(Codec::Av1, Backend::Vulkan),
                gpu(Codec::H264, Backend::Vulkan),
            ],
            unavailable: vec![],
        };
        let order: Vec<_> = report.screen_order(Codec::H264).filter_map(|e| e.device()).map(|d| d.backend).collect();
        assert_eq!(order, [Backend::Vulkan, Backend::VaApi]);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_render_node_that_does_not_open_is_a_refusal_not_a_guess() {
        let report = survey_on(&Sources { render_nodes: vec![NO_SUCH_NODE.to_string()], ..Sources::default() });
        assert!(report.encoders.iter().all(|e| e.runs == Runs::Cpu { convert: None }), "{:?}", report.encoders);
        let vaapi: Vec<_> = report.unavailable.iter().filter(|u| u.path == Path::Hardware(Backend::VaApi)).collect();
        assert_eq!(vaapi.len(), 2);
        assert!(vaapi.iter().all(|u| u.why == Why::DeviceRefused));
        assert!(
            report
                .unavailable
                .iter()
                .filter(|u| u.path == Path::Hardware(Backend::Vulkan))
                .all(|u| u.why == Why::NoDevice)
        );
    }

    #[test]
    fn the_report_lists_each_encoder_once_and_only_from_what_opened() {
        let report = survey();
        for (n, encoder) in report.encoders.iter().enumerate() {
            let listed = match &encoder.runs {
                Runs::Gpu(device) => CANDIDATES.contains(&(encoder.name, device.backend)),
                Runs::Cpu { .. } => SOFTWARE.contains(&encoder.name),
            };
            assert!(listed, "{encoder}");
            assert!(report.encoders[n + 1..].iter().all(|other| other != encoder));
            assert!(report.unavailable.iter().all(|u| u.name != encoder.name));
        }
        let software = report.encoders.iter().filter(|e| e.path() == Path::Software);
        let names = |path| report.encoders.iter().filter(|e| e.path() == path).count();
        let hardware = report.encoders.len() - names(Path::Software);
        let software_names = software.map(|e| e.name).collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(hardware + software_names + report.unavailable.len(), CANDIDATES.len() + SOFTWARE.len());
    }

    #[test]
    fn what_this_machine_can_encode_is_printed_for_the_record() {
        let report = survey();
        let opened: Vec<String> = report.encoders.iter().map(ToString::to_string).collect();
        let refused: Vec<String> = report.unavailable.iter().map(|u| format!("{} ({})", u.name, u.why)).collect();
        eprintln!("opened: {}", if opened.is_empty() { "none".into() } else { opened.join(", ") });
        eprintln!("did not: {}", if refused.is_empty() { "none".into() } else { refused.join(", ") });
    }
}
