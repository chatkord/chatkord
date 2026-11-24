//! A run whose GPU device was lost: its windows drawn again on a new device, and what the scene paints with told.

use std::time::Instant;

use super::Scene;
use super::gpu::Made;
use super::ready::Ready;
use super::unhosted::Unhosted;

impl<S: Scene> Ready<S> {
    /// Whether the device the windows paint with was lost since this was last asked.
    pub(super) fn lost(&self) -> bool {
        self.gpu.losses.tell()
    }

    /// Makes the device again and draws every window on it: a new surface and renderer each, the old renderers ending
    /// before the old device. A third loss within a minute is [`Unhosted::DeviceLostAgain`].
    pub(super) fn mend(&mut self, now: Instant) -> Result<(), Unhosted> {
        if !self.mends.allow(now) {
            return Err(Unhosted::DeviceLostAgain);
        }
        let Made { gpu, node, .. } = self.gpu.again(&self.main.surface)?;
        let remade = self.main.remake(&gpu).and_then(|()| self.out.as_mut().map_or(Ok(()), |out| out.remake(&gpu)));
        let painting = picture::Painting::of(&gpu.device, node);
        // The old device ends here, after every renderer that held it.
        self.gpu = gpu;
        remade?;
        self.scene.repainted(painting);
        self.redraw_all();
        Ok(())
    }
}
