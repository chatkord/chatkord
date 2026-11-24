//! What each encoder here makes of a busy screen at each size, frame rate and rate: the steady state's luma PSNR
//! against the rate it sent, and the first key frame's recovery, printed for the record. The share's envelope is set
//! from these numbers.

use crate::common;

use common::skipped;
use media::{Codec, Decoder, Encoder, Nv12, Session, Settings, Sources, survey};

/// The stream sizes a share is sent at, by their short side: 720p, 1080p, 1440p and 4K.
const SIZES: [(usize, usize); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
/// Pictures a second.
const FRAME_RATES: [usize; 2] = [30, 60];
/// The ladder of rates, in thousandths of a bit per pixel per picture.
const LADDER: [u64; 9] = [10, 15, 20, 30, 45, 60, 80, 110, 150];
/// The most a share is sent at, past which a rung is left out (`proto::rules::SHARE_MAX_BITS_PER_SECOND`).
const MOST: u64 = 40_000_000;
/// How long each rung is encoded: a second from its key frame, two settled, then one from a key frame asked for.
const SECONDS: usize = 4;
/// The second at which a key frame is asked for, as a viewer who joins asks.
const ASKED_AT: usize = 3;
/// The thresholds a rung is read against: blocky but legible, good, and past which more bits are not seen.
const THRESHOLDS: [f64; 3] = [30.0, 35.0, 40.0];

/// What the screen shows.
#[derive(Clone, Copy, Debug)]
enum Content {
    /// A textured scene panning 360 pixels a second, objects crossing it and its light changing, as a game does.
    Motion,
    /// A page of text, one-pixel strokes in words and lines, scrolling 120 pixels a second.
    Text,
}

/// A picture larger than the stream, each sent picture a window into it.
struct Canvas {
    width: usize,
    luma: Vec<u8>,
    content: Content,
}

/// A repeatable stream of numbers, so every run sends the same pictures.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

impl Canvas {
    /// Room for `pictures` windows of `width` x `height` at `per_second`.
    fn new(content: Content, width: usize, height: usize, per_second: usize, pictures: usize) -> Self {
        let travel = Self::step(content, per_second) * pictures;
        let (wide, high) = match content {
            Content::Motion => (width + travel, height + travel),
            Content::Text => (width, height + travel),
        };
        let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
        let luma = match content {
            Content::Motion => textured(wide, high, &mut noise),
            Content::Text => page(wide, high, &mut noise),
        };
        Self { width: wide, luma, content }
    }

    /// How far the window moves each picture.
    fn step(content: Content, per_second: usize) -> usize {
        match content {
            Content::Motion => 360 / per_second,
            Content::Text => 120 / per_second,
        }
    }

    /// Picture `n` of a stream of `width` x `height` at `per_second`.
    fn picture(&self, n: usize, width: usize, height: usize, per_second: usize) -> Nv12 {
        let moved = Self::step(self.content, per_second) * n;
        let (x0, y0) = match self.content {
            Content::Motion => (moved, moved),
            Content::Text => (0, moved),
        };
        let mut data = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            let from = (y0 + y) * self.width + x0;
            data[y * width..(y + 1) * width].copy_from_slice(&self.luma[from..from + width]);
        }
        if let Content::Motion = self.content {
            self.cross(&mut data[..width * height], n, width, height, per_second);
        }
        Nv12::new(width, height, data).expect("a picture of the stream's size")
    }

    /// What a pan alone leaves out of a scene: objects crossing it at speeds of their own, which are no whole number of
    /// pixels a picture, and its light rising and falling every two seconds.
    fn cross(&self, luma: &mut [u8], n: usize, width: usize, height: usize, per_second: usize) {
        let seconds = n as f64 / per_second as f64;
        let (object_width, object_height) = (width / 6, height / 5);
        for k in 0..10usize {
            let speed = (97.0 + 41.0 * k as f64, 53.0 + 29.0 * (9 - k) as f64);
            let span = ((width - object_width) as f64, (height - object_height) as f64);
            let at = |start: f64, speed: f64, span: f64| {
                let travelled = (start + speed * seconds) % (2.0 * span);
                (if travelled > span { 2.0 * span - travelled } else { travelled }) as usize
            };
            let (x, y) = (at(k as f64 * 211.0, speed.0, span.0), at(k as f64 * 137.0, speed.1, span.1));
            let (from_x, from_y) =
                (k * 373 % (self.width - object_width), k * 251 % (self.luma.len() / self.width - object_height));
            for row in 0..object_height {
                let from = (from_y + row) * self.width + from_x;
                luma[(y + row) * width + x..(y + row) * width + x + object_width]
                    .copy_from_slice(&self.luma[from..from + object_width]);
            }
        }
        let light = (12.0 * (seconds * std::f64::consts::PI).sin()).round() as i16;
        for value in luma.iter_mut() {
            *value = (i16::from(*value) + light).clamp(16, 235) as u8;
        }
    }
}

