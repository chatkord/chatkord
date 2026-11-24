//! The servers' and people's pictures, emoji and thumbnails as image handles, by their file id, made once when a
//! picture is ready and painted by id: a file never changes, so a handle made for it is never stale. Each is held for
//! the servers that asked for it, within a bound of bytes: past it, the one painted longest ago that the last frame
//! did not draw is let go, and a server taken off or signed out takes its own with it. What a frame asked for and
//! found missing is told once, for the servers to ask for it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use iced_widget::image;
use portrait::Square;
use proto::files::FileId;
use screens::PlaceId;

/// The most the pictures hold, as RGBA, beside those the last frame drew: 384 of a member's 256-pixel squares (256
/// KiB each), screens of members, timeline and call over, or six of the largest original a viewer opens.
pub(crate) const HELD_MOST: usize = 96 * 1024 * 1024;

/// A picture held, and for whom.
struct Entry {
    handle: image::Handle,
    /// Its RGBA's length.
    bytes: usize,
    /// The servers that asked for it.
    owners: HashSet<PlaceId>,
    /// When it was last held or painted, in the order of [`Held::order`].
    used: u64,
}

#[derive(Default)]
struct Held {
    entries: HashMap<FileId, Entry>,
    /// Each entry by when it was last used, the longest ago first.
    order: BTreeMap<u64, FileId>,
    /// What the frame being drawn asked for.
    drawing: HashSet<FileId>,
    /// What the last frame drawn asked for: with `drawing`, never let go.
    drawn: HashSet<FileId>,
    /// Asked for and not held, not told yet.
    missed: Vec<FileId>,
    /// Told missing since each was last let go, so each is told once.
    told: HashSet<FileId>,
    /// The bytes held in all.
    bytes: usize,
    /// The next use's mark.
    clock: u64,
}

impl Held {
    fn touch(&mut self, file: FileId) {
        let Some(entry) = self.entries.get_mut(&file) else { return };
        self.order.remove(&entry.used);
        entry.used = self.clock;
        self.order.insert(self.clock, file);
        self.clock += 1;
    }

    fn drawn(&self, file: &FileId) -> bool {
        self.drawn.contains(file) || self.drawing.contains(file)
    }

    fn remove(&mut self, file: &FileId) -> Option<Entry> {
        let entry = self.entries.remove(file)?;
        self.order.remove(&entry.used);
        self.bytes -= entry.bytes;
        Some(entry)
    }

    /// Lets go of the pictures painted longest ago that the last frame did not draw, `held` aside, until the rest are
    /// within `most`; answers each let go, by every server that held it.
    fn bound(&mut self, most: usize, held: FileId) -> Vec<(PlaceId, FileId)> {
        let mut gone = Vec::new();
        let mut oldest = self.order.values().copied().collect::<Vec<_>>().into_iter();
        while self.bytes > most
            && let Some(file) = oldest.next()
        {
            // The one just held is wanted now: let go, it would be asked for again at once.
            if file != held
                && !self.drawn(&file)
                && let Some(entry) = self.remove(&file)
            {
                self.told.remove(&file);
                gone.extend(entry.owners.into_iter().map(|owner| (owner, file)));
            }
        }
        gone
    }
}

/// The pictures, shared by the window that paints them and the errands that bring them.
#[derive(Clone)]
pub(crate) struct Pictures {
    held: Arc<Mutex<Held>>,
    most: usize,
}

impl Default for Pictures {
    fn default() -> Pictures {
        Pictures::within(HELD_MOST)
    }
}

impl Pictures {
    /// None held yet, to hold at most `most` bytes beside what the last frame drew.
    fn within(most: usize) -> Pictures {
        Pictures { held: Arc::default(), most }
    }

    /// The handle painted for `picture`, drawn in this frame; none while it is not held, which is told once.
    pub(crate) fn get(&self, picture: FileId) -> Option<image::Handle> {
        let mut guard = self.held.lock().ok()?;
        let held = &mut *guard;
        held.drawing.insert(picture);
        let handle = held.entries.get(&picture).map(|entry| entry.handle.clone());
        match handle {
            Some(_) => held.touch(picture),
            None if held.told.insert(picture) => held.missed.push(picture),
            None => {}
        }
        handle
    }

    /// A frame begins: what the one before asked for is what is drawn now.
    pub(crate) fn frame(&self) {
        if let Ok(mut guard) = self.held.lock() {
            let held = &mut *guard;
            held.drawn = std::mem::take(&mut held.drawing);
        }
    }

    /// What the frames asked for and found missing since this was last asked, each once until it is let go.
    pub(crate) fn missed(&self) -> Vec<FileId> {
        self.held.lock().map(|mut held| std::mem::take(&mut held.missed)).unwrap_or_default()
    }

    /// Holds `square` as the handle painted for `picture`, for `place`; answers what was let go to make room.
    pub(crate) fn hold(&self, place: PlaceId, picture: FileId, square: Square) -> Vec<(PlaceId, FileId)> {
        let rgba = square.into_rgba();
        let bytes = rgba.len();
        self.keep(place, picture, bytes, || image::Handle::from_rgba(Square::SIDE, Square::SIDE, rgba))
    }

