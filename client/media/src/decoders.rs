//! What a hardware device's driver says it decodes, asked before a decoder opens on it: FFmpeg opens a hardware
//! decoder on any device of its type and fails only at the first unit.

use crate::av::HwDevice;
use crate::probe::{Backend, Codec};

/// Whether the driver under `device`, a hardware device context, lists a decoder for `codec`.
pub(crate) fn decodes(device: &HwDevice, codec: Codec) -> bool {
    match device.backend() {
        #[cfg(target_os = "linux")]
        Some(Backend::VaApi) => device.vaapi_display().is_some_and(|display| va::decodes(display.as_ptr(), codec)),
        #[cfg(target_os = "linux")]
        Some(Backend::Vulkan) => device.vulkan_enabled(vulkan_extension(codec)),
        #[cfg(windows)]
        Some(Backend::D3d11) => device.gpu().is_some_and(|gpu| d3d::d3d11(gpu.device(), codec)),
        #[cfg(windows)]
        Some(Backend::D3d12) => device.d3d12().is_some_and(|d3d12| d3d::d3d12(&d3d12, codec)),
        _ => false,
    }
}

/// The Vulkan device extension FFmpeg enables, where the device has it, to decode `codec`.
#[cfg(target_os = "linux")]
fn vulkan_extension(codec: Codec) -> &'static std::ffi::CStr {
    match codec {
        Codec::H264 => c"VK_KHR_video_decode_h264",
        Codec::Av1 => c"VK_KHR_video_decode_av1",
    }
}

#[cfg(target_os = "linux")]
mod va {
    use crate::probe::Codec;
    use std::ffi::{c_int, c_void};

    // va.h's `VAProfile` values; H.264's three cover every stream a sharer sends.
    const H264_MAIN: c_int = 6;
    const H264_HIGH: c_int = 7;
    const H264_CONSTRAINED_BASELINE: c_int = 13;
    const AV1_PROFILE0: c_int = 32;
    // va.h's `VAEntrypointVLD`, a decoder.
    const VLD: c_int = 1;
    const SUCCESS: c_int = 0;

    fn profiles(codec: Codec) -> &'static [c_int] {
        match codec {
            Codec::H264 => &[H264_HIGH, H264_MAIN, H264_CONSTRAINED_BASELINE],
            Codec::Av1 => &[AV1_PROFILE0],
        }
    }

    /// Whether the driver behind `display` has a decoding entrypoint for one of `codec`'s profiles.
    pub(super) fn decodes(display: *mut c_void, codec: Codec) -> bool {
        // SAFETY: `display` is the initialised display of a live VA-API device context.
        let most = usize::try_from(unsafe { va_max_num_profiles(display) }).unwrap_or(0);
        let mut listed = vec![0; most];
        let mut count = 0;
        // SAFETY: `listed` holds the most profiles the display says it lists, and `count` is a valid out-pointer.
        if unsafe { va_query_config_profiles(display, listed.as_mut_ptr(), &raw mut count) } != SUCCESS {
            return false;
        }
        listed.truncate(usize::try_from(count).unwrap_or(0));
        // SAFETY: as above.
        let most = usize::try_from(unsafe { va_max_num_entrypoints(display) }).unwrap_or(0);
        profiles(codec).iter().filter(|wanted| listed.contains(wanted)).any(|&profile| {
            let mut entrypoints = vec![0; most];
            let mut count = 0;
            // SAFETY: `entrypoints` holds the most entrypoints the display says a profile has; `count` is valid.
            let status =
                unsafe { va_query_config_entrypoints(display, profile, entrypoints.as_mut_ptr(), &raw mut count) };
            entrypoints.truncate(usize::try_from(count).unwrap_or(0));
            status == SUCCESS && entrypoints.contains(&VLD)
        })
    }

    // va.h's declarations: `VAProfile` and `VAEntrypoint` are C enums, ints.
    unsafe extern "C" {
        #[link_name = "vaMaxNumProfiles"]
        fn va_max_num_profiles(display: *mut c_void) -> c_int;
        #[link_name = "vaMaxNumEntrypoints"]
        fn va_max_num_entrypoints(display: *mut c_void) -> c_int;
        #[link_name = "vaQueryConfigProfiles"]
        fn va_query_config_profiles(display: *mut c_void, profiles: *mut c_int, count: *mut c_int) -> c_int;
        #[link_name = "vaQueryConfigEntrypoints"]
        fn va_query_config_entrypoints(
            display: *mut c_void,
            profile: c_int,
            entrypoints: *mut c_int,
            count: *mut c_int,
        ) -> c_int;
    }
}

#[cfg(windows)]
mod d3d {
    use crate::probe::Codec;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0, D3D11_DECODER_PROFILE_H264_VLD_NOFGT, ID3D11Device, ID3D11VideoDevice,
    };
    use windows::Win32::Graphics::Direct3D12::ID3D12Device;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
    use windows::Win32::Media::MediaFoundation::{
        D3D12_FEATURE_DATA_VIDEO_DECODE_PROFILE_COUNT, D3D12_FEATURE_DATA_VIDEO_DECODE_PROFILES,
        D3D12_FEATURE_VIDEO_DECODE_PROFILE_COUNT, D3D12_FEATURE_VIDEO_DECODE_PROFILES,
        D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0, D3D12_VIDEO_DECODE_PROFILE_H264, ID3D12VideoDevice,
    };
    use windows::core::{GUID, Interface};

    /// Whether the D3D11 device's video device decodes `codec` into NV12, the format FFmpeg's decoder asks for.
    pub(super) fn d3d11(device: &ID3D11Device, codec: Codec) -> bool {
        let profile = match codec {
            Codec::H264 => D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
            Codec::Av1 => D3D11_DECODER_PROFILE_AV1_VLD_PROFILE0,
        };
        let Ok(video) = device.cast::<ID3D11VideoDevice>() else { return false };
        // SAFETY: `profile` is a GUID this frame holds for the call.
        unsafe { video.CheckVideoDecoderFormat(&raw const profile, DXGI_FORMAT_NV12) }.is_ok_and(|yes| yes.as_bool())
    }

    /// Whether the D3D12 device lists a decoder profile for `codec`.
    pub(super) fn d3d12(device: &ID3D12Device, codec: Codec) -> bool {
        let wanted = match codec {
            Codec::H264 => D3D12_VIDEO_DECODE_PROFILE_H264,
            Codec::Av1 => D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0,
        };
        let Ok(video) = device.cast::<ID3D12VideoDevice>() else { return false };
        let mut count = D3D12_FEATURE_DATA_VIDEO_DECODE_PROFILE_COUNT::default();
        // SAFETY: `count` is the structure this feature fills, of the size given.
        let counted = unsafe {
            video.CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_DECODE_PROFILE_COUNT,
                (&raw mut count).cast(),
                u32::try_from(size_of_val(&count)).unwrap_or(0),
            )
        };
        if counted.is_err() {
            return false;
        }
        let mut profiles = vec![GUID::zeroed(); usize::try_from(count.ProfileCount).unwrap_or(0)];
        let mut listed = D3D12_FEATURE_DATA_VIDEO_DECODE_PROFILES {
            NodeIndex: 0,
            ProfileCount: count.ProfileCount,
            pProfiles: profiles.as_mut_ptr(),
        };
        // SAFETY: `listed` points at room for the count the driver gave, and is of the size given.
        let filled = unsafe {
            video.CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_DECODE_PROFILES,
                (&raw mut listed).cast(),
                u32::try_from(size_of_val(&listed)).unwrap_or(0),
            )
        };
        filled.is_ok() && profiles.contains(&wanted)
    }
}
