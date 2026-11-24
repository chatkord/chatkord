//! A VA-API picture imported where it is paints the pixels of the same picture copied to memory, which says the
//! descriptor's planes, pitches and modifier were read right. No hardware encoder, VA-API decode or import skips.

use crate::common;

use common::{FORMAT, machine_where, render, skipped};
use media::{Backend, Codec, Decoder, Nv12, Path, Session, Settings, Sources, survey};
use picture::{Crop, Current, Gpu, IMPORT_FEATURES, ImportRefused, Painter, Planes, Slot, render_node};
use std::num::NonZeroU32;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
/// The same decode painted by two paths: only float rounding may differ.
const TOLERANCE: i32 = 2;

/// Studio-range BT.709 samples (Y, U, V) of red, green, blue and white.
const QUADRANTS: [(u8, u8, u8); 4] = [(63, 102, 240), (173, 42, 26), (32, 240, 118), (235, 128, 128)];

/// Four colour blocks, with a luma ramp across the top-left one so that a wrong pitch or a swapped
/// plane moves something that is checked.
fn picture() -> Nv12 {
    let mut luma = vec![0u8; WIDTH * HEIGHT];
    let mut chroma = vec![0u8; WIDTH * HEIGHT / 2];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (y_sample, ..) = QUADRANTS[usize::from(x >= WIDTH / 2) + 2 * usize::from(y >= HEIGHT / 2)];
            luma[y * WIDTH + x] =
                if x < WIDTH / 2 && y < HEIGHT / 2 { (16 + x * 200 / (WIDTH / 2)) as u8 } else { y_sample };
        }
    }
    for y in 0..HEIGHT / 2 {
        for x in 0..WIDTH / 2 {
            let (_, u, v) = QUADRANTS[usize::from(x >= WIDTH / 4) + 2 * usize::from(y >= HEIGHT / 4)];
            chroma[y * WIDTH + 2 * x] = u;
            chroma[y * WIDTH + 2 * x + 1] = v;
        }
    }
    luma.extend(chroma);
    Nv12::new(WIDTH, HEIGHT, luma).expect("a whole picture")
}

fn planes(picture: &Nv12) -> Planes {
    let (blue, red) = picture.chroma().as_chunks::<2>().0.iter().map(|[u, v]| (*u, *v)).unzip();
    Planes {
        width: WIDTH as u32,
        height: HEIGHT as u32,
        luma: picture.luma().to_vec(),
        blue,
        red,
        colours: picture.colours(),
    }
}

/// A short stream of `picture()` from this machine's encoder, or why there is none.
fn stream(frames: usize) -> Option<Vec<media::Encoded>> {
    let Some(encoder) = survey().encoders.into_iter().find(|e| e.codec() == Codec::H264) else {
        skipped("no hardware H.264 encoder opens on this machine, so there is no stream to decode");
        return None;
    };
    let settings = Settings {
        width: WIDTH,
        height: HEIGHT,
        frames_per_second: 30,
        bits_per_second: 8_000_000,
        key_frame_every: NonZeroU32::new(30),
    };
    let mut session = Session::open(&encoder, settings).expect("an encoder");
    let mut stream = Vec::new();
    for _ in 0..frames {
        stream.extend(session.encode(&picture()).expect("encode"));
    }
    stream.extend(session.finish().expect("finish"));
    Some(stream)
}

/// What this machine can do on one GPU: a decoder on its render node that keeps pictures on the GPU, and
/// a device on the same GPU that can import them. Decode and paint share a GPU, as the window's do.
fn same_gpu() -> Option<(Decoder, common::Machine)> {
    let has_decoder = |node: &str| {
        let sources = Sources { render_nodes: vec![node.to_string()], ..Sources::default() };
        Decoder::open(Codec::H264, &sources).ok().filter(|d| d.path() == Path::Hardware(Backend::VaApi))
    };
    let (machine, info) = machine_where(IMPORT_FEATURES, |info| {
        info.backend == wgpu::Backend::Vulkan && render_node(info).is_some_and(|node| has_decoder(&node).is_some())
    })?;
    let decoder = has_decoder(&render_node(&info)?)?;
    Some((decoder, machine))
}

/// The DMA-BUF's identity: the file the descriptor names, whoever opened it.
fn identity(surface: &media::Surface) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let object = surface.objects().next().expect("a surface has an object");
    let meta = std::fs::File::from(object.fd.try_clone_to_owned().expect("a duplicate descriptor"))
        .metadata()
        .expect("a DMA-BUF can be asked about");
    (meta.dev(), meta.ino())
}

mod lab {
    use super::*;