/// Value noise summed at four scales, 256 down to 4 pixels, in roughly the share of each a rendered scene has: broad
/// shading with detail on it and a fine texture under that.
fn textured(wide: usize, high: usize, noise: &mut Noise) -> Vec<u8> {
    let mut sum = vec![0f32; wide * high];
    for (cell, weight) in [(256usize, 110.0f32), (64, 60.0), (16, 32.0), (4, 14.0)] {
        let (cols, rows) = (wide / cell + 2, high / cell + 2);
        let corners: Vec<f32> = (0..cols * rows).map(|_| (noise.next() % 1000) as f32 / 1000.0).collect();
        for y in 0..high {
            let (row, fy) = (y / cell, (y % cell) as f32 / cell as f32);
            for x in 0..wide {
                let (col, fx) = (x / cell, (x % cell) as f32 / cell as f32);
                let at = |c: usize, r: usize| corners[r * cols + c];
                let top = at(col, row) * (1.0 - fx) + at(col + 1, row) * fx;
                let bottom = at(col, row + 1) * (1.0 - fx) + at(col + 1, row + 1) * fx;
                sum[y * wide + x] += (top * (1.0 - fy) + bottom * fy) * weight;
            }
        }
    }
    sum.into_iter().map(|v| (16.0 + v.min(219.0)) as u8).collect()
}

/// A page of text: lines 22 pixels apart in paragraphs, each line words of two to nine glyphs with a space between,
/// a glyph a few one-pixel strokes in a 6 x 10 box, dark ink on a light ground, a margin on each side.
fn page(wide: usize, high: usize, noise: &mut Noise) -> Vec<u8> {
    let mut luma = vec![240u8; wide * high];
    let (margin, cell, pitch) = (40usize, 8usize, 22usize);
    let ink = |luma: &mut [u8], x: usize, y: usize| {
        if x < wide && y < high {
            luma[y * wide + x] = 35;
        }
    };
    let mut line = 0;
    while (line + 1) * pitch < high {
        let paragraph_end = noise.next().is_multiple_of(7);
        let fill = margin + (wide.saturating_sub(2 * margin)) * (40 + (noise.next() % 56) as usize) / 100;
        let mut x = margin;
        while x + cell < fill {
            let word = 2 + (noise.next() % 8) as usize;
            for _ in 0..word {
                let strokes = noise.next();
                let top = line * pitch + 6;
                for stroke in 0..4 {
                    match (strokes >> (stroke * 4)) % 6 {
                        0 => (0..10).for_each(|d| ink(&mut luma, x + 1, top + d)),
                        1 => (0..10).for_each(|d| ink(&mut luma, x + 5, top + d)),
                        2 => (0..6).for_each(|d| ink(&mut luma, x + d, top)),
                        3 => (0..6).for_each(|d| ink(&mut luma, x + d, top + 5)),
                        4 => (0..6).for_each(|d| ink(&mut luma, x + d, top + 9)),
                        _ => (0..5).for_each(|d| ink(&mut luma, x + 1 + d, top + 1 + d * 2)),
                    }
                }
                x += cell;
            }
            x += cell;
        }
        line += if paragraph_end { 2 } else { 1 };
    }
    luma
}

