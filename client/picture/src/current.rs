//! The picture on screen now, as the textures that hold it: planes copied up from memory, or a picture imported where
//! it is. Kept between frames, and replaced by the next picture the slot hands over.

#[cfg(windows)]
use crate::d3d12::Imports;
use crate::frame::{Frame, Slot};
use crate::yuv::{Framing, Gpu, Imported, Painter, TooLarge};

/// The picture on screen, or none, and on Windows the imports kept while the decoder lends from their pool.
#[derive(Default)]
pub struct Current {
    on_screen: OnScreen,
    #[cfg(windows)]
    imports: Imports,
}

/// What the picture on screen is made of.
#[derive(Default)]
enum OnScreen {
    /// No picture: nothing is drawn.
    #[default]
    Nothing,
    /// Planes copied up from memory, three or NV12's two.
    Planes(Gpu),
    /// A picture imported where it is: a decoder's surface, a captured screen, or one on the window's D3D12 device.
    Imported {
        /// The texture and what binds it for drawing.
        picture: Imported,
        /// Its size in pixels.
        size: (u32, u32),
        /// The picture under it on the window's D3D12 device, given back to its writer after the window's reads when
        /// this is replaced.
        #[cfg(windows)]
        _held: Option<crate::d3d12::Held>,
    },
    /// A decoder's picture on the window's D3D12 device, drawn from the import kept under `key`.
    #[cfg(windows)]
    Lent {
        /// What finds the import in [`Imports`].
        key: usize,
        /// Its size in pixels.
        size: (u32, u32),
        /// The picture under it, given back to the decoder after the window's reads when this is replaced.
        _held: crate::d3d12::Held,
    },
    /// One's own screen drawn again at its card's size into a texture of the window's, so the compositor's buffer goes
    /// home as soon as it is read; the texture is drawn into again while the card keeps its size.
    #[cfg(target_os = "linux")]
    Card {
        /// The window's texture, which `picture` binds.
        texture: wgpu::Texture,
        /// What binds the texture for drawing.
        picture: Imported,
        /// The whole picture, as it is drawn into the texture.
        framing: Framing,
        /// The picture's size inside the texture.
        size: (u32, u32),
    },
}

impl Current {
    /// Make `frame` the picture on screen. One the device cannot import tells the slot, so a decoder sends planes and
    /// a capture nothing until the slot offers imports again; one too large is dropped. Either way the picture on
    /// screen stays as it was.
    pub fn show(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        painter: &Painter,
        slot: &Slot,
        frame: Frame,
    ) -> Result<(), TooLarge> {
        let shown = self.show_on_screen(device, queue, painter, slot, frame);
        self.trim();
        shown
    }

    fn show_on_screen(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        painter: &Painter,
        slot: &Slot,
        frame: Frame,
    ) -> Result<(), TooLarge> {
        match frame {
            Frame::Planes(planes) => {
                let size = (planes.width, planes.height);
                if !self.holds(size, media::PictureLayout::I420) {
                    match Gpu::new(device, painter, size) {
                        Ok(gpu) => self.on_screen = OnScreen::Planes(gpu),
                        Err(refused) => {
                            slot.give_back(planes);
                            return Err(refused);
                        }
                    }
                }
                if let OnScreen::Planes(gpu) = &mut self.on_screen {
                    gpu.upload(queue, planes.planes(), planes.colours);
                }
                slot.give_back(planes);
            }
            Frame::InMemory(picture) => {
                let (size, layout) = (picture.size(), picture.layout());
                if !self.holds(size, layout) {
                    self.on_screen = OnScreen::Planes(Gpu::of_layout(device, painter, size, layout)?);
                }
                if let OnScreen::Planes(gpu) = &mut self.on_screen {
                    gpu.upload_samples(queue, picture.samples(), picture.colours());
                }
            }
            #[cfg(target_os = "linux")]
            Frame::Surface(surface) => {
                let size = (surface.width(), surface.height());
                match painter.import(device, queue, surface) {
                    Ok(picture) => self.on_screen = OnScreen::Imported { picture, size },
                    Err(_) => slot.refuse_imports(),
                }
            }
            #[cfg(windows)]
            Frame::OnWindow(picture) => {
                let size = picture.size();
                match picture {
                    crate::d3d12::OnWindow::Decoded(surface) => match self.imports.lend(device, painter, surface) {
                        Ok((key, held)) => self.on_screen = OnScreen::Lent { key, size, _held: held },
                        Err(_) => slot.refuse_imports(),
                    },
                    preview @ crate::d3d12::OnWindow::Preview(_) => {
                        let bound = crate::d3d12::import(device, preview)
                            .ok()
                            .and_then(|(texture, held)| Some((painter.bind_rgb(device, texture, size)?, held)));
                        match bound {
                            Some((picture, held)) => {
                                self.on_screen = OnScreen::Imported { picture, size, _held: Some(held) };
                            }
                            None => slot.refuse_imports(),
                        }
                    }
                }
            }
            #[cfg(target_os = "linux")]
            Frame::Captured(captured) => {
                let size = (captured.layout().width, captured.layout().height);
                let card = slot.wanted().and_then(|within| media::card(size, within));
                match (painter.import_captured(device, queue, captured), card) {
                    (Ok(picture), Some(card)) => self.draw_card(device, queue, painter, &picture, card),
                    (Ok(picture), None) => self.on_screen = OnScreen::Imported { picture, size },
                    (Err(_), _) => slot.refuse_imports(),
                }
            }
        }
        Ok(())
    }

