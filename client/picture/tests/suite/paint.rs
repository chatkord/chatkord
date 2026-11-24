//! The planes of a picture painted offscreen and read back, so the colour math is checked without
//! a window. A machine with no graphics adapter says so and skips; it proves nothing here.

use crate::common;

use media::{Colours, Matrix, Range};
use picture::{Crop, Current, Framing, Gpu, Painter, Planes, Slot, TooLarge};

use common::{FORMAT, Machine, machine, machine_where, render, skipped};
/// Rounding in the GPU's float math, and the integer limits of the studio-range values below.
const TOLERANCE: i32 = 4;

/// Paint the part of `picture` `framing` shows into a target of its own size and return its pixels as rows of RGBA.
fn paint_framed(
    machine: &Machine,
    painter: &Painter,
    gpu: &mut Gpu,
    picture: &Planes,
    framing: &Framing,
) -> Vec<Vec<[u8; 4]>> {
    gpu.upload(&machine.queue, picture.planes(), picture.colours);
    let gpu = &*gpu;
    render(machine, gpu.size(), |pass| painter.draw(pass, gpu, framing))
}

/// Paint all of `picture` into a target of its own size and return its pixels as rows of RGBA.
fn paint(machine: &Machine, painter: &Painter, gpu: &mut Gpu, picture: &Planes) -> Vec<Vec<[u8; 4]>> {
    paint_framed(machine, painter, gpu, picture, &painter.framing(&machine.device, &machine.queue))
}

fn near(got: [u8; 4], want: [u8; 3], what: &str) {
    for (channel, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (i32::from(*g) - i32::from(w)).abs() <= TOLERANCE,
            "{what}: channel {channel} is {g}, wanted {w} (pixel {got:?})"
        );
    }
    assert_eq!(got[3], 255, "{what}: a picture is opaque");
}

/// 16x16: red, green / blue, white, a quadrant each. The samples are the BT.709 studio-range values of those
/// colours, taken from the standard and not from the shader.
fn quadrants() -> Planes {
    let (red, green, blue, white) = ((63, 102, 240), (173, 42, 26), (32, 240, 118), (235, 128, 128));
    let quadrants = [[red, green], [blue, white]];
    let mut picture = flat((16, 16), white);
    for y in 0..16usize {
        for x in 0..16usize {
            picture.luma[y * 16 + x] = quadrants[y / 8][x / 8].0;
        }
    }
    for y in 0..8usize {
        for x in 0..8usize {
            picture.blue[y * 8 + x] = quadrants[y / 4][x / 4].1;
            picture.red[y * 8 + x] = quadrants[y / 4][x / 4].2;
        }
    }
    picture
}

/// Mid grey, white and black in studio range, as Y, U and V.
const GREY: (u8, u8, u8) = (126, 128, 128);
const WHITE: (u8, u8, u8) = (235, 128, 128);
const BLACK: (u8, u8, u8) = (16, 128, 128);

/// A picture of one flat colour in BT.709 studio range, given as its Y, U and V sample values.
fn flat(size: (u32, u32), yuv: (u8, u8, u8)) -> Planes {
    flat_in(size, yuv, Colours::HD)
}

/// A picture of one flat colour, given as its Y, U and V sample values in `colours`.
fn flat_in((width, height): (u32, u32), (y, u, v): (u8, u8, u8), colours: Colours) -> Planes {
    let (half_width, half_height) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
    Planes {
        width,
        height,
        luma: vec![y; (width * height) as usize],
        blue: vec![u; half_width * half_height],
        red: vec![v; half_width * half_height],
        colours,
    }
}

mod lab {
    use super::*;