    /// Holds `picture`, a fetched file's, as the handle painted for `file`, for `place`; answers what was let go to
    /// make room.
    pub(crate) fn hold_file(&self, place: PlaceId, file: FileId, picture: portrait::Decoded) -> Vec<(PlaceId, FileId)> {
        let bytes = picture.rgba.len();
        self.keep(place, file, bytes, || image::Handle::from_rgba(picture.width, picture.height, picture.rgba))
    }

    /// `place` was taken off or signed out: what it alone held goes, and what is missed is told again, as the place
    /// forgets what it asked for.
    pub(crate) fn clear(&self, place: PlaceId) {
        let Ok(mut guard) = self.held.lock() else { return };
        let held = &mut *guard;
        held.told.clear();
        let orphans: Vec<FileId> = held
            .entries
            .iter_mut()
            .filter_map(|(file, entry)| (entry.owners.remove(&place) && entry.owners.is_empty()).then_some(*file))
            .collect();
        for file in orphans {
            held.remove(&file);
        }
    }

    fn keep(
        &self,
        place: PlaceId,
        file: FileId,
        bytes: usize,
        handle: impl FnOnce() -> image::Handle,
    ) -> Vec<(PlaceId, FileId)> {
        let Ok(mut guard) = self.held.lock() else { return Vec::new() };
        let held = &mut *guard;
        match held.entries.get_mut(&file) {
            Some(entry) => {
                entry.owners.insert(place);
            }
            None => {
                let entry = Entry { handle: handle(), bytes, owners: HashSet::from([place]), used: held.clock };
                held.entries.insert(file, entry);
                held.bytes += bytes;
                held.order.insert(held.clock, file);
                held.clock += 1;
            }
        }
        held.touch(file);
        held.bound(self.most, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of 64 bytes.
    const SIDE: u32 = 4;
    const BYTES: usize = 64;

    fn file(at: u8) -> FileId {
        FileId::from_bytes([at; FileId::LEN])
    }

    fn picture() -> portrait::Decoded {
        portrait::Decoded { width: SIDE, height: SIDE, rgba: vec![0; BYTES] }
    }

    fn places() -> (PlaceId, PlaceId) {
        use screens::harness::{books, owls};
        let rows = vec![screens::Opening::Server(owls()), screens::Opening::Server(books())];
        let (window, _) = screens::Window::launch(rows, None, std::time::Instant::now());
        (window.places()[0].id(), window.places()[1].id())
    }

    #[test]
    fn filled_past_its_bound_it_lets_go_of_the_least_painted_and_says_whose() {
        let (owls, _) = places();
        let pictures = Pictures::within(4 * BYTES);
        for at in 0..4 {
            assert!(pictures.hold_file(owls, file(at), picture()).is_empty(), "within the bound");
        }
        assert!(pictures.get(file(0)).is_some(), "painted, so the newest used");
        let gone = pictures.hold_file(owls, file(4), picture());
        assert_eq!(gone, [(owls, file(1))], "the one painted longest ago");
        assert!(pictures.get(file(1)).is_none());
        for at in 5..40 {
            let _ = pictures.hold_file(owls, file(at), picture());
            assert!(pictures.held.lock().unwrap().bytes <= 4 * BYTES, "after {at}");
        }
    }

    #[test]
    fn what_the_last_frame_drew_is_never_let_go() {
        let (owls, _) = places();
        let pictures = Pictures::within(2 * BYTES);
        let _ = pictures.hold_file(owls, file(0), picture());
        assert!(pictures.get(file(0)).is_some());
        pictures.frame();
        for at in 1..10 {
            let gone = pictures.hold_file(owls, file(at), picture());
            assert!(!gone.contains(&(owls, file(0))), "drawn");
        }
        pictures.frame();
        pictures.frame();
        let _ = pictures.hold_file(owls, file(10), picture());
        let _ = pictures.hold_file(owls, file(11), picture());
        assert!(pictures.get(file(0)).is_none(), "let go once no frame drew it");
    }

    #[test]
    fn a_picture_drawn_but_not_held_is_told_once_until_it_is_let_go() {
        let (owls, _) = places();
        let pictures = Pictures::within(BYTES);
        assert!(pictures.get(file(0)).is_none());
        assert!(pictures.get(file(0)).is_none());
        assert_eq!(pictures.missed(), [file(0)]);
        pictures.frame();
        assert!(pictures.get(file(0)).is_none());
        assert!(pictures.missed().is_empty(), "told once");
        let _ = pictures.hold_file(owls, file(0), picture());
        pictures.frame();
        pictures.frame();
        assert_eq!(pictures.hold_file(owls, file(1), picture()), [(owls, file(0))]);
        assert!(pictures.get(file(0)).is_none());
        assert_eq!(pictures.missed(), [file(0)], "let go, it is told again");
    }

    #[test]
    fn a_server_cleared_takes_what_it_alone_held() {
        let (owls, larks) = places();
        let pictures = Pictures::within(16 * BYTES);
        let _ = pictures.hold_file(owls, file(0), picture());
        let _ = pictures.hold_file(owls, file(1), picture());
        let _ = pictures.hold_file(larks, file(1), picture());
        let _ = pictures.hold_file(larks, file(2), picture());
        pictures.clear(owls);
        assert!(pictures.get(file(0)).is_none(), "owls' own");
        assert!(pictures.get(file(1)).is_some() && pictures.get(file(2)).is_some(), "larks' stay");
        assert_eq!(pictures.held.lock().unwrap().bytes, 2 * BYTES, "held once, counted once");
    }
}
