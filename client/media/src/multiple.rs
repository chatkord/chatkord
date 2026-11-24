//! The sizes an encoder codes a picture at without padding it. AV1 has no crop in its stream, so what an encoder pads
//! a picture to is what every viewer decodes; a stream of a multiple of its padding shows no padding.

use crate::probe::Codec;

/// The size, on each side, of which an encoder's stream is a multiple so a viewer decodes the picture sent and no
/// padding: two for a codec that crops what it pads, and what the encoder pads to for AV1, read when it opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Multiple {
    /// Pixels across, a power of two from two up.
    pub width: u32,
    /// Pixels down, a power of two from two up.
    pub height: u32,
}

impl Multiple {
    /// An even size, which NV12 needs and every encoder takes as it is.
    pub const EVEN: Self = Self { width: 2, height: 2 };

    /// The size, two past a multiple of the largest padding a coder has (AV1's 128-pixel superblock), at which an
    /// encoder is opened to read its multiple: one that pads to `n` pads it by `n - 2`.
    pub(crate) const PROBE: (u32, u32) = (10 * 128 + 2, 6 * 128 + 2);

    /// The multiple of an encoder of `codec` that, opened at [`Multiple::PROBE`], codes pictures of `padded`.
    pub(crate) fn of(codec: Codec, padded: (u32, u32)) -> Self {
        let side = |padded: u32, probe: u32| (padded.saturating_sub(probe) + 2).next_power_of_two();
        match codec {
            Codec::H264 => Self::EVEN,
            Codec::Av1 => Self { width: side(padded.0, Self::PROBE.0), height: side(padded.1, Self::PROBE.1) },
        }
    }

    /// The largest size of this multiple inside `width` × `height`: the stream a picture of that size is sent as,
    /// its right and bottom edge left out. Zero on a side smaller than the multiple.
    pub fn inside(self, width: u32, height: u32) -> (u32, u32) {
        (width - width % self.width, height - height % self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_av1_encoder_s_multiple_is_what_it_pads_the_probe_to_and_h264_s_is_even() {
        let (w, h) = Multiple::PROBE;
        assert_eq!(Multiple::of(Codec::Av1, (w, h)), Multiple::EVEN, "pads nothing");
        assert_eq!(Multiple::of(Codec::Av1, (w + 6, h)), Multiple { width: 8, height: 2 }, "AMF on RDNA4");
        assert_eq!(Multiple::of(Codec::Av1, (w + 62, h + 14)), Multiple { width: 64, height: 16 }, "AMF on RDNA3");
        assert_eq!(Multiple::of(Codec::H264, (w + 14, h + 14)), Multiple::EVEN, "H.264 crops what it pads");
    }

    #[test]
    fn a_size_is_rounded_down_to_the_multiple_on_each_side() {
        let amf = Multiple { width: 8, height: 2 };
        assert_eq!(amf.inside(1902, 1102), (1896, 1102));
        assert_eq!(amf.inside(1920, 1080), (1920, 1080));
        assert_eq!(Multiple { width: 64, height: 16 }.inside(1902, 1102), (1856, 1088));
        assert_eq!(Multiple::EVEN.inside(1903, 1103), (1902, 1102));
        assert_eq!(amf.inside(6, 1), (0, 0));
    }
}