    #[test]
    fn four_known_colours_are_painted_where_they_were_put() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let mut gpu = Gpu::new(&machine.device, &painter, (16, 16)).expect("a texture this device holds");
        let pixels = paint(&machine, &painter, &mut gpu, &quadrants());
        // A pixel well inside each quadrant, away from the blend of its neighbours' chroma.
        near(pixels[3][3], [255, 0, 0], "top left is red");
        near(pixels[3][11], [0, 255, 0], "top right is green");
        near(pixels[11][3], [0, 0, 255], "bottom left is blue");
        near(pixels[11][11], [255, 255, 255], "bottom right is white");
    }

    #[test]
    fn the_same_colours_as_nv12_in_memory_are_painted_where_they_were_put() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let planes = quadrants();
        let chroma: Vec<u8> = planes.blue.iter().zip(&planes.red).flat_map(|(blue, red)| [*blue, *red]).collect();
        // Rows padded past the picture, as a decoder's are.
        const STRIDES: (u32, u32) = (20, 24);
        let (stride, chroma_stride) = (STRIDES.0 as usize, STRIDES.1 as usize);
        let mut luma = vec![0; stride * 16];
        let mut interleaved = vec![0; chroma_stride * 8];
        for row in 0..16 {
            luma[row * stride..row * stride + 16].copy_from_slice(&planes.luma[row * 16..row * 16 + 16]);
        }
        for row in 0..8 {
            interleaved[row * chroma_stride..row * chroma_stride + 16]
                .copy_from_slice(&chroma[row * 16..row * 16 + 16]);
        }
        let mut gpu = Gpu::of_layout(&machine.device, &painter, (16, 16), media::PictureLayout::Nv12)
            .expect("a texture this device holds");
        let samples = media::Samples::Nv12([(luma.as_slice(), STRIDES.0), (interleaved.as_slice(), STRIDES.1)]);
        gpu.upload_samples(&machine.queue, samples, Colours::HD);
        let framing = painter.framing(&machine.device, &machine.queue);
        let gpu = &gpu;
        let pixels = render(&machine, gpu.size(), |pass| painter.draw(pass, gpu, &framing));
        near(pixels[3][3], [255, 0, 0], "top left is red");
        near(pixels[3][11], [0, 255, 0], "top right is green");
        near(pixels[11][3], [0, 0, 255], "bottom left is blue");
        near(pixels[11][11], [255, 255, 255], "bottom right is white");
    }

    #[test]
    fn a_framed_picture_paints_only_its_window() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let mut gpu = Gpu::new(&machine.device, &painter, (16, 16)).expect("a texture this device holds");
        let mut framing = painter.framing(&machine.device, &machine.queue);
        framing.set(&machine.queue, Crop { left: 0.0, top: 0.0, width: 0.5, height: 0.5 });
        let top_left = paint_framed(&machine, &painter, &mut gpu, &quadrants(), &framing);
        // The top left quadrant over the whole target, so even its far corner, where the whole picture is white, is
        // red; the half-size chroma blends with a neighbour only near the window's inner edges.
        for (y, x) in [(1, 1), (1, 13), (13, 1), (13, 13), (7, 7)] {
            near(top_left[y][x], [255, 0, 0], &format!("({x}, {y}) of the top left window"));
        }
        framing.set(&machine.queue, Crop { left: 0.5, top: 0.5, width: 0.5, height: 0.5 });
        let bottom_right = paint_framed(&machine, &painter, &mut gpu, &quadrants(), &framing);
        for (y, x) in [(5, 5), (13, 13)] {
            near(bottom_right[y][x], [255, 255, 255], &format!("({x}, {y}) of the bottom right window"));
        }
    }

    /// A 4K page of fine detail shown on a tile a tenth of its size is the mean of the texels each pixel stands for,
    /// where one bilinear sample of four of them would alias: mean absolute error against a box filter, in the share
    /// of white.
    #[test]
    fn a_picture_drawn_a_tenth_of_its_size_is_the_mean_of_the_texels_each_pixel_stands_for() {
        const SOURCE: (u32, u32) = (3840, 2160);
        const SHRUNK: (u32, u32) = (384, 216);
        const FACTOR: usize = 10;
        const MOST_ERROR: f64 = 0.1;
        // Texels of black and white that no two neighbours share a rule for, as the strokes of small text are.
        fn white(x: usize, y: usize) -> bool {
            let mut hash = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
            hash ^= hash >> 15;
            hash = hash.wrapping_mul(0x2C1B_3C6D);
            hash ^= hash >> 13;
            hash & 0x10 != 0
        }
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let Ok(mut gpu) = Gpu::new(&machine.device, &painter, SOURCE) else {
            return skipped("the device holds no 4K texture");
        };
        let mut picture = flat(SOURCE, GREY);
        for y in 0..SOURCE.1 as usize {
            for x in 0..SOURCE.0 as usize {
                picture.luma[y * SOURCE.0 as usize + x] = if white(x, y) { WHITE.0 } else { BLACK.0 };
            }
        }
        gpu.upload(&machine.queue, picture.planes(), picture.colours);
        let framing = painter.framing(&machine.device, &machine.queue);
        let gpu = &gpu;
        let pixels = render(&machine, SHRUNK, |pass| painter.draw(pass, gpu, &framing));
        let mut total = 0.0;
        for (row, line) in pixels.iter().enumerate() {
            for (column, pixel) in line.iter().enumerate() {
                let whites = (0..FACTOR)
                    .flat_map(|dy| (0..FACTOR).map(move |dx| (column * FACTOR + dx, row * FACTOR + dy)))
                    .filter(|&(x, y)| white(x, y))
                    .count();
                let want = whites as f64 / (FACTOR * FACTOR) as f64;
                total += (f64::from(pixel[0]) / 255.0 - want).abs();
            }
        }
        let error = total / f64::from(SHRUNK.0 * SHRUNK.1);
        eprintln!("a 3840x2160 page at 384x216: mean absolute error {error:.4} against the box filter");
        assert!(error < MOST_ERROR, "the picture aliases: mean absolute error {error:.4}");
    }

    #[test]
    fn a_picture_larger_than_the_device_holds_is_refused_not_a_panic() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let max = machine.device.limits().max_texture_dimension_2d;
        assert_eq!(Gpu::new(&machine.device, &painter, (max + 1, 500)).err(), Some(TooLarge), "{}x500", max + 1);
        assert_eq!(Gpu::new(&machine.device, &painter, (0, 500)).err(), Some(TooLarge));
        assert!(Gpu::new(&machine.device, &painter, (16, 16)).is_ok());
    }

    /// Fails with wgpu's buffer helper, which panics on a lost device: a window then crashed where it rebuilds.
    #[test]
    fn a_picture_bound_once_the_device_is_lost_is_refused_not_a_panic() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let texture = machine.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("a picture"),
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        machine.device.destroy();
        assert!(painter.bind_rgb(&machine.device, texture, (16, 16)).is_none(), "refused on a lost device");
    }

    #[test]
    fn an_odd_sized_picture_is_painted_whole_and_a_later_one_replaces_it() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let mut gpu = Gpu::new(&machine.device, &painter, (5, 3)).expect("a texture this device holds");
        let grey = paint(&machine, &painter, &mut gpu, &flat((5, 3), GREY));
        assert_eq!((grey.len(), grey[0].len()), (3, 5), "the target is the picture's size");
        for row in &grey {
            for pixel in row {
                near(*pixel, [128, 128, 128], "mid grey everywhere");
            }
        }
        let white = paint(&machine, &painter, &mut gpu, &flat((5, 3), WHITE));
        near(white[2][4], [255, 255, 255], "the last pixel of the second picture");
        let black = paint(&machine, &painter, &mut gpu, &flat((5, 3), BLACK));
        near(black[0][0], [0, 0, 0], "the first pixel of the third picture");
    }

    /// A pure red as its samples are in each matrix and range, taken from the standards: BT.601's limited and full,
    /// and BT.709's limited.
    const RED_601: (u8, u8, u8) = (81, 90, 240);
    const RED_601_FULL: (u8, u8, u8) = (76, 85, 255);
    const RED_709: (u8, u8, u8) = (63, 102, 240);
    const CAMERA: Colours = Colours { matrix: Matrix::Bt601, range: Range::Limited };
    const CAMERA_FULL: Colours = Colours { matrix: Matrix::Bt601, range: Range::Full };

    #[test]
    fn a_picture_is_painted_by_the_matrix_and_range_it_carries() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let mut gpu = Gpu::new(&machine.device, &painter, (16, 16)).expect("a texture this device holds");
        let mut painted = |yuv, colours| paint(&machine, &painter, &mut gpu, &flat_in((16, 16), yuv, colours))[8][8];
        near(painted(RED_601, CAMERA), [255, 0, 0], "a camera's red in BT.601");
        near(painted(RED_709, Colours::HD), [255, 0, 0], "a screen's red in BT.709, on the same textures");
        near(painted(RED_601_FULL, CAMERA_FULL), [255, 0, 0], "a camera's red in full range");
        near(painted(RED_601, CAMERA), [255, 0, 0], "and back, the buffer written again for each change");
        let leaning = painted(RED_601, Colours::HD);
        assert!(leaning[1] > 12, "a camera's red read as BT.709 is orange, as the bug showed: {leaning:?}");
    }

    #[test]
    fn a_camera_s_picture_put_in_a_slot_is_painted_in_its_own_matrix() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        let framing = painter.framing(&machine.device, &machine.queue);
        let mut shown = |colours| {
            // The camera's picture as the memory route makes it: NV12, a sample of each for every 2x2 block.
            let (luma, chroma) = (vec![RED_601.0; 16 * 16], [RED_601.1, RED_601.2].repeat(8 * 8));
            slot.put_nv12((16, 16), &luma, &chroma, colours).expect("a whole picture");
            current.follow(&slot);
            let frame = slot.take().expect("waits");
            assert_eq!(frame.colours(), Some(colours));
            current.show(&machine.device, &machine.queue, &painter, &slot, frame).expect("shown");
            render(&machine, (16, 16), |pass| current.draw(&painter, pass, &framing))[8][8]
        };
        near(shown(CAMERA), [255, 0, 0], "put in as a camera's BT.601");
        assert!(shown(Colours::HD)[1] > 12, "put in as BT.709, the same samples lean");
    }

    /// A software decoder's own planes, put in a slot as they decoded, are uploaded and painted the colours encoded.
    #[test]
    fn a_software_decoder_s_planes_are_painted_as_they_decoded() {
        // On the backend the window paints with: DX12 on Windows.
        let window_s = |info: &wgpu::AdapterInfo| cfg!(not(windows)) || info.backend == wgpu::Backend::Dx12;
        let Some((machine, info)) = machine_where(wgpu::Features::empty(), window_s) else {
            return skipped("see above");
        };
        let Some(x264) = media::survey().encoders.into_iter().find(|encoder| encoder.name == media::Name::X264) else {
            return skipped("x264 is not built here");
        };
        let side = 64;
        // NV12: the quadrants' luma, then their chroma interleaved, a sample for each 2x2 block.
        let (red, green, blue, white) = ((63, 102, 240), (173, 42, 26), (32, 240, 118), (235, 128, 128));
        let colour = |x: usize, y: usize| [[red, green], [blue, white]][y * 2 / side][x * 2 / side];
        let mut nv12: Vec<u8> = (0..side * side).map(|at| colour(at % side, at / side).0).collect();
        for y in 0..side / 2 {
            for x in 0..side / 2 {
                let (_, u, v) = colour(x * 2, y * 2);
                nv12.extend([u, v]);
            }
        }
        let settings = media::Settings {
            width: side,
            height: side,
            frames_per_second: 30,
            bits_per_second: 8_000_000,
            key_frame_every: None,
        };
        let mut session = media::Session::open(&x264, settings).expect("x264 opens");
        let picture = media::Nv12::new(side, side, nv12).expect("a picture");
        let mut units = Vec::new();
        for _ in 0..3 {
            units.extend(session.encode(&picture).expect("encoded"));
        }
        units.extend(session.finish().expect("finished"));
        let software = media::Sources { render_nodes: Vec::new(), ..media::Sources::default() };
        let mut decoder = media::Decoder::open(media::Codec::H264, &software).expect("a software decoder");
        let planar = units
            .iter()
            .flat_map(|unit| decoder.decode_shown(&unit.data, unit.pts).expect("decoded"))
            .find_map(|shown| match shown {
                media::Shown::InMemory(picture) => Some(picture),
                _ => None,
            })
            .expect("a picture as its decoder's planes");
        eprintln!("painted on {:?} ({})", info.backend, info.name);
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        slot.put_picture(planar);
        current.follow(&slot);
        current.show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("waits")).expect("shown");
        assert_eq!(current.size(), Some((64, 64)));
        let framing = painter.framing(&machine.device, &machine.queue);
        let pixels = render(&machine, (64, 64), |pass| current.draw(&painter, pass, &framing));
        near(pixels[16][16], [255, 0, 0], "top left is red");
        near(pixels[16][48], [0, 255, 0], "top right is green");
        near(pixels[48][16], [0, 0, 255], "bottom left is blue");
        near(pixels[48][48], [255, 255, 255], "bottom right is white");
    }

    #[test]
    fn the_picture_on_screen_is_gone_once_its_slot_is_emptied() {
        let Some(machine) = machine(wgpu::Features::empty()) else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let (slot, mut current) = (Slot::new(), Current::default());
        let show = |current: &mut Current| {
            let grey = flat((4, 2), GREY);
            slot.put((4, 2), (&grey.luma, &grey.blue, &grey.red), (4, 2, 2), grey.colours).expect("planes");
            current.follow(&slot);
            current.show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("waits")).expect("shown");
        };
        show(&mut current);
        assert_eq!(current.size(), Some((4, 2)));
        slot.empty();
        current.follow(&slot);
        assert_eq!(current.size(), None, "the watch ended: no picture on screen");
        show(&mut current);
        assert_eq!(current.size(), Some((4, 2)), "the next watch's picture shows");
    }
}