    #[test]
    fn an_imported_surface_paints_the_same_pixels_as_its_copy() {
        let Some(stream) = stream(8) else { return skipped("see above") };
        let Some((mut on_gpu, machine)) = same_gpu() else { return skipped("see above") };
        let mut copying =
            Decoder::open(Codec::H264, &Sources { vulkan: false, ..Sources::system() }).expect("a second decoder");
        let (mut surface, mut copy) = (None, None);
        for unit in &stream {
            surface = on_gpu.decode_surfaces(&unit.data, unit.pts).expect("export").pop().or(surface);
            copy = copying.decode(&unit.data, unit.pts).expect("decode").pop().or(copy);
        }
        let (surface, copy) = (surface.expect("a surface"), copy.expect("a copy"));

        // The path the window takes: the slot holds the surface, and the picture on screen is made from it.
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        slot.put_surface(surface);
        let mut current = Current::default();
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the surface waits"))
            .expect("a picture this device holds");
        assert!(slot.takes_imports(), "a surface the device can import is not refused");
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
                    assert!(worst <= TOLERANCE, "{crop:?}, pixel ({x}, {y}): the import shows {a:?}, the copy {b:?}");
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
    fn a_surface_is_the_decoders_again_once_the_gpu_is_done_with_its_texture() {
        let Some(stream) = stream(60) else { return skipped("see above") };
        let Some((mut on_gpu, machine)) = same_gpu() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let (mut pictures, mut buffers) = (0, std::collections::HashSet::new());
        let mut each = |surface: media::Surface| {
            buffers.insert(identity(&surface));
            let imported = match painter.import(&machine.device, &machine.queue, surface) {
                Ok(imported) => imported,
                Err(ImportRefused::NoDevice) => return false,
                Err(refused) => panic!("the decoder's own surface was refused: {refused:?}"),
            };
            render(&machine, (16, 16), |pass| {
                painter.draw_imported(pass, &imported, &painter.framing(&machine.device, &machine.queue))
            });
            pictures += 1;
            true
        };
        for unit in &stream {
            for surface in on_gpu.decode_surfaces(&unit.data, unit.pts).expect("export") {
                if !each(surface) {
                    return skipped("the graphics device cannot import a DMA-BUF picture");
                }
            }
        }
        // Every picture was imported, painted and dropped in turn: the decoder's pool comes round only if each
        // texture gave its surface back. A pool that was never given anything back would grow to one buffer a picture.
        assert_eq!(pictures, 60);
        assert!(
            buffers.len() <= pictures / 2,
            "{} distinct buffers for {pictures} pictures: surfaces were not given back",
            buffers.len()
        );
    }

    #[test]
    fn a_decoders_last_surface_ends_off_the_thread_that_renders() {
        let Some(stream) = stream(8) else { return skipped("see above") };
        let Some((mut on_gpu, machine)) = same_gpu() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let surface = stream
            .iter()
            .find_map(|unit| on_gpu.decode_surfaces(&unit.data, unit.pts).expect("export").pop())
            .expect("a surface");
        let imported = match painter.import(&machine.device, &machine.queue, surface) {
            Ok(imported) => imported,
            Err(ImportRefused::NoDevice) => return skipped("the graphics device cannot import a DMA-BUF picture"),
            Err(refused) => panic!("the decoder's own surface was refused: {refused:?}"),
        };
        // The surface now holds the decoder's device last, so its release ends that device, under the loader's lock.
        drop(on_gpu);
        let device = (*machine.device).clone();
        // A survey or a decoder's open holding the loader.
        let loader = media::alone();
        let (rendered, heard) = std::sync::mpsc::channel();
        // The render thread lets the picture go and has wgpu finish with it.
        std::thread::spawn(move || {
            drop(imported);
            device.poll(wgpu::PollType::wait_indefinitely()).expect("the device finishes");
            let _ = rendered.send(());
        });
        let went_on = heard.recv_timeout(std::time::Duration::from_secs(5)).is_ok();
        drop(loader);
        assert!(went_on, "the render thread waited on the loader to end the decoder's device");
    }

    #[test]
    fn a_device_that_cannot_import_tells_the_slot_and_keeps_the_picture_it_had() {
        let Some(stream) = stream(8) else { return skipped("see above") };
        let Some((mut on_gpu, _)) = same_gpu() else { return skipped("see above") };
        let Some(machine) = common::machine(wgpu::Features::empty()) else { return skipped("see above") };
        let surface = stream
            .iter()
            .find_map(|unit| on_gpu.decode_surfaces(&unit.data, unit.pts).expect("export").pop())
            .expect("a surface");
        let painter = Painter::new(&machine.device, FORMAT);
        assert!(
            matches!(painter.import(&machine.device, &machine.queue, surface), Err(ImportRefused::NoDevice)),
            "a device made without the import features says so"
        );

        let slot = Slot::new();
        let mut current = Current::default();
        let planes = Planes {
            width: 2,
            height: 2,
            luma: vec![16; 4],
            blue: vec![128],
            red: vec![128],
            colours: media::Colours::HD,
        };
        slot.put((2, 2), (&planes.luma, &planes.blue, &planes.red), (2, 1, 1), planes.colours).expect("planes");
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("planes wait"))
            .expect("a picture this device holds");
        let surface = stream
            .iter()
            .find_map(|unit| on_gpu.decode_surfaces(&unit.data, unit.pts).expect("export").pop())
            .expect("a surface");
        slot.put_surface(surface);
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the surface waits"))
            .expect("a picture this device holds");
        assert!(!slot.takes_imports(), "the producer is told to send planes");
        assert_eq!(current.size(), Some((2, 2)), "the picture on screen is still the one before");
    }
}
