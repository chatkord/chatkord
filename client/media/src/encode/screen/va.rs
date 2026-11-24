//! libva's values, layouts and calls that the bindings leave out, as `va.h` and `va_drmcommon.h` declare them: a
//! surface made from a DMA-BUF, and a surface exported as one.

use std::ffi::{c_int, c_uint, c_void};

/// `VA_RT_FORMAT_RGB32`.
pub(super) const RT_FORMAT_RGB32: c_uint = 0x0002_0000;
/// `VASurfaceAttribMemoryType`.
pub(super) const MEMORY_TYPE: c_int = 6;
/// `VASurfaceAttribExternalBufferDescriptor`.
pub(super) const EXTERNAL_BUFFER_DESCRIPTOR: c_int = 7;
/// `VA_SURFACE_ATTRIB_SETTABLE`.
pub(super) const SETTABLE: u32 = 2;
/// `VAGenericValueTypeInteger`.
pub(super) const INTEGER: c_int = 1;
/// `VAGenericValueTypePointer`.
pub(super) const POINTER: c_int = 3;
/// `VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2`.
pub(super) const MEM_TYPE_DRM_PRIME_2: u32 = 0x4000_0000;
/// `VA_EXPORT_SURFACE_WRITE_ONLY`.
pub(super) const EXPORT_WRITE_ONLY: u32 = 0x0002;
/// `VA_EXPORT_SURFACE_SEPARATE_LAYERS`.
pub(super) const EXPORT_SEPARATE_LAYERS: u32 = 0x0004;
/// `VA_STATUS_SUCCESS`.
pub(super) const SUCCESS: c_int = 0;

/// `VAGenericValue`: its kind, then a union of an `int32_t`, a `float` and pointers, which a `u64` lays out alike.
#[repr(C)]
pub(super) struct Value {
    /// One of `INTEGER` and `POINTER`.
    pub(super) kind: c_int,
    /// The integer, or the pointer's address.
    pub(super) value: u64,
}

/// `VASurfaceAttrib`.
#[repr(C)]
pub(super) struct Attribute {
    /// One of `MEMORY_TYPE` and `EXTERNAL_BUFFER_DESCRIPTOR`.
    pub(super) kind: c_int,
    /// `SETTABLE`.
    pub(super) flags: u32,
    /// What it is set to.
    pub(super) value: Value,
}

/// `VADRMPRIMESurfaceDescriptor`'s object: one DMA-BUF.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(super) struct PrimeObject {
    /// The DMA-BUF; an export hands the caller each one to close.
    pub(super) fd: c_int,
    /// Its bytes.
    pub(super) size: u32,
    /// How the planes in it are laid out.
    pub(super) drm_format_modifier: u64,
}

/// `VADRMPRIMESurfaceDescriptor`'s layer: planes that form one DRM format.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(super) struct PrimeLayer {
    /// A `DRM_FORMAT_*` fourcc.
    pub(super) drm_format: u32,
    /// How many of the four entries below are used.
    pub(super) num_planes: u32,
    /// Each plane's object, an index into the descriptor's objects.
    pub(super) object_index: [u32; 4],
    /// Each plane's first byte in its object.
    pub(super) offset: [u32; 4],
    /// Each plane's bytes from one row to the next.
    pub(super) pitch: [u32; 4],
}

/// `VADRMPRIMESurfaceDescriptor`.
#[derive(Default)]
#[repr(C)]
pub(super) struct PrimeDescriptor {
    /// The surface's `VA_FOURCC_*`.
    pub(super) fourcc: u32,
    /// Its pixels across.
    pub(super) width: u32,
    /// Its pixels down.
    pub(super) height: u32,
    /// How many of the four objects are used.
    pub(super) num_objects: u32,
    /// The DMA-BUFs the surface is in.
    pub(super) objects: [PrimeObject; 4],
    /// How many of the four layers are used.
    pub(super) num_layers: u32,
    /// How the objects divide into formats.
    pub(super) layers: [PrimeLayer; 4],
}

unsafe extern "C" {
    /// `vaCreateSurfaces`.
    #[link_name = "vaCreateSurfaces"]
    pub(super) fn va_create_surfaces(
        display: *mut c_void,
        format: c_uint,
        width: c_uint,
        height: c_uint,
        surfaces: *mut u32,
        count: c_uint,
        attributes: *mut Attribute,
        attribute_count: c_uint,
    ) -> c_int;
    /// `vaDestroySurfaces`.
    #[link_name = "vaDestroySurfaces"]
    pub(super) fn va_destroy_surfaces(display: *mut c_void, surfaces: *mut u32, count: c_int) -> c_int;
    /// `vaExportSurfaceHandle`, which writes a `PrimeDescriptor` for `MEM_TYPE_DRM_PRIME_2`.
    #[link_name = "vaExportSurfaceHandle"]
    pub(super) fn va_export_surface_handle(
        display: *mut c_void,
        surface: u32,
        memory_type: u32,
        flags: u32,
        descriptor: *mut c_void,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layouts libva's headers give, on the 64-bit systems this builds for.
    #[test]
    fn the_declared_layouts_are_libva_s() {
        assert_eq!(size_of::<Value>(), 16);
        assert_eq!(size_of::<Attribute>(), 24);
        assert_eq!(size_of::<PrimeObject>(), 16);
        assert_eq!(size_of::<PrimeLayer>(), 56);
        assert_eq!(std::mem::offset_of!(PrimeDescriptor, num_layers), 16 + 4 * 16);
        assert_eq!(size_of::<PrimeDescriptor>(), 16 + 4 * 16 + 4 + 4 * 56 + 4);
    }
}
