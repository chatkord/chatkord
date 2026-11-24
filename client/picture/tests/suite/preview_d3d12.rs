//! One's own share: a BGRA picture the encoder's D3D11 device draws into the preview ring paints in place on the
//! window's D3D12 device, is never rewritten while held, and the ring comes round. No D3D12 adapter or shared GPU skips.

use crate::common;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use common::{Checked, FORMAT, Machine, machine_where, render, skipped};
use media::{D3d11, Frame, Gpu, Layout, Native, Pixels, Preview, PreviewPicture};
use picture::{Current, IMPORT_FEATURES, OnWindow, Painter, Painting, Slot, import};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::Interface;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
/// Red, green, blue and white quadrants, as BGRA.
const QUADRANTS: [[u8; 4]; 4] = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255], [255, 255, 255, 255]];
const BLACK: [[u8; 4]; 4] = [[0, 0, 0, 255]; 4];

/// The window's device and the encoder's, on one GPU, and a preview between them.
fn window_and_preview() -> Option<(Machine, Checked, Preview)> {
    window_and_preview_where(|info| info.backend == wgpu::Backend::Dx12)
}

/// The same with the window's device on the first adapter that `suits`, the encoder's on the window's adapter.
fn window_and_preview_where(suits: impl Fn(&wgpu::AdapterInfo) -> bool) -> Option<(Machine, Checked, Preview)> {
    let (machine, _) = machine_where(IMPORT_FEATURES, suits)?;
    let painting = Painting::of(&machine.device, None);
    let window = painting.device().expect("a D3D12 window").clone();
    let Some(gpu) = Gpu::open_on(Some(window.adapter()), media::Validation::of_this_run()) else {
        skipped("no D3D11 device for the encoder");
        return None;
    };
    let Some(preview) = Preview::new(&gpu, &window.0) else {
        skipped("the encoder's device cannot share with the window's");
        return None;
    };
    Some((machine, Checked { gpu: Some(gpu), window }, preview))
}

/// A captured picture of four quadrants of `colours`, drawn on the encoder's device.
fn captured(gpu: &Gpu, colours: [[u8; 4]; 4]) -> Frame {
    captured_at(gpu, colours, (WIDTH, HEIGHT))
}

/// The same at `(width, height)`, as a shared window of that size is captured.
fn captured_at(gpu: &Gpu, colours: [[u8; 4]; 4], (width, height): (u32, u32)) -> Frame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend(colours[usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2)]);
        }
    }
    let described = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA { pSysMem: pixels.as_ptr().cast(), SysMemPitch: width * 4, SysMemSlicePitch: 0 };
    let mut texture = None;
    // SAFETY: the description and the data outlive the call; the data is a whole picture of that size.
    unsafe { gpu.device().CreateTexture2D(&raw const described, Some(&raw const data), Some(&raw mut texture)) }
        .expect("a texture");
    let layout = Layout { pixels: Pixels::Bgra, width, height };
    Frame::new(layout, Native::D3d11(D3d11::new(texture.expect("made"))), Instant::now())
}

/// The next picture of the ring, waiting a moment for the window's GPU to give one back, as a share's next frame would.
fn lend(preview: &mut Preview, frame: &Frame, within: (u32, u32)) -> Option<PreviewPicture> {
    let until = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(picture) = preview.lend(frame, within) {
            return Some(picture);
        }
        if Instant::now() > until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The colour at the middle of each quadrant of a render, as RGBA.
fn quadrants(pixels: &[Vec<[u8; 4]>]) -> [[u8; 4]; 4] {
    let (height, width) = (pixels.len(), pixels[0].len());
    let at = |x: usize, y: usize| pixels[y][x];
    [
        at(width / 4, height / 4),
        at(3 * width / 4, height / 4),
        at(width / 4, 3 * height / 4),
        at(3 * width / 4, 3 * height / 4),
    ]
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 3)
}

/// What a BGRA quadrant looks like painted into the RGBA target.
fn painted([b, g, r, a]: [u8; 4]) -> [u8; 4] {
    [r, g, b, a]
}

mod lab {
    use super::*;

