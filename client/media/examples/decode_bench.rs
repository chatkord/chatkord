#![expect(
    clippy::cast_possible_wrap,
    reason = "an example's numbers are its own fixtures, bounded where it writes them"
)]
#![expect(clippy::print_stdout, clippy::print_stderr, reason = "an example's output is what it prints")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a measuring tool run by hand: a failed step ends the run"
)]
//! `decode_bench record OUT WIDTH HEIGHT FPS BITS_PER_SECOND < raw.nv12` encodes NV12 from stdin on our H.264 encoder.
//! `decode_bench play OUT FPS` times OUT, a raw `.h264` or a raw AV1 `.obu`, decoded on each backend alone, its copy
//! out included, and its first picture from the decoder's open.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::num::NonZeroU32;
use std::time::Instant;

#[cfg(target_os = "linux")]
use media::Shown;
use media::{Codec, Coded, Decoder, Nv12, Session, Settings, Sources, survey};

#[path = "../tests/suite/common/cpu.rs"]
mod cpu;
use cpu::cpu_seconds;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["record", out, width, height, fps, bits] => {
            record(out, number(width)?, number(height)?, number(fps)?, number(bits)?)
        }
        ["play", file, fps] => play(file, number::<f64>(fps)?),
        _ => Err("usage: decode_bench record OUT W H FPS BPS < raw.nv12 | decode_bench play OUT FPS".into()),
    }
}

/// A D3D12 device on the default adapter, as the window's is.
#[cfg(windows)]
fn d3d12_device() -> media::D3d12Device {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a null adapter is the default one; the out-pointer is a live `Option`.
    unsafe { D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.expect("a D3D12 device");
    media::D3d12Device(device.expect("made"))
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse().map_err(|_| format!("not a number: {text}"))
}

fn record(out: &str, width: usize, height: usize, fps: u32, bits: u32) -> Result<(), String> {
    let report = survey();
    let encoder =
        report.encoders.iter().find(|e| e.codec() == Codec::H264).ok_or("no hardware H.264 encoder opens here")?;
    let settings = Settings {
        width,
        height,
        frames_per_second: fps,
        bits_per_second: bits,
        key_frame_every: NonZeroU32::new(fps * 2),
    };
    let mut session = Session::open(encoder, settings).map_err(|e| format!("{encoder}: {e:?}"))?;
    let mut file = BufWriter::new(File::create(out).map_err(|e| e.to_string())?);
    let mut stdin = io::stdin().lock();
    let mut raw = vec![0u8; width * height * 3 / 2];
    let (mut units, mut bytes) = (0usize, 0usize);
    let mut put = |units_out: Vec<media::Encoded>, file: &mut BufWriter<File>| -> Result<(), String> {
        for unit in units_out {
            let length = u32::try_from(unit.data.len()).map_err(|_| "unit too large")?;
            file.write_all(&length.to_le_bytes())
                .and_then(|()| file.write_all(&unit.pts.to_le_bytes()))
                .and_then(|()| file.write_all(&unit.data))
                .map_err(|e| e.to_string())?;
            units += 1;
            bytes += unit.data.len();
        }
        Ok(())
    };
    while stdin.read_exact(&mut raw).is_ok() {
        let picture = Nv12::new(width, height, raw.clone()).map_err(|e| format!("{e:?}"))?;
        put(session.encode(&picture).map_err(|e| format!("{e:?}"))?, &mut file)?;
    }
    put(session.finish().map_err(|e| format!("{e:?}"))?, &mut file)?;
    file.flush().map_err(|e| e.to_string())?;
    let seconds = units as f64 / f64::from(fps);
    eprintln!("{encoder}: {units} units, {:.1} Mbit/s over {seconds:.1} s", bytes as f64 * 8.0 / seconds / 1e6);
    Ok(())
}

/// Cuts an Annex B stream into access units: a new one starts at a slice that begins a picture (`first_mb_in_slice`
/// 0, a leading 1 bit) or at the parameter sets and delimiters in front of one.
fn access_units(stream: &[u8]) -> Vec<(Vec<u8>, i64)> {
    let starts: Vec<usize> = (0..stream.len().saturating_sub(3)).filter(|&i| stream[i..i + 3] == [0, 0, 1]).collect();
    let mut all: Vec<(Vec<u8>, i64)> = Vec::new();
    let mut last_was_picture = false;
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map_or(stream.len(), |&next| if stream[next - 1] == 0 { next - 1 } else { next });
        let nal = &stream[start..end];
        let kind = nal.get(3).map_or(0, |b| b & 0x1f);
        let picture = matches!(kind, 1 | 5);
        let first_slice = picture && nal.get(4).is_some_and(|b| b & 0x80 != 0);
        let opens = if picture { first_slice && last_was_picture } else { matches!(kind, 6..=9) && last_was_picture };
        if opens || all.is_empty() {
            all.push((Vec::new(), all.len() as i64));
        }
        let unit = &mut all.last_mut().expect("a unit was just opened").0;
        unit.extend_from_slice(&[0, 0, 0, 1]);
        unit.extend_from_slice(&nal[3..]);
        last_was_picture = picture;
    }
    all
}

