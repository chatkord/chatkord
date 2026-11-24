//! How every encoder is opened for a live share: held to its rate frame by frame, at the rate control that keeps its
//! key frames, with a key frame asked for made an IDR, each picture handed out of the send that took it, H.264 as
//! High. One row a family, each naming where it was measured.

use crate::av::{CodecContext, CodecRef, PixelFormat};
use crate::probe::{Codec, PciVendor};
use std::ffi::{CStr, c_int};
use std::fmt;

/// Options an encoder is opened with beyond the rate, as FFmpeg names them.
type Preset = &'static [(&'static CStr, &'static CStr)];

/// An FFmpeg encoder this client opens; what it codes and its family follow from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, report::Public)]
pub enum Name {
    /// `h264_vaapi`.
    H264Vaapi,
    /// `av1_vaapi`.
    Av1Vaapi,
    /// `h264_vulkan`.
    H264Vulkan,
    /// `av1_vulkan`.
    Av1Vulkan,
    /// `h264_amf`.
    H264Amf,
    /// `av1_amf`.
    Av1Amf,
    /// `h264_nvenc`.
    H264Nvenc,
    /// `av1_nvenc`.
    Av1Nvenc,
    /// `h264_qsv`.
    H264Qsv,
    /// `av1_qsv`.
    Av1Qsv,
    /// `libx264`.
    X264,
    /// `libsvtav1`.
    SvtAv1,
}

impl Name {
    /// What it encodes.
    pub const fn codec(self) -> Codec {
        match self {
            Self::H264Vaapi | Self::H264Vulkan | Self::H264Amf | Self::H264Nvenc | Self::H264Qsv | Self::X264 => {
                Codec::H264
            }
            Self::Av1Vaapi | Self::Av1Vulkan | Self::Av1Amf | Self::Av1Nvenc | Self::Av1Qsv | Self::SvtAv1 => {
                Codec::Av1
            }
        }
    }

    /// Whose tuning it is opened with.
    pub const fn family(self) -> Family {
        match self {
            Self::H264Vaapi | Self::Av1Vaapi => Family::Vaapi,
            Self::H264Vulkan | Self::Av1Vulkan => Family::Vulkan,
            Self::H264Amf | Self::Av1Amf => Family::Amf,
            Self::H264Nvenc | Self::Av1Nvenc => Family::Nvenc,
            Self::H264Qsv | Self::Av1Qsv => Family::Qsv,
            Self::X264 => Family::X264,
            Self::SvtAv1 => Family::SvtAv1,
        }
    }

    /// FFmpeg's name for it.
    pub(crate) const fn ffmpeg(self) -> &'static CStr {
        match self {
            Self::H264Vaapi => c"h264_vaapi",
            Self::Av1Vaapi => c"av1_vaapi",
            Self::H264Vulkan => c"h264_vulkan",
            Self::Av1Vulkan => c"av1_vulkan",
            Self::H264Amf => c"h264_amf",
            Self::Av1Amf => c"av1_amf",
            Self::H264Nvenc => c"h264_nvenc",
            Self::Av1Nvenc => c"av1_nvenc",
            Self::H264Qsv => c"h264_qsv",
            Self::Av1Qsv => c"av1_qsv",
            Self::X264 => c"libx264",
            Self::SvtAv1 => c"libsvtav1",
        }
    }

    /// FFmpeg's name for it, as text.
    pub fn as_str(self) -> &'static str {
        self.ffmpeg().to_str().unwrap_or_default()
    }

    /// The encoder in this FFmpeg; none when it was built without it.
    pub(crate) fn found(self) -> Option<CodecRef> {
        CodecRef::encoder(self.ffmpeg())
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The encoders that share one tuning: a driver's API, or a software library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// VA-API's, on Linux.
    Vaapi,
    /// Vulkan Video's, on Linux.
    Vulkan,
    /// AMD's AMF, on Windows.
    Amf,
    /// NVIDIA's NVENC, on Windows.
    Nvenc,
    /// Intel's Quick Sync, on Windows.
    Qsv,
    /// x264, on the processor.
    X264,
    /// SVT-AV1, on the processor.
    SvtAv1,
}

