//! Our area pass for a VA-API encoder, as VA-API's own scaler drops one-pixel lines: the compositor's DMA-BUF read on
//! a Vulkan device of the encoder's own GPU (`node`) and written as NV12 straight into the encoder's surfaces
//! (`exported`). A run returns once the GPU has written, before the encoder is given the picture, and the
//! compositor's buffer goes back then too.

use std::mem;

use super::On;
use super::exported::Exported;
use super::imported::Screens;
use super::node;
use super::pass::{Loaded, Pass, Target};
use crate::av::{ColourRange, ColourSpace, Frame, HwDevice, HwFrames};
use crate::encode::{EncodeError, Settings, pool_for};
use crate::probe::Backend;
use capture::{Frame as Captured, Layout};

/// The screen path into a VA-API encoder's own frames through our pass.
pub(crate) struct Path {
    // Declared in the order they go: the pass's objects, what it imported, the encoder's frames, the pass's device.
    pass: Box<Pass>,
    screens: Box<Screens>,
    frames: Box<Exported>,
    vulkan: HwDevice,
}

// SAFETY: a path owns the pass and everything it holds and shares none; FFmpeg's objects, the VA display and Vulkan's
// handles may move between threads while one thread uses them at a time, which `&mut self` ensures.
unsafe impl Send for Path {}

impl Path {
    /// The path that reads the top-left `read` of DMA-BUFs laid out as `source` and leaves NV12 of the settings' size
    /// in frames of `on`'s VA-API device, through a Vulkan device on the same GPU: the one `on` kept, or one opened on
    /// its node. Refused (`Screen`) where any of what that needs is missing here.
    pub(super) fn open(on: &On<'_>, settings: Settings, source: Layout, read: (u32, u32)) -> Result<Self, EncodeError> {
        let vulkan = match on.pass {
            Some(kept) => kept.again().ok_or(EncodeError::Device)?,
            None => {
                let node = on.named.node.as_deref().ok_or(EncodeError::Screen)?;
                node::vulkan_on(node, on.named.validation)?
            }
        };
        let loaded = Loaded::of(&vulkan)?;
        let frames = Box::new(Exported::new(pool_for(on.device, Backend::VaApi, settings)?, &loaded)?);
        let screens = Box::new(Screens::new(loaded.instance.clone(), loaded.physical, loaded.device.clone(), source));
        let pass = Box::new(Pass::new(loaded, settings, read)?);
        let mut path = Self { pass, screens, frames, vulkan };
        // One surface exported and imported now, so a driver or device that cannot is refused before the encoder opens.
        path.frames.next().map_err(|_| EncodeError::Screen)?;
        Ok(path)
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.frames.pool().again().ok_or(EncodeError::Frames)
    }

    /// The Vulkan device the pass runs on, which a reopen on the same encoder device keeps.
    pub(super) const fn vulkan(&self) -> &HwDevice {
        &self.vulkan
    }

    /// Reads `captured` on the GPU and returns the NV12 picture, `pts` stamped, its surface written. The compositor's
    /// buffer goes back as soon as the GPU has read it, before this returns.
    pub(super) fn convert(&mut self, captured: Captured, pts: i64) -> Result<Frame, EncodeError> {
        let Self { pass, screens, frames, .. } = self;
        // An image is freed only once no run reads it; none is, where the pass cannot settle.
        let screen = screens.of(&captured, || pass.settle().is_ok())?;
        let (image, view) = (screen.image, screen.view);
        // Past the open, a surface that will not export is a picture that cannot be converted.
        let (mut target, [luma, chroma]) = frames.next().map_err(|_| EncodeError::Convert)?;
        let size = frames.size();
        let written = |captured: &Captured, into| screens.written(captured, into);
        // SAFETY: `image` and its view are the lent buffer's, imported on the pass's device and freed only once the
        // pass settled; the planes are the surface's, imported there too, and the pool handed the surface out with no
        // picture of it pending.
        let run = unsafe { pass.run(image, view, Target::Planes { luma, chroma, size }, captured, written) };
        if let Err(failed) = run {
            if pass.lost {
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

    /// Waits for every picture still converting: none is, as each run is waited before its convert returns.
    pub(super) fn settle(&mut self) -> Result<(), EncodeError> {
        self.pass.settle()
    }
}

impl Drop for Path {
    fn drop(&mut self) {
        if self.pass.settle().is_ok() {
            // SAFETY: every submission that read or wrote the images is done: the pass settled.
            unsafe { self.screens.clear() };
            // SAFETY: as above.
            unsafe { self.frames.clear() };
        }
    }
}

#[cfg(test)]
mod tests {
    mod lab {
        use super::super::{On, Path};
        use crate::av::{Frame, HwDevice};
        use crate::encode::Settings;
        use crate::encode::screen::painted;
        use crate::probe::{Backend, survey};
        use capture::{Layout, Pixels};

        /// Every surface of the pool held at once, three times over, is exported the first time only.
        #[test]
        fn each_encoder_surface_is_exported_once() {
            tier::lab();
            let report = survey();
            let named = report.encoders.iter().filter_map(|e| e.device()).find(|d| d.backend == Backend::VaApi);
            let Some(named) = named.cloned() else { return tier::skipped("no VA-API encoder here") };
            let Some(node) = named.node.clone() else { return tier::skipped("the VA-API encoder names no node") };
            let Some(device) = HwDevice::open(Backend::VaApi, Some(&node), named.validation) else {
                return tier::skipped("the VA-API device does not open");
            };
            let Some(surface) = painted::Surface::new(&node, 1280, 720) else {
                return tier::skipped("the VA-API device makes no BGRx surface");
            };
            let layout = Layout { pixels: Pixels::Bgrx, width: 1280, height: 720 };
            let settings = Settings {
                width: 640,
                height: 360,
                frames_per_second: 30,
                bits_per_second: 1_000_000,
                key_frame_every: None,
            };
            let on = On { device: &device, named: &named, pass: None };
            let mut path = match Path::open(&on, settings, layout, (1280, 720)) {
                Ok(path) => path,
                Err(why) => {
                    return tier::skipped(format!("our pass does not write this VA-API device's surfaces: {why}"));
                }
            };
            let size = usize::try_from(path.frames.pool().pool_size()).unwrap();
            for _ in 0..3 {
                let held: Vec<Frame> = (0..size)
                    .map(|_| path.convert(surface.frame(Pixels::Bgrx), 0).expect("a converted picture"))
                    .collect();
                let ids: std::collections::HashSet<Option<u32>> = held.iter().map(Frame::vaapi_surface).collect();
                assert_eq!(ids.len(), size, "every surface of the pool is in use");
            }
            assert_eq!(path.frames.exported(), size, "a surface is exported once, not once a picture");
        }
    }
}
