//! A picture decoded onto the window's D3D12 device, adopted from the resource the decoder wrote, paints the pixels of
//! the stream decoded in software. No hardware H.264 encoder, D3D12 adapter or D3D12 decode skips.

use crate::common;

use common::{Checked, FORMAT, Machine, machine_where, render, skipped};
use media::{Backend, Codec, Decoder, Nv12, Path, Session, Settings, Sources, survey};
use picture::{Crop, Current, Gpu, IMPORT_FEATURES, OnWindow, Painter, Painting, Planes, Slot, import};
use std::collections::HashSet;
use std::num::NonZeroU32;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::core::{IUnknown, Interface};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
/// The same decode painted by two paths: only float rounding may differ.
const TOLERANCE: i32 = 2;

/// Studio-range BT.709 samples (Y, U, V) of red, green, blue and white.
const QUADRANTS: [(u8, u8, u8); 4] = [(63, 102, 240), (173, 42, 26), (32, 240, 118), (235, 128, 128)];

/// Four colour blocks at `width` by `height`, with a luma ramp across the top-left one so that a wrong plane or pitch
/// moves what is checked.
fn picture_of(width: usize, height: usize) -> Nv12 {
    let mut luma = vec![0u8; width * height];
    let mut chroma = vec![0u8; width * height / 2];
    for y in 0..height {
        for x in 0..width {
            let (y_sample, ..) = QUADRANTS[usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2)];
            luma[y * width + x] =
                if x < width / 2 && y < height / 2 { (16 + x * 200 / (width / 2)) as u8 } else { y_sample };
        }
    }
    for y in 0..height / 2 {
        for x in 0..width / 2 {
            let (_, u, v) = QUADRANTS[usize::from(x >= width / 4) + 2 * usize::from(y >= height / 4)];
            chroma[y * width + 2 * x] = u;
            chroma[y * width + 2 * x + 1] = v;
        }
    }
    luma.extend(chroma);
    Nv12::new(width, height, luma).expect("a whole picture")
}

fn planes(picture: &Nv12) -> Planes {
    planes_of(picture, (WIDTH as u32, HEIGHT as u32))
}

/// `picture`'s planes, `size` wide and high, as the painter takes them from memory.
fn planes_of(picture: &Nv12, (width, height): (u32, u32)) -> Planes {
    let (blue, red) = picture.chroma().as_chunks::<2>().0.iter().map(|[u, v]| (*u, *v)).unzip();
    Planes { width, height, luma: picture.luma().to_vec(), blue, red, colours: picture.colours() }
}

/// A short stream of [`picture_of`] at [`WIDTH`] by [`HEIGHT`] from this machine's encoder, or why there is none.
fn stream(frames: usize) -> Option<Vec<media::Encoded>> {
    stream_of(WIDTH, HEIGHT, frames)
}

/// A short stream of [`picture_of`] `width` by `height` from this machine's encoder, or why there is none.
fn stream_of(width: usize, height: usize, frames: usize) -> Option<Vec<media::Encoded>> {
    let Some(encoder) = survey().encoders.into_iter().find(|e| e.codec() == Codec::H264) else {
        skipped("no hardware H.264 encoder opens on this machine, so there is no stream to decode");
        return None;
    };
    let settings = Settings {
        width,
        height,
        frames_per_second: 30,
        bits_per_second: 8_000_000,
        key_frame_every: NonZeroU32::new(30),
    };
    let mut session = Session::open(&encoder, settings).expect("an encoder");
    let mut stream = Vec::new();
    for _ in 0..frames {
        stream.extend(session.encode(&picture_of(width, height)).expect("encode"));
    }
    stream.extend(session.finish().expect("finish"));
    Some(stream)
}

/// A D3D12 device as the window makes it, and a decoder handed it, as the window's call hands it.
fn window_and_decoder() -> Option<(Machine, Decoder, Checked)> {
    let (machine, _) = machine_where(IMPORT_FEATURES, |info| info.backend == wgpu::Backend::Dx12)?;
    let painting = Painting::of(&machine.device, None);
    let checked = Checked { gpu: None, window: painting.device().expect("a D3D12 window").clone() };
    let decoder = Decoder::open(Codec::H264, &painting.sources()).expect("a decoder");
    if decoder.path() != Path::Hardware(Backend::D3d12) {
        skipped(format!("this GPU does not decode H.264 on D3D12 (refused {:?})", decoder.refused()));
        return None;
    }
    Some((machine, decoder, checked))
}