/// Where a family's row was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measured {
    /// On these GPUs of the test setup, each by its vendor and the machine and GPU that ran it: the row holds for that
    /// vendor's driver alone.
    On(&'static [(PciVendor, &'static str)]),
    /// On these machines' processors, each with the processor that ran it.
    OnProcessor(&'static [&'static str]),
    /// Nowhere: its row is the vendor's documented options, never seen to hold.
    Untested,
}

/// Whether an H.264 encoder is told the level its stream needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// It states the level its stream needs.
    Own,
    /// It picks one from the size alone, too low at 4K60, so it is told.
    Told,
}

/// How a running encoder takes a new rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NewRate {
    /// Between frames in its open context, with no new stream and no key frame; a larger buffer starts full: VA-API
    /// and Vulkan are sent its fullness with it (`patches/ffmpeg`); AMF's key frame stays near 18 to 23 frames' bits.
    /// On AMD's VCN through VA-API and Vulkan a key frame's size is the driver's, from the bits a frame gets,
    /// whatever buffer or first QP it is given.
    InPlace,
    /// As [`NewRate::InPlace`], but a larger buffer fills only at the rate, so a key frame's budget needs a new encoder,
    /// which starts full: x264 reconfigures its buffer's size and keeps its fullness.
    InPlaceUnfilled,
    /// By a reopen, on a key frame.
    Reopens,
}

/// How a family is opened and held to its rate.
#[derive(Clone, Copy, Debug)]
struct Tuning {
    /// Its presets, tried in turn when the driver refuses one.
    presets: &'static [Preset],
    /// What it is told beside the low-delay flag, so a picture's unit comes out of the send that took it.
    in_send: Preset,
    /// What its H.264 encoder is told beside the context's profile, High, which the router offers.
    high: Preset,
    /// Whether its H.264 encoder is told its level.
    level: Level,
    /// How it is held to its rate.
    control: Control,
    /// The option that holds a picture's unit to the buffer it is held to, where the buffer alone does not.
    unit_bound: Option<&'static CStr>,
    /// The option that makes a key frame asked for an IDR, which a receiver that joined mid-stream starts from.
    idr: Option<&'static CStr>,
    /// How a running encoder takes a new rate.
    new_rate: NewRate,
    /// Where the row was measured.
    measured: Measured,
}

impl Family {
    /// The format a software encoder of this family is given its pictures in: NV12 for x264, which takes a picture in
    /// memory as it comes, and planar 4:2:0 for SVT-AV1, which reads that. None for a hardware family, which takes its
    /// device's frames.
    pub(crate) const fn software_format(self) -> Option<PixelFormat> {
        match self {
            Self::X264 => Some(PixelFormat::Nv12),
            Self::SvtAv1 => Some(PixelFormat::Yuv420p),
            Self::Vaapi | Self::Vulkan | Self::Amf | Self::Nvenc | Self::Qsv => None,
        }
    }