    /// Whether the picture on screen is planes in memory of `size` and `layout`, whose textures the next takes.
    fn holds(&self, size: (u32, u32), layout: media::PictureLayout) -> bool {
        matches!(&self.on_screen, OnScreen::Planes(gpu) if gpu.size() == size && gpu.layout() == layout)
    }

    /// `captured`, imported, drawn into the card's texture, which is made again only when the card's size changes.
    #[cfg(target_os = "linux")]
    fn draw_card(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        painter: &Painter,
        captured: &Imported,
        card: media::Card,
    ) {
        let kept = matches!(&self.on_screen, OnScreen::Card { texture, size, .. }
            if (texture.width(), texture.height()) == card.texture && *size == card.picture);
        if !kept {
            let texture = painter.target(device, card.texture);
            let Some(picture) = painter.bind_rgb(device, texture.clone(), card.picture) else { return };
            self.on_screen =
                OnScreen::Card { texture, picture, framing: painter.framing(device, queue), size: card.picture };
        }
        if let OnScreen::Card { texture, framing, size, .. } = &mut self.on_screen {
            painter.draw_into(device, queue, captured, framing, texture, *size);
        }
    }

    /// Shows nothing once `slot` was emptied: the watch it held ended. Asked before each take.
    pub fn follow(&mut self, slot: &Slot) {
        if slot.take_emptied() {
            *self = Self::default();
        }
    }

    /// Lets go of the imports whose decoder lends no more. Done after each [`Current::show`]; a window whose decoder
    /// ended while no picture arrives may call it too.
    pub fn trim(&mut self) {
        #[cfg(windows)]
        self.imports.trim();
    }

    /// The imports kept while the decoder lends from their pool.
    #[cfg(windows)]
    pub fn imports(&self) -> &Imports {
        &self.imports
    }

    /// The size of the picture on screen, if there is one.
    pub fn size(&self) -> Option<(u32, u32)> {
        match &self.on_screen {
            OnScreen::Nothing => None,
            OnScreen::Planes(gpu) => Some(gpu.size()),
            OnScreen::Imported { size, .. } => Some(*size),
            #[cfg(windows)]
            OnScreen::Lent { size, .. } => Some(*size),
            #[cfg(target_os = "linux")]
            OnScreen::Card { size, .. } => Some(*size),
        }
    }

    /// Draws the part of the picture on screen `framing` shows over the pass's whole viewport, or nothing when there
    /// is none.
    pub fn draw(&self, painter: &Painter, pass: &mut wgpu::RenderPass<'_>, framing: &Framing) {
        match &self.on_screen {
            OnScreen::Nothing => {}
            OnScreen::Planes(gpu) => painter.draw(pass, gpu, framing),
            OnScreen::Imported { picture, .. } => painter.draw_imported(pass, picture, framing),
            #[cfg(windows)]
            OnScreen::Lent { key, .. } => {
                if let Some(picture) = self.imports.get(*key) {
                    painter.draw_imported(pass, picture, framing);
                }
            }
            #[cfg(target_os = "linux")]
            OnScreen::Card { picture, .. } => painter.draw_imported(pass, picture, framing),
        }
    }
}