mod lab {
    use super::*;

    #[test]
    fn a_picture_decoded_on_the_windows_device_paints_the_same_pixels_as_its_copy() {
        let Some(stream) = stream(8) else { return skipped("see above") };
        let Some((machine, mut on_gpu, _checked)) = window_and_decoder() else { return skipped("see above") };
        let mut copying = Decoder::open(Codec::H264, &Sources::default()).expect("a software decoder");
        let (mut surface, mut copy) = (None, None);
        for unit in &stream {
            surface = on_gpu.decode_d3d12(&unit.data, unit.pts).expect("decode").pop().or(surface);
            copy = copying.decode(&unit.data, unit.pts).expect("decode").pop().or(copy);
        }
        let (surface, copy) = (surface.expect("a picture"), copy.expect("a copy"));

        // The path the window takes: the slot holds the picture, and the picture on screen is made from it.
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        slot.put_d3d12(surface);
        let mut current = Current::default();
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the picture waits"))
            .expect("a picture this device holds");
        assert!(slot.takes_imports(), "a picture on the window's own device is not refused");
        assert_eq!(current.size(), Some((WIDTH as u32, HEIGHT as u32)));
        let mut gpu =
            Gpu::new(&machine.device, &painter, (WIDTH as u32, HEIGHT as u32)).expect("a texture this device holds");
        let copied = planes(&copy.picture);
        gpu.upload(&machine.queue, copied.planes(), copied.colours);

        // The whole picture, then the part of it a zoom shows: both paths frame it the same way.
        let mut framing = painter.framing(&machine.device, &machine.queue);
        let mut whole = None;
        for crop in [Crop::WHOLE, Crop { left: 0.25, top: 0.125, width: 0.5, height: 0.5 }] {
            framing.set(&machine.queue, crop);
            let size = (WIDTH as u32, HEIGHT as u32);
            let from_gpu = render(&machine, size, |pass| current.draw(&painter, pass, &framing));
            let from_memory = render(&machine, size, |pass| painter.draw(pass, &gpu, &framing));
            let mut worst = 0;
            for (y, (a, b)) in from_gpu.iter().zip(&from_memory).enumerate() {
                for (x, (a, b)) in a.iter().zip(b).enumerate() {
                    for channel in 0..4 {
                        worst = worst.max((i32::from(a[channel]) - i32::from(b[channel])).abs());
                    }
                    assert!(
                        worst <= TOLERANCE,
                        "{crop:?}, pixel ({x}, {y}): the adopted picture shows {a:?}, the copy {b:?}"
                    );
                }
            }
            whole.get_or_insert(from_gpu);
        }
        let from_gpu = whole.expect("the whole picture was drawn");
        // Both paths could be wrong the same way; the corners of the colour blocks are far from any ramp.
        let corner = |x: usize, y: usize| from_gpu[y][x];
        assert!(corner(WIDTH - 40, 40)[1] > 200 && corner(WIDTH - 40, 40)[0] < 60, "top right is green");
        assert!(corner(40, HEIGHT - 40)[2] > 200 && corner(40, HEIGHT - 40)[0] < 60, "bottom left is blue");
        assert!(corner(WIDTH - 40, HEIGHT - 40).iter().take(3).all(|c| *c > 230), "bottom right is white");
    }

    #[test]
    fn the_texture_painted_is_the_resource_the_decoder_wrote_not_a_copy_of_it() {
        let Some(stream) = stream(4) else { return skipped("see above") };
        let Some((machine, mut on_gpu, _checked)) = window_and_decoder() else { return skipped("see above") };
        let surface = stream
            .iter()
            .find_map(|unit| on_gpu.decode_d3d12(&unit.data, unit.pts).expect("decode").pop())
            .expect("a picture");
        let written = surface.resource().expect("the decoder's resource").as_raw();
        let (texture, _held) = import(&machine.device, OnWindow::Decoded(surface)).expect("adopted");
        // SAFETY: the texture is a D3D12 one of this device.
        let hal = unsafe { texture.as_hal::<wgpu::hal::api::Dx12>() }.expect("a D3D12 texture");
        // SAFETY: only its resource's address is read.
        let painted = unsafe { hal.raw_resource() }.as_raw();
        assert_eq!(painted, written, "one resource: the decoder's memory is what the window reads");
    }