    /// Its row.
    const fn tuning(self) -> Tuning {
        const NONE: &[Preset] = &[&[]];
        match self {
            // In the send, a 4K VA-API call on Machine B took 17 ms against 10.3 and lost 60 a second, so VA-API and
            // Vulkan keep FFmpeg's two pictures in flight and the GPU's encode overlaps the next picture.
            Self::Vaapi => Tuning {
                presets: NONE,
                in_send: &[],
                high: &[],
                level: Level::Own,
                control: Control::Constant(c"CBR"),
                unit_bound: None,
                idr: None,
                new_rate: NewRate::InPlace,
                measured: Measured::On(&[
                    (PciVendor::AMD, "Machine A, RX 9060 XT"),
                    (PciVendor::AMD, "Machine B, Renoir"),
                ]),
            },
            Self::Vulkan => Tuning {
                presets: NONE,
                in_send: &[],
                high: &[],
                level: Level::Own,
                control: Control::Constant(c"cbr"),
                unit_bound: None,
                idr: None,
                new_rate: NewRate::InPlace,
                measured: Measured::On(&[(PciVendor::AMD, "Machine A, RX 9060 XT")]),
            },
            // Its default usage ignores the peak (30 Mbit/s over half a second against 4 on the RX 9060 XT) and some
            // drivers refuse ultra-low latency (AMF issue 410). Untold, it picks level 5.1 for 4K60, which needs 5.2,
            // and writes non-IDR I slices for a key frame asked for.
            Self::Amf => Tuning {
                presets: &[
                    &[(c"usage", c"ultralowlatency"), (c"rc", c"vbr_latency")],
                    &[(c"usage", c"lowlatency"), (c"rc", c"vbr_latency")],
                ],
                in_send: &[(c"async_depth", c"1")],
                high: &[],
                level: Level::Told,
                control: Control::Variable,
                // Its buffer alone let a key frame asked of a 1080p page at 0.045 bits per pixel write 27 frames' bits
                // on the RX 9060 XT; bound, 18 at 30 frames a second and 23 at 60, its first IDR 25 at both: AMF sets
                // that one's quality from the buffer at open, and no rate option or intra refresh moved it unblurred.
                unit_bound: Some(c"max_au_size"),
                idr: Some(c"forced_idr"),
                new_rate: NewRate::InPlace,
                measured: Measured::On(&[(PciVendor::AMD, "Machine A, RX 9060 XT")]),
            },
            // NVENC and Quick Sync keep 3 and 4 pictures in flight by default and ignore the context's profile.
            Self::Nvenc => Tuning {
                presets: NONE,
                in_send: &[(c"delay", c"0")],
                high: &[(c"profile", c"high")],
                level: Level::Own,
                control: Control::Variable,
                unit_bound: None,
                idr: Some(c"forced-idr"),
                new_rate: NewRate::Reopens,
                measured: Measured::Untested,
            },
            Self::Qsv => Tuning {
                presets: NONE,
                in_send: &[(c"async_depth", c"1")],
                high: &[(c"profile", c"high")],
                level: Level::Own,
                control: Control::Variable,
                unit_bound: None,
                idr: Some(c"forced_idr"),
                new_rate: NewRate::Reopens,
                measured: Measured::Untested,
            },
            // Sunshine's: the fastest preset that keeps a screen sharp, with no look-ahead, B-frames or frame threads.
            Self::X264 => Tuning {
                presets: &[&[(c"preset", c"superfast"), (c"tune", c"zerolatency")]],
                in_send: &[],
                high: &[],
                level: Level::Own,
                control: Control::Variable,
                unit_bound: None,
                idr: Some(c"forced-idr"),
                new_rate: NewRate::InPlaceUnfilled,
                measured: Measured::OnProcessor(&["Machine A, Ryzen 5 5600", "Machine B, Renoir"]),
            },
            // Real-time coding is low delay, waits for its unit, and makes a key frame where one is asked.
            Self::SvtAv1 => Tuning {
                presets: &[&[(c"preset", SVT_AV1_PRESET), (c"svtav1-params", c"rtc=1")]],
                in_send: &[],
                high: &[],
                level: Level::Own,
                control: Control::Matched,
                unit_bound: None,
                idr: None,
                new_rate: NewRate::Reopens,
                measured: Measured::OnProcessor(&["Machine A, Ryzen 5 5600", "Machine B, Renoir"]),
            },
        }
    }

    /// Where its row was measured.
    pub const fn measured(self) -> Measured {
        self.tuning().measured
    }

    /// Whether it pads its pictures with filler to keep a constant rate, which is taken out after.
    pub(crate) const fn pads(self) -> bool {
        matches!(self.tuning().control, Control::Constant(_))
    }

    /// Whether it takes a new rate between frames in its open context, with no new stream and no key frame: x264
    /// reconfigures itself, Vulkan sets the session's rate control anew, VA-API sends it with the next picture and AMF
    /// sets it before the next submit (`patches/ffmpeg`). Every other reopens on a key frame.
    pub(crate) const fn takes_rate_in_place(self) -> bool {
        matches!(self.tuning().new_rate, NewRate::InPlace | NewRate::InPlaceUnfilled)
    }

    /// Whether a key frame asked of its running encoder is given its budget there, the buffer raised in place and
    /// full; for every other family that takes a rate in place, the key frame is a new encoder's first.
    pub(crate) const fn budgets_in_place(self) -> bool {
        matches!(self.tuning().new_rate, NewRate::InPlace)
    }
}