/// The squared luma error of `got` against `sent`, per pixel.
fn mean_squared(got: &Nv12, sent: &Nv12) -> f64 {
    let squared: f64 = got
        .luma()
        .iter()
        .zip(sent.luma())
        .map(|(a, b)| {
            let d = f64::from(*a) - f64::from(*b);
            d * d
        })
        .sum();
    squared / got.luma().len() as f64
}

fn decibels(mse: f64) -> f64 {
    if mse <= 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// What one rung came to: its steady state (the pictures between the first second and the key frame asked for) as
/// luma PSNR and the bits a second the encoder sent for it, the worst half second of the first second and of the second
/// after the asked key frame, which are each key frame's recovery, and each key frame's bits.
struct Rung {
    steady: f64,
    sent_bits_per_second: f64,
    start_worst: f64,
    asked_worst: f64,
    key_frame_bits: [usize; 2],
}

/// `encoder` sending `canvas` at `width` x `height`, `per_second`, held to `bits` a second.
fn rung(
    encoder: &Encoder,
    canvas: &Canvas,
    (width, height): (usize, usize),
    per_second: usize,
    bits: u64,
) -> Option<Rung> {
    let settings = Settings {
        width,
        height,
        frames_per_second: per_second as u32,
        bits_per_second: u32::try_from(bits).ok()?,
        key_frame_every: None,
    };
    let mut session = match Session::open(encoder, settings) {
        Ok(session) => session,
        Err(why) => {
            skipped(format!("{encoder} does not open at {width}x{height}@{per_second}: {why}"));
            return None;
        }
    };
    let pictures = per_second * SECONDS;
    let mut units = Vec::new();
    for n in 0..pictures {
        if n == ASKED_AT * per_second {
            session.request_key_frame();
        }
        units.extend(session.encode(&canvas.picture(n, width, height, per_second)).expect("encode"));
    }
    units.extend(session.finish().expect("finish"));
    let (asked, per) = (ASKED_AT as i64 * per_second as i64, per_second as i64);
    let settled = |pts: i64| (per..asked).contains(&pts);
    let key_frame_bits = |at: i64| units.iter().find(|unit| unit.pts == at).map_or(0, |unit| unit.data.len() * 8);
    let steady_bytes: usize = units.iter().filter(|unit| settled(unit.pts)).map(|unit| unit.data.len()).sum();
    let mut decoder = Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() })
        .expect("a software decoder");
    let mut decoded = Vec::new();
    for unit in &units {
        decoded.extend(decoder.decode(&unit.data, unit.pts).expect("decode"));
    }
    decoded.extend(decoder.finish().expect("finish decoding"));
    assert_eq!(decoded.len(), pictures, "{encoder}: every picture decoded");
    let errors: Vec<(i64, f64)> = decoded
        .iter()
        .map(|got| (got.pts, mean_squared(&got.picture, &canvas.picture(got.pts as usize, width, height, per_second))))
        .collect();
    let steady: Vec<f64> = errors.iter().filter(|(pts, _)| settled(*pts)).map(|(_, mse)| *mse).collect();
    let worst = |from: i64| {
        let second: Vec<f64> =
            errors.iter().filter(|(pts, _)| (from..from + per).contains(pts)).map(|(_, mse)| *mse).collect();
        second.chunks(per_second / 2).map(|half| half.iter().sum::<f64>() / half.len() as f64).fold(0.0, f64::max)
    };
    Some(Rung {
        steady: decibels(steady.iter().sum::<f64>() / steady.len() as f64),
        sent_bits_per_second: steady_bytes as f64 * 8.0 / (ASKED_AT - 1) as f64,
        start_worst: decibels(worst(0)),
        asked_worst: decibels(worst(asked)),
        key_frame_bits: [key_frame_bits(0), key_frame_bits(asked)],
    })
}