    #[test]
    fn a_picture_is_the_decoders_again_once_the_window_is_done_with_it() {
        let Some(stream) = stream(60) else { return skipped("see above") };
        let Some((machine, mut on_gpu, _checked)) = window_and_decoder() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let (mut pictures, mut resources) = (0, HashSet::new());
        for unit in &stream {
            for surface in on_gpu.decode_d3d12(&unit.data, unit.pts).expect("decode") {
                resources.insert(surface.resource().expect("the decoder's resource").as_raw() as usize);
                let colours = surface.colours();
                let (texture, held) = import(&machine.device, OnWindow::Decoded(surface)).expect("adopted");
                let size = (WIDTH as u32, HEIGHT as u32);
                let imported =
                    painter.bind(&machine.device, texture, colours, size).expect("a device that adopted can draw it");
                render(&machine, (16, 16), |pass| {
                    painter.draw_imported(pass, &imported, &painter.framing(&machine.device, &machine.queue))
                });
                // Given back after the reads just submitted; the decoder waits for them before it writes this one anew.
                drop(held);
                pictures += 1;
            }
        }
        // Every picture was adopted, painted and given back in turn: the decoder carried on, and its pool came round
        // rather than growing to one resource a picture.
        assert_eq!(pictures, 60);
        assert!(
            resources.len() <= pictures / 2,
            "{} resources for {pictures} pictures: none came round",
            resources.len()
        );
    }

    /// A window that has shown a stream of `pictures` pictures, each taken from the slot as the window takes them.
    struct Shown {
        current: Current,
        _slot: Slot,
        _painter: Painter,
        _decoder: Decoder,
        _machine: Machine,
        resources: HashSet<usize>,
        pictures: usize,
        _checked: Checked,
    }

    fn shown(pictures: usize) -> Option<Shown> {
        let stream = stream(pictures)?;
        let (machine, mut decoder, checked) = window_and_decoder()?;
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        let (mut count, mut resources) = (0, HashSet::new());
        for unit in &stream {
            for surface in decoder.decode_d3d12(&unit.data, unit.pts).expect("decode") {
                resources.insert(surface.resource().expect("the decoder's resource").as_raw() as usize);
                slot.put_d3d12(surface);
                let frame = slot.take().expect("the picture waits");
                current
                    .show(&machine.device, &machine.queue, &painter, &slot, frame)
                    .expect("a picture this device holds");
                count += 1;
            }
        }
        Some(Shown {
            _machine: machine,
            _decoder: decoder,
            current,
            _slot: slot,
            _painter: painter,
            resources,
            pictures: count,
            _checked: checked,
        })
    }