/// The lowest H.264 level_idc, from 3.1 up, whose macroblocks a second and a picture (H.264's Table A-1) hold
/// `width` x `height` at `per_second`; none past level 6.2.
pub fn h264_level(width: u32, height: u32, per_second: u32) -> Option<u8> {
    let picture = u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16));
    let levels = [
        (31, 108_000, 3_600),
        (32, 216_000, 5_120),
        (40, 245_760, 8_192),
        (42, 522_240, 8_704),
        (50, 589_824, 22_080),
        (51, 983_040, 36_864),
        (52, 2_073_600, 36_864),
        (60, 4_177_920, 139_264),
        (61, 8_355_840, 139_264),
        (62, 16_711_680, 139_264),
    ];
    levels
        .iter()
        .find(|(_, a_second, most)| picture * u64::from(per_second) <= *a_second && picture <= *most)
        .map(|(level, _, _)| *level)
}

/// The shortest rate buffer SVT-AV1 takes, in milliseconds: it refuses one frame at 60 a second (16 ms).
const SVT_AV1_SHORTEST_BUFFER_MS: i64 = 20;

/// SVT-AV1's speed: the fastest real-time preset that keeps its screen-content tools (palette, IntraBC), which it turns
/// off from 9. On the Ryzen 5 5600 they cost 1080p60 17 % of the machine against 12 % at 10, and gave one-pixel lines
/// 44.9 dB at 1440p against 41.4; a share's load steps it down where a processor cannot keep up.
const SVT_AV1_PRESET: &CStr = c"8";

/// Opens the encoder `name`, which is `codec`, with the first of its family's presets the driver takes. `fill` sets
/// what the caller decides (pool, size, rate) on each fresh context and is false when it cannot.
pub(crate) fn open(codec: CodecRef, name: Name, fill: impl Fn(&mut CodecContext) -> bool) -> Option<CodecContext> {
    let tuning = name.family().tuning();
    tuning.presets.iter().find_map(|preset| {
        let mut context = CodecContext::new(Some(codec))?;
        if !fill(&mut context) {
            return None;
        }
        context.set_low_delay();
        let h264 = context.is_h264();
        if h264 {
            context.set_h264_high();
        }
        if h264 && tuning.level == Level::Told {
            let (width, height) = context.size();
            if let Some(level) = h264_level(width, height, context.frame_rate()) {
                context.set_level(c_int::from(level));
            }
        }
        let profile = if h264 { tuning.high } else { &[] };
        let idr = tuning.idr.map(|option| (option, c"1"));
        let told = preset
            .iter()
            .chain(tuning.in_send)
            .chain(profile)
            .chain(idr.as_ref())
            .all(|(option, value)| context.set_option(option, value));
        (told && context.open().is_ok()).then_some(context)
    })
}

/// The most of its rate a key frame the session makes may take, in milliseconds: a page of text needs many frames'
/// bits to come sharp. At 300 a page asked for at 1080p60 and 0.045 bits per pixel came to 42 dB on the RX 9060 XT's
/// VA-API and Vulkan against 27 at one frame, 47 against 38 on its AMF, and reaches a viewer up to as much later.
pub const KEY_FRAME_BUDGET_MS: i64 = 300;

/// The rate buffer a picture is encoded against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Buffer {
    /// One frame of the rate, as every picture but a key frame is held to.
    Frame,
    /// [`KEY_FRAME_BUDGET_MS`] of the rate, or one frame where that is longer.
    KeyFrame,
}

impl Buffer {
    /// The buffer a picture of an encoder of `family` is held to: a key frame its budget where the running encoder
    /// takes a new buffer between frames, every other picture one frame.
    pub(crate) const fn for_picture(family: Family, key_frame: bool) -> Self {
        if key_frame && family.takes_rate_in_place() { Self::KeyFrame } else { Self::Frame }
    }
}

