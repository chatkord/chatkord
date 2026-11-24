//! The pixel formats and colour tags this client names, each with FFmpeg's value in this one place.

use ffmpeg_sys_next as ff;
use std::ffi::c_int;

/// Declares an enum and its map to FFmpeg's, so a variant has its value once and the list of them cannot drift.
macro_rules! mapped {
    (
        $(#[$meta:meta])*
        $name:ident($c:ty) {
            $( $(#[doc = $doc:literal])* $(#[cfg($cfg:meta)])? $variant:ident = $value:path ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[doc = $doc])* $(#[cfg($cfg)])? $variant ),+
        }

        impl $name {
            /// Every variant.
            pub const ALL: &'static [Self] = &[ $( $(#[cfg($cfg)])? Self::$variant ),+ ];

            /// FFmpeg's value for it.
            pub(crate) const fn to_c(self) -> $c {
                match self {
                    $( $(#[cfg($cfg)])? Self::$variant => $value ),+
                }
            }

            /// The variant FFmpeg's `value` names; none for one this client does not name.
            pub(crate) fn from_c(value: $c) -> Option<Self> {
                Self::ALL.iter().copied().find(|each| each.to_c() == value)
            }
        }
    };
}

mapped! {
    /// How a picture's samples are laid out.
    PixelFormat(ff::AVPixelFormat) {
        /// Planar 4:2:0, 8 bits.
        Yuv420p = ff::AVPixelFormat::AV_PIX_FMT_YUV420P,
        /// Planar 4:2:2, 8 bits.
        Yuv422p = ff::AVPixelFormat::AV_PIX_FMT_YUV422P,
        /// Planar 4:2:0 over the full range, which JPEG uses.
        Yuvj420p = ff::AVPixelFormat::AV_PIX_FMT_YUVJ420P,
        /// Planar 4:2:2 over the full range.
        Yuvj422p = ff::AVPixelFormat::AV_PIX_FMT_YUVJ422P,
        /// Planar 4:4:0 over the full range.
        Yuvj440p = ff::AVPixelFormat::AV_PIX_FMT_YUVJ440P,
        /// Planar 4:4:4 over the full range.
        Yuvj444p = ff::AVPixelFormat::AV_PIX_FMT_YUVJ444P,
        /// A luma plane and one of interleaved chroma, 4:2:0.
        Nv12 = ff::AVPixelFormat::AV_PIX_FMT_NV12,
        /// Packed 4:2:2, luma first.
        Yuyv422 = ff::AVPixelFormat::AV_PIX_FMT_YUYV422,
        /// Packed 4:2:2, chroma first.
        Uyvy422 = ff::AVPixelFormat::AV_PIX_FMT_UYVY422,
        /// Four bytes a pixel, blue first, the fourth unused.
        Bgr0 = ff::AVPixelFormat::AV_PIX_FMT_BGR0,
        /// Four bytes a pixel, blue first, the fourth alpha.
        Bgra = ff::AVPixelFormat::AV_PIX_FMT_BGRA,
        /// A VA-API surface.
        Vaapi = ff::AVPixelFormat::AV_PIX_FMT_VAAPI,
        /// A Vulkan image.
        Vulkan = ff::AVPixelFormat::AV_PIX_FMT_VULKAN,
        /// A DMA-BUF described by DRM.
        DrmPrime = ff::AVPixelFormat::AV_PIX_FMT_DRM_PRIME,
        /// A D3D11 texture.
        #[cfg(windows)]
        D3d11 = ff::AVPixelFormat::AV_PIX_FMT_D3D11,
        /// A D3D12 resource.
        #[cfg(windows)]
        D3d12 = ff::AVPixelFormat::AV_PIX_FMT_D3D12,
    }
}

impl PixelFormat {
    /// The variant a frame's `format` field names.
    pub(crate) fn from_raw(raw: c_int) -> Option<Self> {
        Self::ALL.iter().copied().find(|each| each.to_c() as c_int == raw)
    }

    /// Whether the format keeps its luma in a plane of its own.
    #[cfg(any(target_os = "linux", test))]
    pub fn is_planar(self) -> bool {
        self.descriptor().is_some_and(|described| described.flags & ff::AV_PIX_FMT_FLAG_PLANAR as u64 != 0)
    }

    /// Whether FFmpeg spells the full range in the format itself.
    pub fn is_full_range_spelling(self) -> bool {
        matches!(self, Self::Yuvj420p | Self::Yuvj422p | Self::Yuvj440p | Self::Yuvj444p)
    }

    /// The bytes one row of `plane` holds at `width` pixels, none for a plane the format lacks.
    pub(super) fn row_bytes(self, width: c_int, plane: usize) -> Option<usize> {
        let plane = c_int::try_from(plane).ok()?;
        // SAFETY: a pure function of its arguments; an unknown plane or width is a negative status.
        let bytes = unsafe { ff::av_image_get_linesize(self.to_c(), width, plane) };
        usize::try_from(bytes).ok().filter(|bytes| *bytes > 0)
    }

    /// The rows `plane` holds at `height` pixels, none for a plane the format lacks.
    pub(super) fn plane_rows(self, plane: usize, height: c_int) -> Option<usize> {
        let described = self.descriptor()?;
        let present = described.comp[..usize::from(described.nb_components)]
            .iter()
            .any(|component| usize::try_from(component.plane) == Ok(plane));
        if !present {
            return None;
        }
        let rows = usize::try_from(height).ok()?;
        Some(if plane == 0 { rows } else { rows.div_ceil(1 << described.log2_chroma_h) })
    }

    fn descriptor(self) -> Option<&'static ff::AVPixFmtDescriptor> {
        // SAFETY: FFmpeg's descriptor table is static and only read; a format it lacks gives null.
        let described = unsafe { ff::av_pix_fmt_desc_get(self.to_c()) };
        // SAFETY: a non-null descriptor is a static entry of that table.
        unsafe { described.as_ref() }
    }
}

mapped! {
    /// The matrix a picture's colour differences are tagged with.
    ColourSpace(ff::AVColorSpace) {
        /// Not said.
        Unspecified = ff::AVColorSpace::AVCOL_SPC_UNSPECIFIED,
        /// ITU-R BT.709.
        Bt709 = ff::AVColorSpace::AVCOL_SPC_BT709,
        /// ITU-R BT.470 BG, which weighs as BT.601.
        Bt470bg = ff::AVColorSpace::AVCOL_SPC_BT470BG,
        /// SMPTE 170M, which weighs as BT.601.
        Smpte170m = ff::AVColorSpace::AVCOL_SPC_SMPTE170M,
        /// FCC, which weighs nearly as BT.601.
        Fcc = ff::AVColorSpace::AVCOL_SPC_FCC,
        /// ITU-R BT.2020, non-constant luminance.
        Bt2020Ncl = ff::AVColorSpace::AVCOL_SPC_BT2020_NCL,
        /// A value FFmpeg reserves.
        Reserved = ff::AVColorSpace::AVCOL_SPC_RESERVED,
    }
}

mapped! {
    /// The range a picture's samples are tagged with.
    ColourRange(ff::AVColorRange) {
        /// Not said.
        Unspecified = ff::AVColorRange::AVCOL_RANGE_UNSPECIFIED,
        /// Studio swing (FFmpeg's MPEG).
        Limited = ff::AVColorRange::AVCOL_RANGE_MPEG,
        /// Every value (FFmpeg's JPEG).
        Full = ff::AVColorRange::AVCOL_RANGE_JPEG,
    }
}

/// ITU-R BT.709's primaries, the one tag a picture's primaries are given.
#[cfg(any(windows, test))]
pub(crate) const BT709_PRIMARIES: ff::AVColorPrimaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;

/// ITU-R BT.709's transfer function, the one tag a picture's transfer is given.
#[cfg(any(windows, test))]
pub(crate) const BT709_TRANSFER: ff::AVColorTransferCharacteristic = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;

impl ColourSpace {
    /// The variant a tag names; a tag this client does not name reads as unspecified, which every consumer reads as
    /// BT.709.
    pub(crate) fn read(tag: ff::AVColorSpace) -> Self {
        Self::from_c(tag).unwrap_or(Self::Unspecified)
    }
}

impl ColourRange {
    /// The variant a tag names; a tag this client does not name reads as unspecified.
    pub(crate) fn read(tag: ff::AVColorRange) -> Self {
        Self::from_c(tag).unwrap_or(Self::Unspecified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_and_tag_round_trips_through_its_c_value() {
        for each in PixelFormat::ALL {
            assert_eq!(PixelFormat::from_c(each.to_c()), Some(*each), "{each:?}");
            assert_eq!(PixelFormat::from_raw(each.to_c() as c_int), Some(*each), "{each:?}");
        }
        for each in ColourSpace::ALL {
            assert_eq!(ColourSpace::from_c(each.to_c()), Some(*each), "{each:?}");
        }
        for each in ColourRange::ALL {
            assert_eq!(ColourRange::from_c(each.to_c()), Some(*each), "{each:?}");
        }
    }

    #[test]
    fn a_value_nobody_names_is_none_and_a_tag_nobody_names_reads_as_unspecified() {
        assert_eq!(PixelFormat::from_raw(ff::AVPixelFormat::AV_PIX_FMT_NONE as c_int), None);
        assert_eq!(ColourSpace::read(ff::AVColorSpace::AVCOL_SPC_YCGCO), ColourSpace::Unspecified);
    }

    #[test]
    fn the_rows_and_bytes_of_a_plane_follow_the_format() {
        assert_eq!(PixelFormat::Nv12.plane_rows(0, 32), Some(32));
        assert_eq!(PixelFormat::Nv12.plane_rows(1, 32), Some(16));
        assert_eq!(PixelFormat::Nv12.plane_rows(2, 32), None);
        assert_eq!(PixelFormat::Yuv420p.plane_rows(2, 33), Some(17));
        assert_eq!(PixelFormat::Yuyv422.plane_rows(0, 32), Some(32));
        assert_eq!(PixelFormat::Nv12.row_bytes(64, 1), Some(64));
        assert_eq!(PixelFormat::Yuv420p.row_bytes(64, 1), Some(32));
        assert_eq!(PixelFormat::Yuyv422.row_bytes(64, 0), Some(128));
        assert!(PixelFormat::Nv12.is_planar() && !PixelFormat::Yuyv422.is_planar());
    }
}
