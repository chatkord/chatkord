//! A device's surface mapped to DRM PRIME: the DMA-BUFs it lives in.

use super::error::{AvError, check};
use super::format::PixelFormat;
use super::frame::Frame;
use ffmpeg_sys_next as ff;
use std::ffi::c_int;
use std::os::fd::RawFd;

/// One DMA-BUF a mapped surface lives in.
#[derive(Clone, Copy, Debug)]
pub struct DrmObject {
    /// The descriptor, open while the mapping is.
    pub fd: RawFd,
    /// Bytes in the buffer.
    pub size: u64,
    /// The DRM format modifier the buffer is laid out with.
    pub modifier: u64,
}

/// Where one plane of a layer is.
#[derive(Clone, Copy, Debug)]
pub struct DrmPlane {
    /// The object it is in, an index into the mapping's objects.
    pub object: usize,
    /// Bytes from the start of that object to the plane's first row, as the driver said.
    pub offset: isize,
    /// Bytes from one row to the next, as the driver said.
    pub pitch: isize,
}

/// A run of planes that form one DRM format.
#[derive(Clone, Debug)]
pub struct DrmLayer {
    /// A `DRM_FORMAT_*` fourcc.
    pub format: u32,
    /// Its planes, in the format's order.
    pub planes: Vec<DrmPlane>,
}

/// A surface mapped to DRM PRIME. Its objects' descriptors are open until it drops, which gives the surface back.
pub struct DrmMapping {
    _frame: Frame,
    /// The DMA-BUFs.
    pub objects: Vec<DrmObject>,
    /// How they divide into planes.
    pub layers: Vec<DrmLayer>,
}

impl Frame {
    /// This surface, on a device that exports DRM PRIME, mapped for reading, and for writing too where `write`.
    pub fn map_drm(&self, write: bool) -> Result<DrmMapping, AvError> {
        if !self.is_hardware() {
            return Err(AvError::Invalid);
        }
        let mapped = Self::new().ok_or(AvError::NoMemory)?;
        // SAFETY: `mapped` is owned and empty but for the format FFmpeg asks the caller to set.
        unsafe { (*mapped.0).format = PixelFormat::DrmPrime.to_c() as c_int };
        let flags = ff::AV_HWFRAME_MAP_READ as c_int | if write { ff::AV_HWFRAME_MAP_WRITE as c_int } else { 0 };
        // SAFETY: both frames are owned and live, and `self` is on a hardware frames context.
        check(unsafe { ff::av_hwframe_map(mapped.0, self.0, flags) })?;
        // SAFETY: `mapped` is owned and the map succeeded.
        let descriptor = unsafe { (*mapped.0).data[0].cast::<ff::AVDRMFrameDescriptor>() };
        // SAFETY: a DRM PRIME frame's `data[0]` is its descriptor, alive while `mapped` holds it; null is handled.
        let descriptor = unsafe { descriptor.as_ref() }.ok_or(AvError::Invalid)?;
        let objects = descriptor.objects
            [..usize::try_from(descriptor.nb_objects).unwrap_or(0).min(descriptor.objects.len())]
            .iter()
            .map(|o| DrmObject { fd: o.fd, size: o.size as u64, modifier: o.format_modifier })
            .collect();
        let layers = descriptor.layers
            [..usize::try_from(descriptor.nb_layers).unwrap_or(0).min(descriptor.layers.len())]
            .iter()
            .map(|l| DrmLayer {
                format: l.format,
                planes: l.planes[..usize::try_from(l.nb_planes).unwrap_or(0).min(l.planes.len())]
                    .iter()
                    .map(|p| DrmPlane {
                        object: usize::try_from(p.object_index).unwrap_or(usize::MAX),
                        offset: p.offset,
                        pitch: p.pitch,
                    })
                    .collect(),
            })
            .collect();
        Ok(DrmMapping { _frame: mapped, objects, layers })
    }
}