/// How a driver keeps an encoder to its rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    /// A constant rate, set by this option: VA-API and Vulkan keep key frames to the buffer only so (on the RX 9060 XT
    /// a variable rate let the first one write 26 frames' worth). The filler it pads with is taken out after.
    Constant(&'static CStr),
    /// A variable rate whose peak is the target: AMF's key frames still pass the budget (25 frames' bits for the first,
    /// 18 to 23 asked, against 9 and 18 allowed).
    Variable,
    /// The average set to the peak, which is how SVT-AV1's wrapper is told a constant rate.
    Matched,
}

/// Holds an encoder of `family` to `bits_per_second` over every frame: the peak is the target and the buffer one frame
/// (SVT-AV1's no shorter than it takes), starting full (the drivers' three quarters cost Machine B's first key frame
/// 2 dB of luma PSNR). A variable rate's average sits one bit under the peak, as drivers need to choose it; AMF is also
/// told no unit may pass the buffer ([`bound_unit`]). False when the driver refuses or it does not fit. `context` is
/// allocated for an encoder of `family`, not yet opened.
pub(crate) fn hold_rate(
    context: &mut CodecContext,
    family: Family,
    bits_per_second: i64,
    frames_per_second: c_int,
    buffer: Buffer,
) -> bool {
    let control = family.tuning().control;
    if !write_rate(context, control, bits_per_second, frames_per_second, buffer) || !bound_unit(context, family) {
        return false;
    }
    match control {
        Control::Constant(mode) => context.set_option(c"rc_mode", mode),
        Control::Variable | Control::Matched => true,
    }
}

/// Holds the open encoder `context` of `family`, which [`Family::takes_rate_in_place`], to `bits_per_second` from its
/// next frame, as [`hold_rate`] held it at its open. False when the rate does not fit.
pub(crate) fn set_rate_in_place(
    context: &mut CodecContext,
    family: Family,
    bits_per_second: i64,
    frames_per_second: c_int,
    buffer: Buffer,
) -> bool {
    write_rate(context, family.tuning().control, bits_per_second, frames_per_second, buffer)
        && bound_unit(context, family)
}

/// Holds each unit of an encoder of `family` to the buffer `context` holds its pictures to, where its row names an
/// option for it. False when the encoder refuses it.
fn bound_unit(context: &mut CodecContext, family: Family) -> bool {
    family.tuning().unit_bound.is_none_or(|option| {
        let bits = i64::from(context.buffer_size());
        context.set_option_int(option, bits)
    })
}

