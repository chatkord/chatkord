//! Vulkan's screen path: the compositor's DMA-BUF imported onto the encoder's device and written as NV12 into the
//! encoder's own frames by our pass (`pass`), as FFmpeg 9.0's `scale_vulkan` cannot convert at the picture's own size.

use std::mem;

use super::imported::Screens;
use super::pass::{Loaded, Pass, Target};
use crate::av::{ColourRange, ColourSpace, Frame, HwDevice, HwFrames};
use crate::encode::{EncodeError, Settings, pool_for};
use capture::{Frame as Captured, Layout};

/// The screen path into a Vulkan encoder's own frames.
pub(crate) struct Path {
    // Declared first: the pass's objects go before the frames and device they were made for.
    pass: Box<Pass>,
    screens: Box<Screens>,
    pool: HwFrames,
    _device: HwDevice,
}

// SAFETY: a path owns the pass and everything it holds and shares none; FFmpeg's objects and Vulkan's handles may
// move between threads while one thread uses them at a time, which `&mut self` ensures.
unsafe impl Send for Path {}

impl Path {
    /// The path that reads the top-left `read` of DMA-BUFs laid out as `source` and leaves NV12 of the settings' size
    /// in frames of `device`, a Vulkan device FFmpeg made for the encoder.
    pub(super) fn open(
        device: &HwDevice,
        settings: Settings,
        source: Layout,
        read: (u32, u32),
    ) -> Result<Self, EncodeError> {
        let pool = pool_for(device, crate::probe::Backend::Vulkan, settings)?;
        let loaded = Loaded::of(device)?;
        let screens = Box::new(Screens::new(loaded.instance.clone(), loaded.physical, loaded.device.clone(), source));
        let pass = Box::new(Pass::new(loaded, settings, read)?);
        let device = device.again().ok_or(EncodeError::Device)?;
        Ok(Self { pass, screens, pool, _device: device })
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.pool.again().ok_or(EncodeError::Frames)
    }

    /// Submits the conversion of `captured` on the GPU and returns the NV12 picture, `pts` stamped, ordered by its
    /// frame's own semaphore, which the encoder waits on. The compositor's buffer goes back once the GPU has read it,
    /// which a later convert or [`Path::settle`] waits for.
    pub(super) fn convert(&mut self, captured: Captured, pts: i64) -> Result<Frame, EncodeError> {
        let Self { pass, screens, pool, .. } = self;
        // An image is freed only once no run reads it; none is, where the pass cannot settle.
        let screen = screens.of(&captured, || pass.settle().is_ok())?;
        let (image, view) = (screen.image, screen.view);
        let mut target = pool.take().map_err(|_| EncodeError::Convert)?;
        let written = |captured: &Captured, into| screens.written(captured, into);
        // SAFETY: `image` and its view are the lent buffer's, imported on the pass's device and freed only once the
        // pass settled; `target` is a live Vulkan frame of the pool the pass was opened for.
        if let Err(failed) = unsafe { pass.run(image, view, Target::Frame(&target), captured, written) } {
            if pass.lost {
                // The target holds a reference to its frames context, so the image behind it stays alive too.
                #[expect(
                    clippy::mem_forget,
                    reason = "the GPU may still be reading or writing it: leaked, never freed under it"
                )]
                mem::forget(target);
            }
            return Err(failed);
        }
        target.set_pts(pts);
        target.set_colour_space(ColourSpace::Bt709);
        target.set_colour_range(ColourRange::Limited);
        Ok(target)
    }

    /// Waits for every picture still converting, so each compositor's buffer is the compositor's again.
    pub(super) fn settle(&mut self) -> Result<(), EncodeError> {
        self.pass.settle()
    }
}

impl Drop for Path {
    fn drop(&mut self) {
        if self.pass.settle().is_ok() {
            // SAFETY: every submission that read the images is done: the pass settled.
            unsafe { self.screens.clear() };
        }
    }
}

#[cfg(test)]
mod tests {
    mod lab {
        use super::super::Path;
        use crate::av::HwDevice;
        use crate::encode::Settings;
        use crate::encode::screen::painted;
        use crate::probe::{Backend, survey};
        use capture::{Layout, Pixels};

        #[test]
        fn a_path_leaves_its_device_to_its_owner_once_it_has_converted() {
            tier::lab();
            let report = survey();
            let node = report.encoders.iter().filter_map(|e| e.device()).find(|d| d.backend == Backend::VaApi);
            let Some(node) = node.and_then(|d| d.node.clone()) else {
                return tier::skipped("no VA-API device to make a DMA-BUF on");
            };
            let Some(device) = HwDevice::open(Backend::Vulkan, None, crate::Validation::Off) else {
                return tier::skipped("no Vulkan device opens here");
            };
            let Some(surface) = painted::Surface::new(&node, 640, 360) else {
                return tier::skipped("the VA-API device makes no BGRx surface");
            };
            let layout = Layout { pixels: Pixels::Bgrx, width: 640, height: 360 };
            let settings = Settings {
                width: 640,
                height: 360,
                frames_per_second: 30,
                bits_per_second: 1_000_000,
                key_frame_every: None,
            };
            let Ok(mut path) = Path::open(&device, settings, layout, (640, 360)) else {
                return tier::skipped("this Vulkan device takes no screen here");
            };
            for _ in 0..3 {
                path.convert(surface.frame(Pixels::Bgrx), 0).expect("a converted picture");
            }
            drop(path);
            let held = device.references();
            assert_eq!(held, 1, "the path left references to the encoder's device behind");
        }
    }
}