/// Every encoder this machine opens, each once: a codec's hardware ones, then its software one.
fn encoders() -> Vec<Encoder> {
    let report = survey();
    let mut all: Vec<Encoder> = Vec::new();
    for codec in [Codec::H264, Codec::Av1] {
        for encoder in report.memory_order(codec) {
            if !all.iter().any(|seen| seen.name == encoder.name) {
                all.push(encoder.clone());
            }
        }
    }
    all
}

mod lab {
    use super::*;

    /// `h264_amf` passes a key frame's budget by a known amount, and no more: a 1080p page at 0.045 bits per pixel, its
    /// first key frame and one asked after a second, each against the most frames' bits measured on the RX 9060 XT
    /// (first 25.2 at both rates, asked 18.2 at 30 and 22.8 at 60).
    #[test]
    #[ignore = "a measurement, run on request"]
    fn h264_amf_s_first_and_asked_key_frames_stay_within_the_bits_measured_for_them() {
        key_frames_stay_within(&["h264_amf"], |_| 27.0, [(30, 20.0), (60, 25.0)]);
    }

    /// AMD's VCN takes its first key frame's size from the bits a frame gets, whatever buffer, fullness or first QP it
    /// is given: a 1080p page at 0.045 bits per pixel, at 30 and 60 a second, measured on the RX 9060 XT (Mesa 26.2.6)
    /// at 6.9 frames' bits on `h264_vaapi`, 7.0 on `h264_vulkan` and 9.4 on `av1_vaapi` and `av1_vulkan`; the key frame
    /// asked for after a second took 9.1 at 30 and 17.5 to 17.7 at 60.
    #[test]
    #[ignore = "a measurement, run on request"]
    fn vcn_s_first_key_frame_stays_within_the_bits_measured_for_it() {
        key_frames_stay_within(
            &["h264_vaapi", "h264_vulkan", "av1_vaapi", "av1_vulkan"],
            |encoder| if encoder.name.as_str().starts_with("h264") { 7.5 } else { 10.0 },
            [(30, 10.0), (60, 19.0)],
        );
    }

    /// Each named encoder that opens here, on a 1080p page at 0.045 bits per pixel: its first key frame within
    /// `first_most` frames' bits and the one asked after a second within each rate's most.
    fn key_frames_stay_within(names: &[&str], first_most: impl Fn(&Encoder) -> f64, asked_most: [(usize, f64); 2]) {
        tier::lab();
        let named: Vec<Encoder> =
            encoders().into_iter().filter(|encoder| names.contains(&encoder.name.as_str())).collect();
        if named.is_empty() {
            return skipped(format_args!("none of {names:?} opens here"));
        }
        for encoder in &named {
            for (per_second, asked_most) in asked_most {
                let size = SIZES[1];
                let bits = (size.0 * size.1 * per_second) as u64 * 45 / 1000;
                let canvas = Canvas::new(Content::Text, size.0, size.1, per_second, per_second * SECONDS);
                if let Some(rung) = rung(encoder, &canvas, size, per_second, bits) {
                    let frames = |key: usize| key as f64 * per_second as f64 / bits as f64;
                    let [first, asked] = rung.key_frame_bits.map(frames);
                    assert!(
                        first <= first_most(encoder),
                        "{encoder} at {per_second} a second: the first key frame is {first:.1} frames' bits"
                    );
                    assert!(
                        asked <= asked_most,
                        "{encoder} at {per_second} a second: the key frame asked for is {asked:.1} frames' bits"
                    );
                }
            }
        }
    }

