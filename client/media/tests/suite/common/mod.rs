//! What the media tests share: a synthetic picture, the encode settings, and a luma comparison.

#[cfg(target_os = "linux")]
pub(crate) mod dmabuf;

use media::{Encoded, Encoder, Nv12, Session, Settings, survey};
use std::num::NonZeroU32;

pub(crate) const WIDTH: usize = 1280;
pub(crate) const HEIGHT: usize = 720;
pub(crate) const FRAMES: i64 = 60;
pub(crate) const SETTINGS: Settings = Settings {
    width: WIDTH,
    height: HEIGHT,
    frames_per_second: 30,
    bits_per_second: 4_000_000,
    key_frame_every: NonZeroU32::new(30),
};
/// The screen the settings are for, as the compositor sends it.
#[cfg(target_os = "linux")]
pub(crate) const LAYOUT: capture::Layout =
    capture::Layout { pixels: capture::Pixels::Bgrx, width: WIDTH as u32, height: HEIGHT as u32 };

pub(crate) use tier::skipped;

/// The first encoder that opens, or a recorded skip.
pub(crate) fn first_encoder() -> Option<Encoder> {
    tier::lab();
    let found = survey().encoders.into_iter().next();
    if found.is_none() {
        skipped("no hardware encoder opens on this machine");
    }
    found
}

/// The first encoder of each codec that opens, or a recorded skip when none does.
pub(crate) fn first_of_each_codec() -> Vec<Encoder> {
    tier::lab();
    let mut found = survey().encoders;
    found.dedup_by_key(|encoder| encoder.codec());
    if found.is_empty() {
        skipped("no hardware encoder opens on this machine");
    }
    found
}

/// A diagonal gradient with a block that moves, so every frame differs and detail is checkable.
pub(crate) fn picture(n: usize) -> Nv12 {
    let mut data = vec![0u8; WIDTH * HEIGHT * 3 / 2];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let in_block = (x + n * 8) % WIDTH < 160 && (y / 160) % 2 == 0;
            data[y * WIDTH + x] = if in_block { 235 } else { ((x + y) / 8 % 200 + 16) as u8 };
        }
    }
    data[WIDTH * HEIGHT..].fill(128);
    Nv12::new(WIDTH, HEIGHT, data).expect("test picture is a valid frame")
}

pub(crate) fn encode_all(encoder: &Encoder) -> Vec<Encoded> {
    let mut session = Session::open(encoder, SETTINGS).unwrap_or_else(|e| panic!("{encoder} would not open: {e}"));
    let mut out = Vec::new();
    for n in 0..FRAMES as usize {
        out.extend(session.encode(&picture(n)).unwrap_or_else(|e| panic!("{encoder} would not encode: {e}")));
    }
    out.extend(session.finish().expect("finish"));
    out
}

/// Luma PSNR in dB of `got` against the synthetic picture number `n`.
pub(crate) fn psnr(got: &Nv12, n: usize) -> f64 {
    let source = picture(n);
    let squared: f64 = got
        .luma()
        .iter()
        .zip(source.luma())
        .map(|(a, b)| {
            let d = f64::from(*a) - f64::from(*b);
            d * d
        })
        .sum();
    let mse = squared / (WIDTH * HEIGHT) as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// The profile_idc and level_idc an H.264 stream's first unit states in its SPS (100 is High; 52 is level 5.2).
pub(crate) fn profile_and_level(first_unit: &[u8]) -> Option<(u8, u8)> {
    let start = first_unit.windows(5).position(|w| w[..4] == [0, 0, 0, 1] && w[4] & 0x1f == 7)? + 5;
    Some((*first_unit.get(start)?, *first_unit.get(start + 2)?))
}

/// The level a stream of `width` x `height` at `per_second` needs, as `media` tells the encoders that need telling.
pub(crate) fn level_needed(width: usize, height: usize, per_second: usize) -> u8 {
    let (width, height, per_second) = (width as u32, height as u32, per_second as u32);
    media::h264_level(width, height, per_second).unwrap_or(u8::MAX)
}

/// The colour description an H.264 stream's first unit carries in its SPS: whether the range is full, and the
/// primaries, transfer and matrix codes (1 is BT.709). None when the stream leaves a receiver to guess.
pub(crate) fn colour_of(first_unit: &[u8]) -> Option<(bool, [u8; 3])> {
    let start = first_unit.windows(5).position(|w| w[..4] == [0, 0, 0, 1] && w[4] & 0x1f == 7)? + 5;
    // Emulation prevention bytes (00 00 03) are not part of the syntax.
    let mut rbsp = Vec::new();
    let mut zeros = 0;
    for &byte in &first_unit[start..] {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        rbsp.push(byte);
    }
    let mut bits = Bits { data: &rbsp, at: 0 };
    let profile = bits.take(8)?;
    bits.take(16)?; // constraint flags, level
    bits.ue()?; // sps id
    if profile == 100 {
        if bits.ue()? == 3 {
            bits.take(1)?; // separate_colour_plane
        }
        bits.ue()?; // bit_depth_luma - 8
        bits.ue()?; // bit_depth_chroma - 8
        bits.take(1)?; // qpprime_y_zero_transform_bypass
        if bits.take(1)? == 1 {
            return None; // scaling matrices, which no encoder here writes
        }
    }
    bits.ue()?; // log2_max_frame_num - 4
    if bits.ue()? == 0 {
        bits.ue()?; // log2_max_pic_order_cnt_lsb - 4
    }
    bits.ue()?; // max_num_ref_frames
    bits.take(1)?; // gaps_in_frame_num_value_allowed
    bits.ue()?; // width in macroblocks - 1
    bits.ue()?; // height in map units - 1
    if bits.take(1)? == 0 {
        bits.take(1)?; // mb_adaptive_frame_field
    }
    bits.take(1)?; // direct_8x8_inference
    if bits.take(1)? == 1 {
        for _ in 0..4 {
            bits.ue()?; // frame cropping
        }
    }
    if bits.take(1)? == 0 {
        return None; // no VUI
    }
    if bits.take(1)? == 1 && bits.take(8)? == 255 {
        bits.take(32)?; // extended aspect ratio
    }
    if bits.take(1)? == 1 {
        bits.take(1)?; // overscan_appropriate
    }
    if bits.take(1)? == 0 {
        return None; // no video signal type
    }
    bits.take(3)?; // video_format
    let full = bits.take(1)? == 1;
    if bits.take(1)? == 0 {
        return None; // no colour description
    }
    Some((full, [bits.take(8)? as u8, bits.take(8)? as u8, bits.take(8)? as u8]))
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.data.get(self.at / 8)?;
            value = value << 1 | u32::from(byte >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Some(value)
    }

    /// An unsigned Exp-Golomb code.
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.take(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + if zeros == 0 { 0 } else { self.take(zeros)? })
    }
}