/// A stream's units in decode order, each with its pts.
type Units = Vec<(Vec<u8>, i64)>;

/// Cuts a stream of low-overhead AV1 OBUs into temporal units, each starting at its temporal delimiter.
fn temporal_units(stream: &[u8]) -> Units {
    const TEMPORAL_DELIMITER: u8 = 2;
    let mut all: Units = Vec::new();
    let mut at = 0;
    while let Some(&header) = stream.get(at) {
        let start = at;
        at += 1 + usize::from((header >> 2) & 1);
        let (mut size, mut shift) = (0usize, 0);
        while let Some(byte) = stream.get(at) {
            size |= usize::from(byte & 0x7f) << shift;
            (at, shift) = (at + 1, shift + 7);
            if byte & 0x80 == 0 {
                break;
            }
        }
        at = (at + size).min(stream.len());
        if (header >> 3) & 0x0f == TEMPORAL_DELIMITER || all.is_empty() {
            all.push((Vec::new(), all.len() as i64));
        }
        all.last_mut().expect("a unit was just opened").0.extend_from_slice(&stream[start..at]);
    }
    all
}

/// The codec of `file` and its units: AV1 for a `.obu`, H.264 otherwise.
fn units(file: &str) -> Result<(Codec, Units), String> {
    if file.ends_with(".obu") {
        return Ok((Codec::Av1, temporal_units(&std::fs::read(file).map_err(|e| e.to_string())?)));
    }
    if file.ends_with(".h264") {
        return Ok((Codec::H264, access_units(&std::fs::read(file).map_err(|e| e.to_string())?)));
    }
    let mut reader = BufReader::new(File::open(file).map_err(|e| e.to_string())?);
    let mut all = Vec::new();
    loop {
        let mut length = [0u8; 4];
        if reader.read_exact(&mut length).is_err() {
            return Ok((Codec::H264, all));
        }
        let mut pts = [0u8; 8];
        reader.read_exact(&mut pts).map_err(|e| e.to_string())?;
        let mut data = vec![0u8; u32::from_le_bytes(length) as usize];
        reader.read_exact(&mut data).map_err(|e| e.to_string())?;
        all.push((data, i64::from_le_bytes(pts)));
    }
}

fn play(file: &str, fps: f64) -> Result<(), String> {
    let (codec, units) = units(file)?;
    let stream: Vec<(Coded, i64)> =
        units.into_iter().map(|(data, pts)| (Coded::copied(&data).expect("a unit"), pts)).collect();
    let system = Sources::system();
    let backends = [
        ("VA-API", Sources { render_nodes: system.render_nodes.clone(), ..Sources::default() }),
        ("Vulkan", Sources { vulkan: true, ..Sources::default() }),
        #[cfg(windows)]
        ("D3D11", Sources { d3d11: true, ..Sources::default() }),
        #[cfg(windows)]
        ("D3D12", Sources { d3d12: Some(d3d12_device()), ..Sources::default() }),
        ("software", Sources::default()),
    ];
    #[cfg(target_os = "linux")]
    surfaces(codec, &stream, &backends[0].1, fps)?;
    shown_in_software(codec, &stream, fps)?;
    for (label, sources) in &backends {
        let (opening, cpu_before) = (Instant::now(), cpu_seconds());
        let mut decoder = match Decoder::open(codec, sources) {
            Ok(decoder) => decoder,
            Err(e) => {
                println!("{label}: would not open ({e:?})");
                continue;
            }
        };
        let started = Instant::now();
        let mut pictures = 0usize;
        let mut first = None;
        for (data, pts) in &stream {
            pictures += decoder.decode(data, *pts).map_err(|e| format!("{label}: {e:?}"))?.len();
            if first.is_none() && pictures > 0 {
                first = Some(opening.elapsed());
            }
        }
        pictures += decoder.finish().map_err(|e| format!("{label}: {e:?}"))?.len();
        let seconds = started.elapsed().as_secs_f64();
        let cpu = cpu_seconds().zip(cpu_before).map(|(after, before)| after - before);
        let rate = pictures as f64 / seconds;
        println!(
            "{label}: {codec:?}, asked for it, ran on {:?} (refused {:?}): first picture {:.0} ms after the open began, {pictures} pictures in {seconds:.2} s = {:.2} ms a picture, {rate:.1} pictures/s, {} the {fps} a second needed; {}",
            decoder.path(),
            decoder.refused(),
            first.unwrap_or_default().as_secs_f64() * 1000.0,
            seconds * 1000.0 / pictures.max(1) as f64,
            if rate >= fps { "keeps up with" } else { "falls behind" },
            cpu.map_or_else(
                || "CPU time not read".to_string(),
                |cpu| format!("{cpu:.1} s of CPU, {:.1} cores busy", cpu / seconds)
            ),
        );
    }
    Ok(())
}