    /// The ladder of every encoder here, both contents, every size and frame rate, with the least rate sent at which
    /// the steady state reaches each threshold. Its numbers go in the commit that sets the envelope; `QUALITY_ENCODER` narrows it to one
    /// encoder by FFmpeg's name, `QUALITY_CONTENT` to `Motion` or `Text`, `QUALITY_SHORT_SIDE` to one size.
    #[test]
    #[ignore = "a measurement, run on request"]
    fn each_encoder_s_quality_by_rate_size_and_frame_rate_is_printed_for_the_record() {
        tier::lab();
        let only = std::env::var("QUALITY_ENCODER").ok();
        let content_only = std::env::var("QUALITY_CONTENT").ok();
        let short_side: Option<usize> = std::env::var("QUALITY_SHORT_SIDE").ok().and_then(|side| side.parse().ok());
        let encoders: Vec<Encoder> =
            encoders().into_iter().filter(|e| only.as_deref().is_none_or(|name| e.name.as_str() == name)).collect();
        if encoders.is_empty() {
            return skipped("no encoder opens here");
        }
        for encoder in &encoders {
            for content in [Content::Motion, Content::Text]
                .into_iter()
                .filter(|content| content_only.as_deref().is_none_or(|only| format!("{content:?}") == only))
            {
                for per_second in FRAME_RATES {
                    for size in SIZES.into_iter().filter(|size| short_side.is_none_or(|side| side == size.1)) {
                        let canvas = Canvas::new(content, size.0, size.1, per_second, per_second * SECONDS);
                        let pixels = (size.0 * size.1 * per_second) as u64;
                        // Each threshold's least rate: the steady state's against the bits it sent, and each key frame's
                        // recovery against the rate it was held to.
                        let (mut steady, mut start, mut asked) = ([None; 3], [None; 3], [None; 3]);
                        for thousandths in LADDER {
                            let bits = pixels * thousandths / 1000;
                            if bits > MOST {
                                break;
                            }
                            let Some(rung) = rung(encoder, &canvas, size, per_second, bits) else { break };
                            let (held, sent) = (thousandths as f64 / 1000.0, rung.sent_bits_per_second / pixels as f64);
                            let frames = |key: usize| key as f64 * per_second as f64 / bits as f64;
                            println!(
                                "{encoder} {content:?} {}x{}@{per_second} held to {held:.3} bpp ({:.1} Mbit/s): steady {:.1} dB at {sent:.3} bpp sent ({:.1} Mbit/s), first second's worst half {:.1} dB, asked key frame's {:.1} dB; key frames {:.1} and {:.1} frames' bits",
                                size.0,
                                size.1,
                                bits as f64 / 1e6,
                                rung.steady,
                                rung.sent_bits_per_second / 1e6,
                                rung.start_worst,
                                rung.asked_worst,
                                frames(rung.key_frame_bits[0]),
                                frames(rung.key_frame_bits[1]),
                            );
                            for (at, threshold) in THRESHOLDS.iter().enumerate() {
                                let least = |was: Option<f64>, now: f64| Some(was.map_or(now, |was: f64| was.min(now)));
                                if rung.steady >= *threshold {
                                    steady[at] = least(steady[at], sent);
                                }
                                if rung.start_worst >= *threshold {
                                    start[at] = least(start[at], held);
                                }
                                if rung.asked_worst >= *threshold {
                                    asked[at] = least(asked[at], held);
                                }
                            }
                        }
                        let said = |reached: [Option<f64>; 3]| {
                            reached.map(|t| t.map_or("past the ladder".into(), |t| format!("{t:.3}"))).join(" / ")
                        };
                        println!(
                            "RECORD {} {content:?} {}p{per_second}: 30 / 35 / 40 dB steady at {} bpp sent, first second at {} bpp held, asked key frame's at {}",
                            encoder.name,
                            size.1,
                            said(steady),
                            said(start),
                            said(asked)
                        );
                    }
                }
            }
        }
    }
}