    /// The count of references to `resource`, its own among them.
    fn references(resource: &ID3D12Resource) -> u32 {
        let raw = resource.as_raw();
        // SAFETY: `raw` is the live resource's own pointer, borrowed for the calls below.
        let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) }.expect("a resource");
        let table = Interface::vtable(unknown);
        // SAFETY: the count is raised on a live resource ...
        unsafe { (table.AddRef)(raw) };
        // ... and lowered again at once; what it returns is the count as it was.
        // SAFETY: as above.
        unsafe { (table.Release)(raw) }
    }

    #[test]
    fn a_resource_is_imported_once_however_often_the_decoder_lends_it() {
        let Some(shown) = shown(60) else { return skipped("see above") };
        let imports = shown.current.imports();
        assert!(shown.pictures > shown.resources.len(), "the pool came round");
        assert_eq!(
            imports.made(),
            shown.resources.len(),
            "one import for each resource of the pool, not for each picture"
        );
        assert_eq!(imports.kept(), imports.made());
    }

    #[test]
    fn imports_go_with_the_decoder_that_lent_them_and_give_the_pool_its_resources_back() {
        let Some(stream) = stream(30) else { return skipped("see above") };
        let Some((machine, mut decoder, _checked)) = window_and_decoder() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        let mut witness = None;
        for unit in &stream {
            for surface in decoder.decode_d3d12(&unit.data, unit.pts).expect("decode") {
                witness.get_or_insert_with(|| surface.resource().expect("the decoder's resource"));
                slot.put_d3d12(surface);
                let frame = slot.take().expect("the picture waits");
                current
                    .show(&machine.device, &machine.queue, &painter, &slot, frame)
                    .expect("a picture this device holds");
            }
        }
        let witness = witness.expect("a picture");
        assert!(current.imports().kept() > 0);
        assert!(references(&witness) > 2, "the pool, the witness and the window's import hold the resource");

        drop(decoder);
        assert!(current.imports().kept() > 0, "the picture on screen is still lent, so its pool is still lent from");
        let planes = vec![0; 16 * 16];
        slot.put_nv12((16, 16), &planes, &planes[..16 * 8], media::Colours::HD).expect("a whole picture");
        let frame = slot.take().expect("the picture waits");
        current.show(&machine.device, &machine.queue, &painter, &slot, frame).expect("planes are held");
        assert_eq!(current.imports().kept(), 0, "no decoder lends from the pool, so nothing of it is kept");
        assert_eq!(references(&witness), 1, "only the witness holds the resource: the window's references are gone");
    }

    /// The size a stream shrinks to: not a whole number of macroblocks high, so the decoder's pool is taller than its
    /// pictures, as a pool made for a larger picture is.
    const SHRUNK: (usize, usize) = (1008, 562);

    #[test]
    fn a_stream_that_shrinks_is_drawn_at_its_pictures_size_from_its_own_rows() {
        let Some(mut units) = stream(4) else { return skipped("see above") };
        let Some(shrunk) = stream_of(SHRUNK.0, SHRUNK.1, 4) else { return skipped("see above") };
        units.extend(shrunk);
        let Some((machine, mut on_gpu, _checked)) = window_and_decoder() else { return skipped("see above") };
        let mut copying = Decoder::open(Codec::H264, &Sources::default()).expect("a software decoder");
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        let mut copy = None;
        for unit in &units {
            for surface in on_gpu.decode_d3d12(&unit.data, unit.pts).expect("decode") {
                slot.put_d3d12(surface);
                let frame = slot.take().expect("the picture waits");
                current
                    .show(&machine.device, &machine.queue, &painter, &slot, frame)
                    .expect("a picture this device holds");
            }
            copy = copying.decode(&unit.data, unit.pts).expect("decode").pop().or(copy);
        }
        let size = (SHRUNK.0 as u32, SHRUNK.1 as u32);
        assert_eq!(current.size(), Some(size), "the picture's own size, not its resource's");
        let copy = copy.expect("a copy");
        let mut gpu = Gpu::new(&machine.device, &painter, size).expect("a texture this device holds");
        let copied = planes_of(&copy.picture, size);
        gpu.upload(&machine.queue, copied.planes(), copied.colours);
        let framing = painter.framing(&machine.device, &machine.queue);
        let from_gpu = render(&machine, size, |pass| current.draw(&painter, pass, &framing));
        let from_memory = render(&machine, size, |pass| painter.draw(pass, &gpu, &framing));
        for (y, (a, b)) in from_gpu.iter().zip(&from_memory).enumerate() {
            for (x, (a, b)) in a.iter().zip(b).enumerate() {
                let apart = (0..4).map(|channel| (i32::from(a[channel]) - i32::from(b[channel])).abs()).max();
                assert!(
                    apart.unwrap_or_default() <= TOLERANCE,
                    "pixel ({x}, {y}): the adopted picture shows {a:?}, the copy {b:?}: rows the picture did not write"
                );
            }
        }
        let last = SHRUNK.1 - 1;
        let at = |x: usize| from_gpu[last][x];
        assert!(at(40)[2] > 200 && at(40)[0] < 60, "the last row's left is the picture's blue");
        assert!(at(SHRUNK.0 - 40).iter().take(3).all(|c| *c > 230), "the last row's right is the picture's white");
    }
}