/// A software decode as the window takes it, each picture the decoder's own planes, beside the copy `play`'s software
/// row makes.
fn shown_in_software(codec: Codec, stream: &[(Coded, i64)], fps: f64) -> Result<(), String> {
    let mut decoder = Decoder::open(codec, &Sources::default()).map_err(|e| format!("software: {e:?}"))?;
    let (started, cpu_before) = (Instant::now(), cpu_seconds());
    let mut pictures = 0usize;
    for (data, pts) in stream {
        pictures += decoder.decode_shown(data, *pts).map_err(|e| format!("software shown: {e:?}"))?.len();
    }
    let seconds = started.elapsed().as_secs_f64();
    let cpu = cpu_seconds().zip(cpu_before).map(|(after, before)| after - before);
    let rate = pictures as f64 / seconds;
    println!(
        "software, as the window takes it: {pictures} pictures in {seconds:.2} s = {:.2} ms a picture, {rate:.1} pictures/s, {} the {fps} a second needed; {}",
        seconds * 1000.0 / pictures.max(1) as f64,
        if rate >= fps { "keeps up with" } else { "falls behind" },
        cpu.map_or_else(
            || "CPU time not read".to_string(),
            |cpu| format!("{cpu:.1} s of CPU, {:.1} cores busy", cpu / seconds)
        ),
    );
    Ok(())
}

/// The same VA-API decode as the window takes it: each picture left on the GPU as DMA-BUF planes while the path
/// allows, copied once a fallback took another.
#[cfg(target_os = "linux")]
fn surfaces(codec: Codec, stream: &[(Coded, i64)], vaapi: &Sources, fps: f64) -> Result<(), String> {
    if let Ok(mut decoder) = Decoder::open(codec, vaapi)
        && decoder.path() == media::Path::Hardware(media::Backend::VaApi)
    {
        let started = Instant::now();
        let (mut on_gpu, mut copied) = (0usize, 0usize);
        for (data, pts) in stream {
            let shown = decoder.decode_shown(data, *pts).map_err(|e| format!("VA-API surfaces: {e:?}"))?;
            for picture in shown {
                match picture {
                    Shown::OnGpu(_) => on_gpu += 1,
                    Shown::InMemory(_) => copied += 1,
                }
            }
        }
        if decoder.path().leaves_on_gpu() {
            on_gpu += decoder.finish_surfaces().map_err(|e| format!("VA-API surfaces: {e:?}"))?.len();
        } else {
            copied += decoder.finish().map_err(|e| format!("VA-API surfaces: {e:?}"))?.len();
        }
        let seconds = started.elapsed().as_secs_f64();
        let pictures = on_gpu + copied;
        let rate = pictures as f64 / seconds;
        println!(
            "VA-API surfaces: {on_gpu} pictures left on the GPU, {copied} copied after it went on {:?} (refused {:?}), in {seconds:.2} s = {:.2} ms a picture, {rate:.1} pictures/s, {} the {fps} a second needed",
            decoder.path(),
            decoder.refused(),
            seconds * 1000.0 / pictures.max(1) as f64,
            if rate >= fps { "keeps up with" } else { "falls behind" },
        );
    }
    Ok(())
}