/// The rate fields [`hold_rate`] describes, written into `context`. False when the buffer does not fit.
fn write_rate(
    context: &mut CodecContext,
    control: Control,
    bits_per_second: i64,
    frames_per_second: c_int,
    buffer: Buffer,
) -> bool {
    let frame = bits_per_second / i64::from(frames_per_second.max(1));
    let shortest = match (control, buffer) {
        (_, Buffer::KeyFrame) => bits_per_second * KEY_FRAME_BUDGET_MS / 1000,
        // Rounded up: the wrapper takes the buffer in whole milliseconds, rounded down.
        (Control::Matched, Buffer::Frame) => (bits_per_second * SVT_AV1_SHORTEST_BUFFER_MS + 999) / 1000,
        (Control::Constant(_) | Control::Variable, Buffer::Frame) => 0,
    };
    let Ok(buffer) = c_int::try_from(frame.max(shortest)) else {
        return false;
    };
    let average = match control {
        Control::Constant(_) | Control::Matched => bits_per_second,
        Control::Variable => (bits_per_second - 1).max(1),
    };
    context.set_rates(average, bits_per_second, buffer);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_NAME: [Name; 12] = [
        Name::H264Vaapi,
        Name::Av1Vaapi,
        Name::H264Vulkan,
        Name::Av1Vulkan,
        Name::H264Amf,
        Name::Av1Amf,
        Name::H264Nvenc,
        Name::Av1Nvenc,
        Name::H264Qsv,
        Name::Av1Qsv,
        Name::X264,
        Name::SvtAv1,
    ];

    #[test]
    fn every_name_is_ffmpeg_s_and_names_its_codec() {
        for name in EVERY_NAME {
            let text = name.as_str();
            let codec = if text.starts_with("av1_") || text == "libsvtav1" { Codec::Av1 } else { Codec::H264 };
            assert_eq!(name.codec(), codec, "{text}");
            assert!(!text.is_empty() && text.is_ascii(), "{name:?}");
        }
    }

    #[test]
    fn every_family_s_row_names_where_it_was_measured_or_says_it_was_not() {
        for name in EVERY_NAME {
            let named = |machine: &str| machine.starts_with("Machine ") && machine.contains(", ");
            let software = name.family().software_format().is_some();
            match name.family().measured() {
                Measured::On(gpus) => {
                    assert!(!gpus.is_empty() && !software, "{name}");
                    assert!(gpus.iter().all(|(_, machine)| named(machine)), "{name}");
                }
                Measured::OnProcessor(machines) => {
                    assert!(!machines.is_empty() && software, "{name}: a processor's row is a software family's");
                    assert!(machines.iter().all(|machine| named(machine)), "{name}");
                }
                Measured::Untested => {
                    assert!(matches!(name.family(), Family::Nvenc | Family::Qsv), "{name} was measured");
                }
            }
        }
    }

    #[test]
    fn every_amf_encoder_tries_ultra_low_latency_first_and_the_others_take_no_options() {
        let tried = Family::Amf.tuning().presets;
        assert_eq!(tried.len(), 2);
        assert!(tried[0].contains(&(c"usage", c"ultralowlatency")));
        for family in [Family::Vaapi, Family::Vulkan, Family::Nvenc, Family::Qsv] {
            assert_eq!(family.tuning().presets, &[&[] as Preset], "{family:?}");
        }
    }

    #[test]
    fn amf_quick_sync_and_nvenc_hold_no_picture_past_its_send_and_the_others_are_told_nothing() {
        for family in [Family::Amf, Family::Qsv] {
            assert_eq!(family.tuning().in_send, &[(c"async_depth", c"1")], "{family:?}");
        }
        assert_eq!(Family::Nvenc.tuning().in_send, &[(c"delay", c"0")]);
        for family in [Family::SvtAv1, Family::Vaapi, Family::Vulkan, Family::X264] {
            assert!(family.tuning().in_send.is_empty(), "{family:?}");
        }
    }

    #[test]
    fn vaapi_and_vulkan_run_at_a_constant_rate_with_the_target_as_their_average_and_amf_at_a_variable_one() {
        assert_eq!(Family::Vaapi.tuning().control, Control::Constant(c"CBR"));
        assert_eq!(Family::Vulkan.tuning().control, Control::Constant(c"cbr"));
        assert!(Family::Vaapi.pads() && Family::Vulkan.pads());
        for family in [Family::Amf, Family::Nvenc, Family::Qsv, Family::X264] {
            assert_eq!(family.tuning().control, Control::Variable, "{family:?}");
            assert!(!family.pads(), "{family:?}");
        }
        // A variable rate sets no option, and NVENC bounds no unit, so a context for no codec in particular will do.
        let held = held_by(None, Family::Nvenc, 30);
        assert_eq!(held, (3_999_999, 4_000_000, 133_333));
        #[cfg(target_os = "linux")]
        {
            let codec = Name::H264Vaapi.found().expect("this FFmpeg has h264_vaapi");
            let held = held_by(Some(codec), Family::Vaapi, 30);
            assert_eq!(held, (4_000_000, 4_000_000, 133_333));
        }
    }

    #[test]
    fn svt_av1_runs_at_a_constant_rate_with_a_buffer_it_takes_at_60_a_second() {
        assert_eq!(Family::SvtAv1.tuning().control, Control::Matched);
        let codec = Name::SvtAv1.found().expect("this FFmpeg has libsvtav1");
        let held = held_by(Some(codec), Family::SvtAv1, 60);
        assert_eq!(held, (4_000_000, 4_000_000, 80_000), "the average is the peak, the buffer 20 ms, not 16");
        let held = held_by(Some(codec), Family::SvtAv1, 30);
        assert_eq!(held, (4_000_000, 4_000_000, 133_333), "one frame where that is longer");
        let (average, _, buffer) = held_by_at(Some(codec), Family::SvtAv1, 19_906_560, 60);
        assert!(i64::from(buffer) * 1000 / average >= 20, "the wrapper's whole milliseconds are still 20: {buffer}");
    }

    #[test]
    fn a_key_frame_is_given_its_budget_where_the_running_encoder_takes_a_buffer_and_every_other_picture_one_frame() {
        for family in [Family::Vaapi, Family::Vulkan, Family::Amf, Family::X264] {
            assert_eq!(Buffer::for_picture(family, true), Buffer::KeyFrame, "{family:?}");
            assert_eq!(Buffer::for_picture(family, false), Buffer::Frame, "{family:?}");
        }
        for family in [Family::Nvenc, Family::Qsv, Family::SvtAv1] {
            assert_eq!(Buffer::for_picture(family, true), Buffer::Frame, "{family:?} reopens for a key frame's rate");
        }
        let mut context = CodecContext::new(None).expect("a context for no codec in particular");
        assert!(write_rate(&mut context, Control::Variable, 6_000_000, 60, Buffer::KeyFrame));
        let rates = context.rates();
        assert_eq!((rates.buffer, rates.initial), (1_800_000, 1_800_000), "300 ms, full");
        assert!(write_rate(&mut context, Control::Variable, 6_000_000, 60, Buffer::Frame));
        assert_eq!(context.buffer_size(), 100_000, "one frame again");
    }

    #[test]
    fn vaapi_vulkan_and_amf_budget_a_key_frame_in_place_and_x264_by_a_new_encoder() {
        for family in [Family::Vaapi, Family::Vulkan, Family::Amf] {
            assert!(family.budgets_in_place(), "{family:?}");
        }
        assert!(Family::X264.takes_rate_in_place() && !Family::X264.budgets_in_place());
    }

    #[test]
    fn x264_vulkan_vaapi_and_amf_take_a_new_rate_in_place_and_the_others_reopen() {
        for family in [Family::X264, Family::Vulkan, Family::Vaapi, Family::Amf] {
            assert!(family.takes_rate_in_place(), "{family:?}");
        }
        for family in [Family::Nvenc, Family::Qsv, Family::SvtAv1] {
            assert!(!family.takes_rate_in_place(), "{family:?}");
        }
    }

    #[test]
    fn amf_holds_each_unit_to_the_buffer_its_picture_is_held_to_and_no_other_family_is_told_to() {
        for family in [Family::Vaapi, Family::Vulkan, Family::Nvenc, Family::Qsv, Family::X264, Family::SvtAv1] {
            assert_eq!(family.tuning().unit_bound, None, "{family:?}");
        }
        #[cfg(windows)]
        for name in [Name::H264Amf, Name::Av1Amf] {
            let codec = name.found().expect("this FFmpeg has AMF's encoders");
            let mut context = CodecContext::new(Some(codec)).expect("a context, never opened");
            for (buffer, bits) in [(Buffer::KeyFrame, 1_800_000), (Buffer::Frame, 100_000)] {
                assert!(hold_rate(&mut context, Family::Amf, 6_000_000, 60, buffer), "{name} takes {buffer:?}");
                assert_eq!(
                    context.option_int(c"max_au_size"),
                    Some(bits),
                    "{name}: a unit is held to its {buffer:?} buffer"
                );
            }
        }
    }

    /// The average, the peak and the buffer `hold_rate` sets on a fresh context for `codec`, of `family`, at
    /// `frames_per_second`.
    fn held_by(codec: Option<CodecRef>, family: Family, frames_per_second: c_int) -> (i64, i64, c_int) {
        held_by_at(codec, family, 4_000_000, frames_per_second)
    }

    /// [`held_by`] at `bits_per_second`.
    fn held_by_at(
        codec: Option<CodecRef>,
        family: Family,
        bits_per_second: i64,
        frames_per_second: c_int,
    ) -> (i64, i64, c_int) {
        let mut context = CodecContext::new(codec).expect("a context");
        let held = hold_rate(&mut context, family, bits_per_second, frames_per_second, Buffer::Frame);
        assert!(held, "{family:?} takes its rate control");
        let rates = context.rates();
        (rates.average, rates.peak, rates.buffer)
    }
}