    #[test]
    fn a_hidden_window_is_lent_nothing_and_gives_back_what_waited() {
        let Some((_machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let frame = captured(&gpu, QUADRANTS);
        let slot = Slot::new();
        slot.want((640, 360));
        assert!(slot.lend_preview(|within| lend(&mut preview, &frame, within)), "lent");
        slot.show(false);
        assert!(slot.take().is_none(), "the picture waiting went back to its ring when the window hid");
        let mut asked = false;
        assert!(
            !slot.lend_preview(|within| {
                asked = true;
                lend(&mut preview, &frame, within)
            }),
            "a hidden window is lent nothing"
        );
        assert!(!asked, "nor is the ring asked to draw one");
        slot.show(true);
        assert!(slot.lend_preview(|within| lend(&mut preview, &frame, within)), "shown again, it is lent the next");
    }

    #[test]
    fn ones_own_picture_is_painted_at_the_cards_size_with_its_colours_where_they_were() {
        let Some((machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let frame = captured(&gpu, QUADRANTS);
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        slot.want((640, 360));
        assert!(slot.lend_preview(|within| lend(&mut preview, &frame, within)), "lent");
        let mut current = Current::default();
        current.show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("waits")).expect("shown");
        assert!(slot.takes_imports(), "a preview on the window's own GPU is not refused");
        assert_eq!(current.size(), Some((640, 360)), "drawn at the card's size");
        let pixels = render(&machine, (640, 360), |pass| {
            current.draw(&painter, pass, &painter.framing(&machine.device, &machine.queue))
        });
        for (got, want) in quadrants(&pixels).into_iter().zip(QUADRANTS) {
            assert!(near(got, painted(want)), "a quadrant painted {got:?}, captured {want:?}");
        }

        // A card larger than the picture gets the picture's own size, never more.
        slot.want((4000, 4000));
        let mut current = Current::default();
        assert!(slot.lend_preview(|within| lend(&mut preview, &frame, within)), "lent again");
        current.show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("waits")).expect("shown");
        assert_eq!(current.size(), Some((WIDTH, HEIGHT)));
    }

    #[test]
    fn a_picture_lent_to_a_window_on_each_adapter_dxgi_lists_is_painted_there() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        if adapters.is_empty() {
            return skipped("no D3D12 adapter");
        }
        for at in 0..adapters.len() {
            let seen = std::cell::Cell::new(0);
            let nth = |info: &wgpu::AdapterInfo| {
                if info.backend != wgpu::Backend::Dx12 {
                    return false;
                }
                let this = seen.get();
                seen.set(this + 1);
                this == at
            };
            let Some((machine, gpu, mut preview)) = window_and_preview_where(nth) else {
                return skipped("see above");
            };
            assert_eq!(gpu.adapter(), Some(gpu.window.adapter()), "adapter {at}: the encoder's device is the window's");
            let frame = captured(&gpu, QUADRANTS);
            let painter = Painter::new(&machine.device, FORMAT);
            let picture = lend(&mut preview, &frame, (640, 360)).unwrap_or_else(|| panic!("adapter {at}: lent"));
            let shown = (picture.width(), picture.height());
            let (texture, _held) = import(&machine.device, OnWindow::Preview(picture)).expect("adopted");
            let drawn = painter.bind_rgb(&machine.device, texture, shown).expect("a live device binds");
            let pixels = render(&machine, (640, 360), |pass| {
                painter.draw_imported(pass, &drawn, &painter.framing(&machine.device, &machine.queue))
            });
            for (got, want) in quadrants(&pixels).into_iter().zip(QUADRANTS) {
                assert!(near(got, painted(want)), "adapter {at}: a quadrant painted {got:?}, captured {want:?}");
            }
        }
    }

    #[test]
    fn a_picture_the_window_holds_is_never_written_again() {
        let Some((machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let (coloured, black) = (captured(&gpu, QUADRANTS), captured(&gpu, BLACK));
        let painter = Painter::new(&machine.device, FORMAT);
        let first = lend(&mut preview, &coloured, (640, 360)).expect("a free texture");
        let kept = first.resource().as_raw();
        let shown = (first.width(), first.height());
        let (texture, _held) = import(&machine.device, OnWindow::Preview(first)).expect("adopted");
        let held = painter.bind_rgb(&machine.device, texture, shown).expect("a live device binds");
        for _ in 0..5 {
            let next = lend(&mut preview, &black, (640, 360)).expect("a free texture besides the held one");
            assert_ne!(next.resource().as_raw(), kept, "the held texture was lent again");
            let shown = (next.width(), next.height());
            let (texture, again) = import(&machine.device, OnWindow::Preview(next)).expect("adopted");
            let drawn = painter.bind_rgb(&machine.device, texture, shown).expect("a live device binds");
            render(&machine, (16, 16), |pass| {
                painter.draw_imported(pass, &drawn, &painter.framing(&machine.device, &machine.queue))
            });
            drop(again);
        }
        let pixels = render(&machine, (640, 360), |pass| {
            painter.draw_imported(pass, &held, &painter.framing(&machine.device, &machine.queue))
        });
        for (got, want) in quadrants(&pixels).into_iter().zip(QUADRANTS) {
            assert!(near(got, painted(want)), "the held picture now shows {got:?}, not {want:?}");
        }
    }

    #[test]
    fn the_ring_comes_round_as_the_window_gives_pictures_back() {
        let Some((machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let frame = captured(&gpu, QUADRANTS);
        let painter = Painter::new(&machine.device, FORMAT);
        let mut resources = HashSet::new();
        for lent in 0..30 {
            let picture =
                lend(&mut preview, &frame, (640, 360)).unwrap_or_else(|| panic!("lend {lent}: no texture came back"));
            resources.insert(picture.resource().as_raw() as usize);
            let shown = (picture.width(), picture.height());
            let (texture, held) = import(&machine.device, OnWindow::Preview(picture)).expect("adopted");
            let drawn = painter.bind_rgb(&machine.device, texture, shown).expect("a live device binds");
            render(&machine, (16, 16), |pass| {
                painter.draw_imported(pass, &drawn, &painter.framing(&machine.device, &machine.queue))
            });
            drop(held);
        }
        assert!(resources.len() <= 3, "{} textures for 30 pictures: the ring did not come round", resources.len());
    }

    #[test]
    fn a_picture_lent_with_no_encode_after_it_is_painted_at_once() {
        let Some((machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let frame = captured(&gpu, QUADRANTS);
        let painter = Painter::new(&machine.device, FORMAT);
        let picture = lend(&mut preview, &frame, (640, 360)).expect("a free texture");
        // Nothing more is asked of the encoder's device, as when a share nobody watches lends its card a picture.
        let shown = (picture.width(), picture.height());
        let began = Instant::now();
        let (texture, _held) = import(&machine.device, OnWindow::Preview(picture)).expect("adopted");
        let drawn = painter.bind_rgb(&machine.device, texture, shown).expect("a live device binds");
        render(&machine, (16, 16), |pass| {
            painter.draw_imported(pass, &drawn, &painter.framing(&machine.device, &machine.queue))
        });
        let took = began.elapsed();
        assert!(took < Duration::from_millis(500), "the window's queue waited {took:?} for the picture's signal");
    }

    #[test]
    fn a_picture_dropped_unshown_gives_its_texture_back() {
        let Some((_machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let frame = captured(&gpu, QUADRANTS);
        for lent in 0..10 {
            // An emptied slot or a refused import drops a picture the window never drew.
            let picture = lend(&mut preview, &frame, (640, 360));
            assert!(picture.is_some(), "lend {lent}: the ring lost its textures to pictures dropped unshown");
        }
    }

    #[test]
    fn a_shared_window_that_changes_size_keeps_the_ring_and_is_painted_whole() {
        let Some((machine, gpu, mut preview)) = window_and_preview() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let mut resources = HashSet::new();
        // A window dragged a few pixels at a time, its card the same size.
        for step in 0..12 {
            let size = (WIDTH - 8 * step, HEIGHT - 6 * step);
            let frame = captured_at(&gpu, QUADRANTS, size);
            let slot = Slot::new();
            slot.want((640, 360));
            assert!(slot.lend_preview(|within| lend(&mut preview, &frame, within)), "step {step}: lent");
            let picture = slot.take().expect("waits");
            let picture_size = match &picture {
                picture::Frame::OnWindow(OnWindow::Preview(lent)) => {
                    resources.insert(lent.resource().as_raw() as usize);
                    (lent.width(), lent.height())
                }
                _ => panic!("a preview"),
            };
            let mut current = Current::default();
            current.show(&machine.device, &machine.queue, &painter, &slot, picture).expect("shown");
            let pixels = render(&machine, picture_size, |pass| {
                current.draw(&painter, pass, &painter.framing(&machine.device, &machine.queue))
            });
            for (got, want) in quadrants(&pixels).into_iter().zip(QUADRANTS) {
                assert!(near(got, painted(want)), "step {step}: a quadrant painted {got:?}, captured {want:?}");
            }
            let edge = pixels[pixels.len() - 1][pixels[0].len() - 1];
            assert!(near(edge, painted(QUADRANTS[3])), "step {step}: the corner painted {edge:?}");
        }
        assert!(resources.len() <= 3, "{} textures for 12 sizes: the ring was made again", resources.len());
    }
}
