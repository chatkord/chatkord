//! Pictures from somewhere that is not the network, for the window and its tests: one decoded surface of a gradient, a
//! recorded 4K stream (`record`) and its endless playback at 60 pictures a second (`play`). No window toolkit in here.

use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant as Clock};

use media::{Codec, Decoder, Nv12, Session, Settings, Sources, Surface, survey};
use picture::Slot;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

/// Pictures the decoder has made, counted where it makes them.
pub(crate) static DECODED: AtomicU64 = AtomicU64::new(0);

/// One decoded surface from this machine's encoder and a decoder on `node`, or why not.
pub(crate) fn one_surface(node: &str) -> Result<Surface, String> {
    let encoder =
        survey().encoders.into_iter().find(|e| e.codec() == Codec::H264).ok_or("no hardware H.264 encoder")?;
    let settings = Settings {
        width: WIDTH,
        height: HEIGHT,
        frames_per_second: 30,
        bits_per_second: 8_000_000,
        key_frame_every: NonZeroU32::new(30),
    };
    let mut session = Session::open(&encoder, settings).map_err(|e| format!("{e:?}"))?;
    let mut data = vec![128u8; WIDTH * HEIGHT * 3 / 2];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            data[y * WIDTH + x] = (x * 255 / WIDTH) as u8;
        }
    }
    for row in 0..HEIGHT / 2 {
        for pair in 0..WIDTH / 2 {
            let at = WIDTH * HEIGHT + row * WIDTH + pair * 2;
            data[at] = (row * 255 / (HEIGHT / 2)) as u8;
            data[at + 1] = (255 - pair * 255 / (WIDTH / 2)) as u8;
        }
    }
    let picture = Nv12::new(WIDTH, HEIGHT, data).map_err(|e| format!("{e:?}"))?;
    let mut units = Vec::new();
    for _ in 0..8 {
        units.extend(session.encode(&picture).map_err(|e| format!("{e:?}"))?);
    }
    units.extend(session.finish().map_err(|e| format!("{e:?}"))?);
    let sources = Sources { render_nodes: vec![node.to_string()], ..Sources::default() };
    let mut decoder = Decoder::open(Codec::H264, &sources).map_err(|e| format!("{e:?}"))?;
    let mut last = None;
    for unit in &units {
        last = decoder.decode_surfaces(&unit.data, unit.pts).map_err(|e| format!("{e:?}"))?.pop().or(last);
    }
    last.ok_or_else(|| "the decoder gave no surface".into())
}

/// `SPIKE_RECORD=file`: encode a few seconds of moving 4K texture into `file` as length-prefixed units.
pub(crate) fn record(path: &str) -> Result<(), String> {
    let (width, height) = (3840usize, 2160usize);
    let encoder =
        survey().encoders.into_iter().find(|e| e.codec() == Codec::H264).ok_or("no hardware H.264 encoder")?;
    let settings = Settings {
        width,
        height,
        frames_per_second: 60,
        bits_per_second: 25_000_000,
        key_frame_every: NonZeroU32::new(120),
    };
    let mut session = Session::open(&encoder, settings).map_err(|e| format!("{e:?}"))?;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    let mut put = |units: Vec<media::Encoded>| -> Result<(), String> {
        for unit in units {
            let length = u32::try_from(unit.data.len()).map_err(|_| "unit too large")?;
            out.write_all(&length.to_le_bytes())
                .and_then(|()| out.write_all(&unit.pts.to_le_bytes()))
                .and_then(|()| out.write_all(&unit.data))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    for frame in 0..240usize {
        let mut data = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                noise ^= noise << 13;
                noise ^= noise >> 7;
                noise ^= noise << 17;
                let stripe = ((x + y + frame * 24) / 48 % 2) as u8 * 60;
                data[y * width + x] = (x * 120 / width) as u8 + stripe + (noise & 15) as u8;
            }
        }
        for row in 0..height / 2 {
            for pair in 0..width / 2 {
                let at = width * height + row * width + pair * 2;
                data[at] = ((row + frame) * 255 / (height / 2)) as u8;
                data[at + 1] = (255 - pair * 255 / (width / 2)) as u8;
            }
        }
        put(session
            .encode(&Nv12::new(width, height, data).map_err(|e| format!("{e:?}"))?)
            .map_err(|e| format!("{e:?}"))?)?;
    }
    put(session.finish().map_err(|e| format!("{e:?}"))?)
}

pub(crate) fn units(path: &str) -> Result<Vec<(media::Coded, i64)>, String> {
    let mut input = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    let mut all = Vec::new();
    loop {
        let mut length = [0u8; 4];
        if input.read_exact(&mut length).is_err() {
            return Ok(all);
        }
        let mut pts = [0u8; 8];
        input.read_exact(&mut pts).map_err(|e| e.to_string())?;
        let mut data = vec![0u8; u32::from_le_bytes(length) as usize];
        input.read_exact(&mut data).map_err(|e| e.to_string())?;
        all.push((media::Coded::copied(&data).map_err(|e| e.to_string())?, i64::from_le_bytes(pts)));
    }
}

/// `SPIKE_STREAM=file`: decode `file` over and over at 60 pictures a second on `node` into `slot`, where a picture
/// nobody took yet is replaced by the next and so goes back to the decoder.
pub(crate) fn play(path: &str, node: &str, slot: Slot) -> Result<(), String> {
    let stream = units(path)?;
    let sources = Sources { render_nodes: vec![node.to_string()], ..Sources::default() };
    std::thread::spawn(move || {
        let tick = Duration::from_micros(16_667);
        let mut next = Clock::now();
        loop {
            let Ok(mut decoder) = Decoder::open(Codec::H264, &sources) else { return };
            for (data, pts) in &stream {
                let Ok(surfaces) = decoder.decode_surfaces(data, *pts) else { return };
                for surface in surfaces {
                    DECODED.fetch_add(1, Ordering::Relaxed);
                    slot.put_surface(surface);
                    next += tick;
                    std::thread::sleep(next.saturating_duration_since(Clock::now()));
                }
            }
        }
    });
    Ok(())
}
