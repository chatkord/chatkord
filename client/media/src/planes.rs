//! Pictures moved between layouts in system memory: a device's frame downloaded, one pixel format converted to
//! another at the same size, each into frames kept from one picture to the next.

use crate::av::{Frame, PixelFormat, Scaler};

/// What moving pictures keeps from one to the next: the converter, made again only when the size or a format
/// changes, the frames a device's picture is downloaded into and those another layout is converted into. They are
/// apart so a download can be read while the converter writes.
#[derive(Default)]
pub(crate) struct Planes {
    /// The converter.
    pub(crate) scaler: Scaler,
    /// Brings a device's picture to memory.
    pub(crate) download: Download,
    /// What a picture of a layout the window does not take is converted into.
    pub(crate) converted: Kept,
}

/// How many frames of one kind are kept: the window holds at most two pictures in memory at once, the one waiting in
/// its slot and the one it uploads, so a third is free to be written and a steady stream allocates none.
const KEPT: usize = 3;

/// Frames kept from one picture to the next. One a picture still holds is never written again: another is.
#[derive(Default)]
pub(crate) struct Kept(Vec<Frame>);

impl Kept {
    /// A kept frame nothing else holds that `fits`, to be written in place; else one to be filled anew: an empty one
    /// while fewer than [`KEPT`] are kept, one nothing holds, or the first, whose holder keeps its buffers. None when no
    /// frame can be made.
    fn free(&mut self, fits: impl Fn(&Frame) -> bool) -> Option<&mut Frame> {
        let at = match self.0.iter().position(|frame| frame.is_writable() && fits(frame)) {
            Some(at) => at,
            None if self.0.len() < KEPT => {
                self.0.push(Frame::new()?);
                self.0.len() - 1
            }
            None => self.0.iter().position(Frame::is_writable).unwrap_or(0),
        };
        self.0.get_mut(at)
    }

    /// A kept frame of `format` and `size` with buffers nothing else holds, to be written.
    pub(crate) fn frame(&mut self, format: PixelFormat, size: (u32, u32)) -> Option<&mut Frame> {
        let fits = |frame: &Frame| frame.format() == Some(format) && frame.size() == size;
        let frame = self.free(fits)?;
        if !(frame.is_writable() && fits(frame)) {
            *frame = Frame::software(format, size).ok()?;
        }
        Some(frame)
    }
}

/// The frames a device's pictures are downloaded into, kept from one to the next.
#[derive(Default)]
pub(crate) struct Download(Kept);

impl Download {
    /// `frame` in system memory with its timestamp: itself, or its download into a kept frame, written over in place
    /// while it is the right size and nothing else holds it, so a picture handed on by reference is never written
    /// again under its holder. None when the download fails.
    pub(crate) fn in_memory<'a>(&'a mut self, frame: &'a Frame) -> Option<&'a Frame> {
        if !frame.is_hardware() {
            return Some(frame);
        }
        let (layout, size) = (frame.hardware_layout(), frame.size());
        let download = self.0.free(|kept| kept.format() == layout && kept.size() == size)?;
        download.fetch(frame).ok()?;
        download.set_pts(frame.pts());
        Some(&*download)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (64, 32);

    fn luma_of(frame: &Frame) -> *const u8 {
        frame.plane(0).expect("a plane").bytes.as_ptr()
    }

    #[test]
    fn a_kept_frame_a_picture_still_holds_is_never_written_and_one_let_go_of_is_written_again() {
        let mut kept = Kept::default();
        let first = kept.frame(PixelFormat::Nv12, SIZE).expect("a frame");
        let (first_luma, held) = (luma_of(first), first.reference().expect("a reference"));
        let second = luma_of(kept.frame(PixelFormat::Nv12, SIZE).expect("a frame"));
        assert_ne!(second, first_luma, "the held picture's buffer is not handed out again");
        assert_eq!(luma_of(&held), first_luma, "and its holder still reads its own");
        drop(held);
        let again = luma_of(kept.frame(PixelFormat::Nv12, SIZE).expect("a frame"));
        assert!(again == first_luma || again == second, "a frame let go of is written again, none made");
    }

    #[test]
    fn every_kept_frame_held_has_its_place_filled_anew_and_the_holders_keep_theirs() {
        let mut kept = Kept::default();
        let held: Vec<Frame> = (0..=KEPT)
            .map(|_| kept.frame(PixelFormat::Nv12, SIZE).and_then(|frame| frame.reference()).expect("a picture"))
            .collect();
        let lumas: Vec<*const u8> = held.iter().map(luma_of).collect();
        let unique: std::collections::HashSet<_> = lumas.iter().collect();
        assert_eq!(unique.len(), held.len(), "no two pictures share a buffer");
        assert_eq!(kept.0.len(), KEPT, "no more than {KEPT} kept");
    }
}
