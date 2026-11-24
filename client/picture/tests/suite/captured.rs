//! A captured BGRx DMA-BUF, a VA-API surface standing in, shows through the slot as painted, and is the texture's
//! memory: painting the buffer again changes what the same texture shows. No VA-API device or importing GPU skips.

use crate::common;

use crate::dmabuf;

use capture::Pixels;
use common::{FORMAT, Machine, machine_where, render, skipped};
use picture::{Current, Painter, Slot, render_node};

const WIDTH: usize = 640;
const HEIGHT: usize = 360;

/// Four colour blocks, with a ramp across the top-left one, so a wrong stride or channel order moves something checked.
fn painted(colours: [[u8; 3]; 4]) -> Vec<[u8; 3]> {
    let mut rgb = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let block = usize::from(x >= WIDTH / 2) + 2 * usize::from(y >= HEIGHT / 2);
            rgb.push(if block == 0 { [(x * 255 / (WIDTH / 2)) as u8, 0, 0] } else { colours[block] });
        }
    }
    rgb
}

const FIRST: [[u8; 3]; 4] = [[0; 3], [0, 200, 0], [0, 0, 200], [250, 250, 250]];
const SECOND: [[u8; 3]; 4] = [[0; 3], [200, 0, 200], [200, 200, 0], [10, 10, 10]];

/// A painted surface and a device on the same GPU that can import it.
fn same_gpu() -> Option<(dmabuf::Surface, Machine)> {
    let node = std::cell::RefCell::new(None);
    let (machine, _) = machine_where(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF, |info| {
        let found = (info.backend == wgpu::Backend::Vulkan)
            .then(|| render_node(info))
            .flatten()
            .and_then(|path| dmabuf::Surface::new(&path, WIDTH, HEIGHT));
        let suits = found.is_some();
        *node.borrow_mut() = found;
        suits
    })?;
    if !machine.device.features().contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF) {
        skipped("this GPU cannot import a DMA-BUF");
        return None;
    }
    node.into_inner().map(|surface| (surface, machine))
}

fn shown(machine: &Machine, painter: &Painter, current: &Current) -> Vec<Vec<[u8; 4]>> {
    render(machine, (WIDTH as u32, HEIGHT as u32), |pass| {
        current.draw(painter, pass, &painter.framing(&machine.device, &machine.queue))
    })
}

/// The worst difference between what was drawn and `rgb`, over every pixel.
fn worst(drawn: &[Vec<[u8; 4]>], rgb: &[[u8; 3]]) -> i32 {
    let mut worst = 0;
    for (y, row) in drawn.iter().enumerate() {
        for (x, pixel) in row.iter().enumerate() {
            let want = rgb[y * WIDTH + x];
            for channel in 0..3 {
                worst = worst.max((i32::from(pixel[channel]) - i32::from(want[channel])).abs());
            }
            assert_eq!(pixel[3], 255, "the compositor's alpha is not shown");
        }
    }
    worst
}

mod lab {
    use super::*;

    #[test]
    fn a_captured_screen_is_painted_from_its_own_memory() {
        let Some((mut surface, machine)) = same_gpu() else { return skipped("see above") };
        let first = painted(FIRST);
        surface.paint_rgb(&first);
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        let captured = surface.frame(Pixels::Bgrx);
        assert!(slot.lend(&captured), "an empty slot takes the picture");
        drop(captured);
        let mut current = Current::default();
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the picture waits"))
            .expect("a picture this device holds");
        assert!(slot.takes_imports(), "a capture the device can import is not refused");
        assert_eq!(current.size(), Some((WIDTH as u32, HEIGHT as u32)));
        assert!(worst(&shown(&machine, &painter, &current), &first) <= 1, "the painted pixels");

        // The same texture, nothing imported again: what it shows follows the buffer, so it is the buffer.
        let second = painted(SECOND);
        surface.paint_rgb(&second);
        assert!(worst(&shown(&machine, &painter, &current), &second) <= 1, "the buffer painted again");
    }

    /// Tells when the compositor's buffer it rides with goes home.
    struct Home(std::sync::mpsc::Sender<()>);

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[test]
    fn one_s_own_screen_is_drawn_at_its_card_s_size_and_its_buffer_goes_home_at_once() {
        let Some((mut surface, machine)) = same_gpu() else { return skipped("see above") };
        let first = painted(FIRST);
        surface.paint_rgb(&first);
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        slot.want((WIDTH as u32 / 2, HEIGHT as u32 / 2));
        let (home, went_home) = std::sync::mpsc::channel();
        let captured = surface.frame_lent(Pixels::Bgrx, Home(home));
        assert!(slot.lend(&captured));
        drop(captured);
        let mut current = Current::default();
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the picture waits"))
            .expect("a picture this device holds");
        assert_eq!(current.size(), Some((WIDTH as u32 / 2, HEIGHT as u32 / 2)), "drawn at the card's size");
        machine.device.poll(wgpu::PollType::wait_indefinitely()).expect("the draw finishes");
        // The buffer is let go of on the release thread once the draw is done, so it is waited for, within a bound.
        let gone = went_home.recv_timeout(std::time::Duration::from_secs(1));
        assert!(gone.is_ok(), "the compositor's buffer is held past its draw");
        // Each block's middle shows its colour, the picture whole in the smaller texture.
        let drawn = shown(&machine, &painter, &current);
        for (block, (x, y)) in
            [(1, (WIDTH * 3 / 4, HEIGHT / 4)), (2, (WIDTH / 4, HEIGHT * 3 / 4)), (3, (WIDTH * 3 / 4, HEIGHT * 3 / 4))]
        {
            let pixel = drawn[y][x];
            let want = FIRST[block];
            assert!((0..3).all(|c| (i32::from(pixel[c]) - i32::from(want[c])).abs() <= 2), "block {block}: {pixel:?}");
        }
    }

    /// DRM's own "no modifier": a layout no device reads.
    const NO_LAYOUT: u64 = 0x00ff_ffff_ffff_ffff;

    #[test]
    fn a_screen_laid_out_as_the_device_cannot_read_is_refused_before_an_image_is_made() {
        if !tier::validated() {
            return;
        }
        let Some((surface, machine)) = same_gpu() else { return skipped("see above") };
        let painter = Painter::new(&machine.device, FORMAT);
        let slot = Slot::new();
        assert!(slot.lend(&surface.frame_claiming(Pixels::Bgrx, NO_LAYOUT)), "an empty slot takes the picture");
        let mut current = Current::default();
        // An image made with a modifier the device does not list is invalid, which the validation layer ends.
        current
            .show(&machine.device, &machine.queue, &painter, &slot, slot.take().expect("the picture waits"))
            .expect("refused, not too large");
        assert!(!slot.takes_imports(), "the capture is told the window cannot take its buffers");
        assert_eq!(current.size(), None, "nothing is shown");
    }
}
