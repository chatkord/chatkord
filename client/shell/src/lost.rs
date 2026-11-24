//! A lost GPU device: told once by the device's own callback, and mended at most twice a minute.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iced_wgpu::wgpu;

use crate::window::Wake;

/// Whether the device was lost since it was last asked. One value per device: a device ended on purpose, after its
/// window moved to a new one, tells the value it had, which nothing reads any more.
#[derive(Clone, Debug, Default)]
pub(crate) struct Losses(Arc<AtomicBool>);

impl Losses {
    /// Has `device` say when it is lost: remembered here, and `wake` asks the loop to mend it.
    pub(crate) fn watch(&self, device: &wgpu::Device, wake: Wake) {
        let lost = self.0.clone();
        device.set_device_lost_callback(move |reason, _| {
            // The reason's text is the driver's own, which can name an adapter: only whether we ended it is kept.
            report::note!(
                error,
                "the graphics device was lost",
                destroyed = reason == wgpu::DeviceLostReason::Destroyed
            );
            lost.store(true, Ordering::Release);
            wake();
        });
    }

    /// The device could not draw a frame: it is as good as lost, and mended as a lost one is.
    pub(crate) fn unusable(&self) {
        report::note!(error, "the graphics device could not draw a frame");
        self.0.store(true, Ordering::Release);
    }

    /// Whether the device was lost, left to be told.
    pub(crate) fn seen(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Whether the device was lost since the last ask; true once for one loss.
    pub(crate) fn tell(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// The most devices a run makes again within [`WITHIN`]: a device lost faster than that is not coming back.
const MOST: usize = 2;
/// The span [`MOST`] counts in.
const WITHIN: Duration = Duration::from_secs(60);

/// When the run made its device again lately.
#[derive(Debug, Default)]
pub(crate) struct Mends(VecDeque<Instant>);

impl Mends {
    /// Whether a device may be made again at `now`, counting it when it may.
    pub(crate) fn allow(&mut self, now: Instant) -> bool {
        while self.0.front().is_some_and(|at| now.saturating_duration_since(*at) >= WITHIN) {
            self.0.pop_front();
        }
        if self.0.len() >= MOST {
            return false;
        }
        self.0.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_third_mend_within_a_minute_is_refused_and_one_after_it_is_not() {
        let start = Instant::now();
        let mut mends = Mends::default();
        assert!(mends.allow(start));
        assert!(mends.allow(start + Duration::from_secs(10)));
        assert!(!mends.allow(start + Duration::from_secs(20)));
        assert!(!mends.allow(start + Duration::from_secs(59)));
        assert!(mends.allow(start + Duration::from_secs(61)));
        assert!(!mends.allow(start + Duration::from_secs(62)));
    }

    mod lab {
        use super::*;
        use crate::painter::Refusals;
        use crate::painter::tests::{Gpu, SIDE};
        use iced_wgpu::graphics::Viewport;
        use iced_winit::core::Size;
        use iced_winit::core::renderer::Scale;

        /// A device on whatever adapter is here, watched, then destroyed and polled, which tells its loss.
        fn lost(losses: &Losses, refusals: &Refusals) -> Gpu {
            let gpu = Gpu::new(refusals);
            losses.watch(gpu.device(), Arc::new(|| {}));
            gpu.device().destroy();
            let _ = gpu.device().poll(wgpu::PollType::Poll);
            gpu
        }

        #[test]
        fn a_destroyed_device_is_told_lost_once() {
            tier::lab();
            let (losses, refusals) = (Losses::default(), Refusals::default());
            let _gpu = lost(&losses, &refusals);
            assert!(losses.seen(), "the loss is seen until it is told");
            assert!(losses.tell(), "told once");
            assert!(!losses.tell(), "and not again");
        }

        #[test]
        fn a_renderer_on_a_new_device_draws_after_the_old_one_was_lost() {
            tier::lab();
            let (losses, refusals) = (Losses::default(), Refusals::default());
            let _old = lost(&losses, &refusals);
            assert!(losses.tell());
            let gpu = Gpu::new(&refusals);
            let mut renderer = gpu.renderer();
            let target = gpu.target(SIDE);
            let fits = Viewport::with_physical_size(Size::new(SIDE, SIDE), Scale::default());
            assert!(!gpu.draw(&mut renderer, &refusals, &losses, &target, &fits));
            assert_eq!(gpu.centre(&target), [0, 0, 255, 255], "the icon is drawn");
        }

        #[test]
        fn nothing_is_drawn_on_a_device_that_is_lost_and_not_yet_mended() {
            tier::lab();
            let (losses, refusals) = (Losses::default(), Refusals::default());
            let _old = lost(&losses, &refusals);
            let gpu = Gpu::new(&refusals);
            let mut renderer = gpu.renderer();
            let target = gpu.target(SIDE);
            let fits = Viewport::with_physical_size(Size::new(SIDE, SIDE), Scale::default());
            assert!(!gpu.draw(&mut renderer, &refusals, &losses, &target, &fits));
            assert_eq!(gpu.centre(&target), [0, 0, 0, 0], "the frame was left undrawn");
        }
    }
}
