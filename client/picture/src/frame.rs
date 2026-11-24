//! Where a received picture waits for the window. The engine takes its next picture only when a put returns, so a put
//! never waits on a painter: one the window did not take before the next is dropped and counted, never queued.

use net::Backoff;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A picture waiting for the window: planes in memory, or one on the GPU that the window imports without a copy.
#[derive(Debug)]
pub enum Frame {
    /// Planes in memory, copied up when painted.
    Planes(Planes),
    /// A decoded picture in memory, the decoder's own planes or its download, copied up when painted and then let go.
    InMemory(media::Picture),
    /// A decoder's surface left on the GPU, given back to the decoder when dropped.
    #[cfg(target_os = "linux")]
    Surface(media::Surface),
    /// A picture on the window's own D3D12 device: the decoder's, or one's own share's.
    #[cfg(windows)]
    OnWindow(crate::d3d12::OnWindow),
    /// This device's own captured screen, lent while the encoder reads it too.
    #[cfg(target_os = "linux")]
    Captured(capture::Frame),
}

impl Frame {
    /// Whether it is one's own screen, lent by its producer and wanted back.
    fn is_lent(&self) -> bool {
        match self {
            Self::Planes(_) | Self::InMemory(_) => false,
            #[cfg(target_os = "linux")]
            Self::Surface(_) => false,
            #[cfg(target_os = "linux")]
            Self::Captured(_) => true,
            #[cfg(windows)]
            Self::OnWindow(picture) => matches!(picture, crate::d3d12::OnWindow::Preview(_)),
        }
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Planes(planes) => (planes.width, planes.height),
            Self::InMemory(picture) => picture.size(),
            #[cfg(target_os = "linux")]
            Self::Surface(surface) => (surface.width(), surface.height()),
            #[cfg(windows)]
            Self::OnWindow(picture) => picture.size(),
            #[cfg(target_os = "linux")]
            Self::Captured(frame) => (frame.layout().width, frame.layout().height),
        }
    }

    /// The matrix and range its samples are painted by; none for an RGB picture, which has neither.
    pub fn colours(&self) -> Option<media::Colours> {
        match self {
            Self::Planes(planes) => Some(planes.colours),
            Self::InMemory(picture) => Some(picture.colours()),
            #[cfg(target_os = "linux")]
            Self::Surface(surface) => Some(surface.colours()),
            #[cfg(windows)]
            Self::OnWindow(picture) => picture.colours(),
            #[cfg(target_os = "linux")]
            Self::Captured(_) => None,
        }
    }
}

/// One picture as three tightly packed planes (no row padding); the chroma planes are half the width and height,
/// rounded up.
#[derive(Debug)]
pub struct Planes {
    /// In pixels.
    pub width: u32,
    /// In pixels.
    pub height: u32,
    /// One sample a pixel, row after row.
    pub luma: Vec<u8>,
    /// The blue-difference chroma (Cb).
    pub blue: Vec<u8>,
    /// The red-difference chroma (Cr).
    pub red: Vec<u8>,
    /// What the samples are in, which they are painted by.
    pub colours: media::Colours,
}

impl Planes {
    /// Luma, blue and red, each with its bytes from one row to the next, as [`crate::Gpu::upload`] takes them.
    pub fn planes(&self) -> [(&[u8], u32); 3] {
        let half = self.width.div_ceil(2);
        [(&self.luma, self.width), (&self.blue, half), (&self.red, half)]
    }
}

/// Why a picture was not taken. The sender is the engine, so its sizes are checked where they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// A picture with no area.
    Empty,
    /// A row stride shorter than a row of the plane.
    Stride,
    /// A plane shorter than its rows and stride say.
    Short,
}

/// What the slot has seen since it was made or last emptied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Pictures put in.
    pub received: u64,
    /// Pictures replaced before the window took them.
    pub dropped: u64,
}

/// When the window last took a picture to paint, and the widest wait between two it took since the gap was read.
#[derive(Default)]
struct Painting {
    last: Option<Instant>,
    widest_gap: Duration,
}

impl Painting {
    /// The window took a picture at `now`.
    fn took(&mut self, now: Instant) {
        if let Some(last) = self.last.replace(now) {
            self.widest_gap = self.widest_gap.max(now.saturating_duration_since(last));
        }
    }
}

#[derive(Default)]
struct State {
    waiting: Option<Frame>,
    painting: Painting,
    /// A buffer the window gave back, so a steady stream allocates nothing.
    spare: Option<Planes>,
    stats: Stats,
    /// The size of the newest picture put in.
    size: Option<(u32, u32)>,
    /// Emptied since the window last looked, so the picture it shows is no longer the one watched.
    emptied: bool,
    /// The size the window draws the picture at, in pixels, for a producer that can draw it that size.
    wanted: Option<(u32, u32)>,
    /// The window's last refusal of an import in this watch, if any.
    refused: Option<Refused>,
}

/// The window could not import a picture: imports are offered again at `again_at`, after a wait that doubles at each
/// refusal of one watch, so a device that never imports costs a failed import now and then and one that failed once
/// is back on the GPU within a second.
#[derive(Clone, Copy)]
struct Refused {
    again_at: Instant,
    /// Refusals of this watch so far, the wait's count of tries.
    tries: u32,
}

/// The wait after a watch's first refusal, doubling to the longest, which seven refusals in one watch reach.
const BACKOFF: Backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(64));

/// The newest picture, shared by the engine's task (which puts) and the window (which takes).
#[derive(Clone)]
pub struct Slot {
    state: Arc<Mutex<State>>,
    /// Told when a picture arrives, so a window sleeps until it has something to paint.
    waker: Arc<Mutex<Option<Wake>>>,
    /// Set while the window is hidden or minimised: one's own screen is lent nothing, as nobody sees it.
    hidden: Arc<AtomicBool>,
    /// The slot's one source of time, which a refused import's wait is measured on.
    clock: fn() -> Instant,
}

impl Default for Slot {
    fn default() -> Self {
        Self { state: Arc::default(), waker: Arc::default(), hidden: Arc::default(), clock: Instant::now }
    }
}

type Wake = Arc<dyn Fn() + Send + Sync>;

impl State {
    fn takes_imports(&self, now: Instant) -> bool {
        self.refused.is_none_or(|refused| now >= refused.again_at)
    }
}

impl std::fmt::Debug for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot").finish_non_exhaustive()
    }
}

fn rounded_half(n: u32) -> u32 {
    n.div_ceil(2)
}

/// Copy `rows` rows of `row` bytes out of a plane that has `stride` bytes a row.
fn pack(into: &mut Vec<u8>, from: &[u8], stride: usize, row: usize, rows: usize) -> Result<(), Malformed> {
    if stride < row {
        return Err(Malformed::Stride);
    }
    // The last row needs only `row` bytes, not a whole stride.
    if rows > 0 && from.len() < (rows - 1) * stride + row {
        return Err(Malformed::Short);
    }
    into.clear();
    into.reserve(row * rows);
    for r in 0..rows {
        into.extend_from_slice(&from[r * stride..r * stride + row]);
    }
    Ok(())
}

impl Slot {
    /// An empty slot that takes imports.
    pub fn new() -> Self {
        Self::default()
    }

    /// The state is plain data and every writer leaves it whole, so a panic elsewhere does not make it unusable.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Call `wake` after every picture put in from now on, replacing any earlier one.
    pub fn wake_with(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(wake));
    }

    /// Put the newest picture in, its samples in `colours`, replacing one nobody took. The copy happens outside the
    /// lock, so the window never waits on it.
    pub fn put(
        &self,
        (width, height): (u32, u32),
        (luma, blue, red): (&[u8], &[u8], &[u8]),
        (luma_stride, blue_stride, red_stride): (u32, u32, u32),
        colours: media::Colours,
    ) -> Result<(), Malformed> {
        if width == 0 || height == 0 {
            return Err(Malformed::Empty);
        }
        let mut planes = self.lock().spare.take().unwrap_or(Planes {
            width,
            height,
            luma: Vec::new(),
            blue: Vec::new(),
            red: Vec::new(),
            colours,
        });
        let (half_width, half_height) = (rounded_half(width) as usize, rounded_half(height) as usize);
        pack(&mut planes.luma, luma, luma_stride as usize, width as usize, height as usize)?;
        pack(&mut planes.blue, blue, blue_stride as usize, half_width, half_height)?;
        pack(&mut planes.red, red, red_stride as usize, half_width, half_height)?;
        (planes.width, planes.height, planes.colours) = (width, height, colours);
        self.arrive(Frame::Planes(planes));
        Ok(())
    }

    /// Put a picture in from NV12 (packed luma, then interleaved blue and red) in `colours`, as one's own camera has it,
    /// unless the window is hidden; the chroma is split into buffers the window gave back, so no second copy is made.
    pub fn put_nv12(
        &self,
        (width, height): (u32, u32),
        luma: &[u8],
        chroma: &[u8],
        colours: media::Colours,
    ) -> Result<(), Malformed> {
        if width == 0 || height == 0 {
            return Err(Malformed::Empty);
        }
        if !self.is_shown() {
            return Ok(());
        }
        let mut planes = self.lock().spare.take().unwrap_or(Planes {
            width,
            height,
            luma: Vec::new(),
            blue: Vec::new(),
            red: Vec::new(),
            colours,
        });
        let (half_width, half_height) = (rounded_half(width) as usize, rounded_half(height) as usize);
        pack(&mut planes.luma, luma, width as usize, width as usize, height as usize)?;
        if chroma.len() < half_width * 2 * half_height {
            return Err(Malformed::Short);
        }
        planes.blue.clear();
        planes.red.clear();
        for [blue, red] in chroma[..half_width * 2 * half_height].as_chunks::<2>().0 {
            planes.blue.push(*blue);
            planes.red.push(*red);
        }
        (planes.width, planes.height, planes.colours) = (width, height, colours);
        self.arrive(Frame::Planes(planes));
        Ok(())
    }

    /// Put a decoded picture in memory in as the newest, by reference, replacing one nobody took; its buffers are let
    /// go once the window has copied them up, or when replaced.
    pub fn put_picture(&self, picture: media::Picture) {
        self.arrive(Frame::InMemory(picture));
    }

    /// Put the decoder's own surface in as the newest picture, replacing one nobody took. The replaced surface is
    /// dropped, which gives it back to the decoder.
    #[cfg(target_os = "linux")]
    pub fn put_surface(&self, surface: media::Surface) {
        if self.is_shown() {
            self.arrive(Frame::Surface(surface));
        }
    }

    /// Put the decoder's picture on the window's D3D12 device in as the newest, replacing one nobody took; the replaced
    /// picture goes back to the decoder.
    #[cfg(windows)]
    pub fn put_d3d12(&self, surface: media::D3d12Surface) {
        self.arrive(Frame::OnWindow(crate::d3d12::OnWindow::Decoded(surface)));
    }

    /// Lend one's own share's picture, drawn at the size the window wants, unless the window refused one or has not
    /// taken the last. Whether it was lent; one not lent goes back to its ring.
    #[cfg(windows)]
    pub fn lend_preview(&self, make: impl FnOnce((u32, u32)) -> Option<media::PreviewPicture>) -> bool {
        let wanted = {
            let state = self.lock();
            if !self.lends(&state) || state.waiting.is_some() {
                return false;
            }
            state.wanted
        };
        match wanted.and_then(make) {
            Some(picture) => {
                self.arrive(Frame::OnWindow(crate::d3d12::OnWindow::Preview(picture)));
                true
            }
            None => false,
        }
    }

    /// Whether the window is shown. While it is not, one's own screen is lent nothing and the picture waiting goes
    /// back to its producer, so a hidden window holds none of the compositor's buffers.
    pub fn show(&self, shown: bool) {
        self.hidden.store(!shown, Ordering::Relaxed);
        if !shown {
            let mut state = self.lock();
            if state.waiting.as_ref().is_some_and(Frame::is_lent) {
                state.waiting = None;
            }
        }
    }

    /// Whether the window is shown: while it is not, a producer that makes pictures only for it makes none.
    pub fn is_shown(&self) -> bool {
        !self.hidden.load(Ordering::Relaxed)
    }

    /// Whether a lent picture may be put in: the window imports and is shown.
    fn lends(&self, state: &State) -> bool {
        state.takes_imports((self.clock)()) && !self.hidden.load(Ordering::Relaxed)
    }

    /// The window draws the picture at `size` pixels: a producer that can draw it that size does.
    pub fn want(&self, size: (u32, u32)) {
        self.lock().wanted = Some(size);
    }

    /// The size the window draws the picture at, once it has said.
    #[cfg(target_os = "linux")]
    pub(crate) fn wanted(&self) -> Option<(u32, u32)> {
        self.lock().wanted
    }

    /// Lend `frame`, a captured screen the encoder also reads, unless the window refused one or is hidden. One the
    /// window has not taken goes home in its place, so the window holds at most two of the compositor's buffers and
    /// none for longer than a picture it draws. Whether it was lent.
    #[cfg(target_os = "linux")]
    pub fn lend(&self, frame: &capture::Frame) -> bool {
        if !self.lends(&self.lock()) {
            return false;
        }
        let Some(again) = frame.lend_again() else { return false };
        let mut state = self.lock();
        if !state.waiting.as_ref().is_some_and(Frame::is_lent) {
            drop(state);
            self.arrive(Frame::Captured(again));
            return true;
        }
        // The compositor drops a frame it has no free buffer for, so one not drawn (a page without the card) must
        // not keep its buffer. The window was woken for the one replaced, and a page that never draws it takes none.
        state.stats.received += 1;
        state.stats.dropped += 1;
        let home = state.waiting.replace(Frame::Captured(again));
        drop(state);
        drop(home);
        true
    }

    fn arrive(&self, frame: Frame) {
        let mut state = self.lock();
        state.stats.received += 1;
        state.size = Some(frame.size());
        let replaced = state.waiting.replace(frame);
        if replaced.is_some() {
            state.stats.dropped += 1;
        }
        if let Some(Frame::Planes(unseen)) = replaced {
            state.spare = Some(unseen);
        }
        drop(state);
        self.wake();
    }

    /// Forgets the pictures of a watch that ended, their size, counts and refused imports, and tells the window to
    /// show none.
    pub fn empty(&self) {
        let mut state = self.lock();
        state.waiting = None;
        state.painting = Painting::default();
        state.stats = Stats::default();
        state.size = None;
        state.refused = None;
        state.emptied = true;
        drop(state);
        self.wake();
    }

    /// Whether the slot was emptied since the last ask.
    pub(crate) fn take_emptied(&self) -> bool {
        std::mem::take(&mut self.lock().emptied)
    }

    fn wake(&self) {
        let wake = self.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    /// The newest picture if one arrived since the last take.
    pub fn take(&self) -> Option<Frame> {
        let now = (self.clock)();
        let mut state = self.lock();
        let taken = state.waiting.take();
        if taken.is_some() {
            state.painting.took(now);
        }
        taken
    }

    /// The widest wait between two pictures the window took since this was last asked, which starts it over: what a
    /// person sees as the picture stalling, whatever arrived.
    pub fn take_painted_gap(&self) -> Duration {
        std::mem::take(&mut self.lock().painting.widest_gap)
    }

    /// The window could not import a picture: a decoder hands over planes in memory, a capture nothing, until the
    /// slot offers imports again.
    pub fn refuse_imports(&self) {
        let now = (self.clock)();
        let mut state = self.lock();
        let tries = state.refused.map_or(1, |last| last.tries.saturating_add(1));
        state.refused = Some(Refused { again_at: now + BACKOFF.wait(tries), tries });
    }

    /// Whether a producer may put surfaces or captures in.
    pub fn takes_imports(&self) -> bool {
        let now = (self.clock)();
        self.lock().takes_imports(now)
    }

    /// Hand a painted picture's buffers back for the next one.
    pub fn give_back(&self, planes: Planes) {
        self.lock().spare = Some(planes);
    }

    /// The counts since the slot was made or last emptied.
    pub fn stats(&self) -> Stats {
        self.lock().stats
    }

    /// The size of the newest picture put in, if there has been one.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.lock().size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (4, 2);

    std::thread_local! {
        static NOW: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }

    /// The test thread's clock, moved by `at`.
    fn now() -> Instant {
        NOW.with(|now| {
            let at = now.get().unwrap_or_else(Instant::now);
            now.set(Some(at));
            at
        })
    }

    fn at(later: Duration) {
        NOW.with(|now| now.set(Some(now.get().unwrap_or_else(Instant::now) + later)));
    }

    #[test]
    fn a_refused_import_is_offered_again_after_a_wait_that_doubles_until_the_watch_ends() {
        let slot = Slot { clock: now, ..Slot::new() };
        let tick = Duration::from_millis(1);
        assert!(slot.takes_imports());
        slot.refuse_imports();
        at(BACKOFF.wait(1) - tick);
        assert!(!slot.takes_imports(), "planes until the first wait is over");
        at(tick);
        assert!(slot.takes_imports(), "imports are offered again");
        slot.refuse_imports();
        at(BACKOFF.wait(2) - tick);
        assert!(!slot.takes_imports(), "a second refusal waits twice as long");
        at(tick);
        assert!(slot.takes_imports());
        for _ in 0..10 {
            slot.refuse_imports();
        }
        at(BACKOFF.wait(u32::MAX));
        assert!(slot.takes_imports(), "no wait is longer than the longest");
        slot.refuse_imports();
        slot.empty();
        assert!(slot.takes_imports(), "a new watch starts taking imports");
        slot.refuse_imports();
        at(BACKOFF.wait(1));
        assert!(slot.takes_imports(), "and its first refusal waits the first wait again");
    }

    fn planes(frame: Option<Frame>) -> Planes {
        match frame.expect("one waiting") {
            Frame::Planes(planes) => planes,
            Frame::InMemory(_) => panic!("these tests put planes"),
            #[cfg(target_os = "linux")]
            Frame::Surface(_) | Frame::Captured(_) => panic!("these tests put planes"),
            #[cfg(windows)]
            Frame::OnWindow(_) => panic!("these tests put planes"),
        }
    }

    /// A picture whose every sample is `n`, with rows padded to a stride.
    fn put_flat(slot: &Slot, n: u8) -> Result<(), Malformed> {
        let luma = vec![n; 6 * 2];
        let chroma = vec![n; 3];
        slot.put(SIZE, (&luma, &chroma, &chroma), (6, 3, 3), media::Colours::HD)
    }

    #[test]
    fn a_picture_comes_out_without_its_row_padding() {
        let slot = Slot::new();
        let mut luma = vec![0xEE; 6 * 2];
        luma[..4].copy_from_slice(&[1, 2, 3, 4]);
        luma[6..10].copy_from_slice(&[5, 6, 7, 8]);
        let chroma = [9, 10, 11];
        // 4x2 has chroma 2x1: two bytes a row, the third of the stride is padding.
        slot.put(SIZE, (&luma, &chroma, &chroma), (6, 3, 3), media::Colours::HD).expect("a whole picture");
        let planes = planes(slot.take());
        assert_eq!(planes.luma, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(planes.blue, [9, 10]);
        assert_eq!(planes.red, [9, 10]);
    }

    #[test]
    fn an_nv12_picture_is_split_into_its_planes() {
        let slot = Slot::new();
        let hd = media::Colours::HD;
        slot.put_nv12((4, 2), &[1, 2, 3, 4, 5, 6, 7, 8], &[10, 20, 11, 21], hd).expect("a whole picture");
        let planes = planes(slot.take());
        assert_eq!((planes.luma, planes.blue, planes.red), (vec![1, 2, 3, 4, 5, 6, 7, 8], vec![10, 11], vec![20, 21]));
        assert_eq!(slot.put_nv12((4, 2), &[0; 8], &[0; 3], hd), Err(Malformed::Short));
    }

    #[test]
    fn a_picture_waits_in_the_colours_it_was_put_in_even_in_a_buffer_the_window_gave_back() {
        let slot = Slot::new();
        let camera = media::Colours { matrix: media::Matrix::Bt601, range: media::Range::Limited };
        put_flat(&slot, 1).expect("first");
        let first = slot.take().expect("waits");
        assert_eq!(first.colours(), Some(media::Colours::HD));
        slot.give_back(planes(Some(first)));
        slot.put_nv12((4, 2), &[0; 8], &[0; 4], camera).expect("a whole picture");
        let second = slot.take().and_then(|frame| frame.colours());
        assert_eq!(second, Some(camera), "not the colours of the buffer's last use");
    }

    #[test]
    fn an_odd_size_rounds_its_chroma_up() {
        let slot = Slot::new();
        let luma = vec![1; 3 * 3];
        let chroma = vec![2; 2 * 2];
        slot.put((3, 3), (&luma, &chroma, &chroma), (3, 2, 2), media::Colours::HD).expect("3x3 has 2x2 chroma");
        let planes = planes(slot.take());
        assert_eq!((planes.luma.len(), planes.blue.len(), planes.red.len()), (9, 4, 4));
    }

    #[test]
    fn an_emptied_slot_forgets_its_pictures_and_tells_the_window_once() {
        let slot = Slot::new();
        put_flat(&slot, 1).expect("first");
        put_flat(&slot, 2).expect("second");
        assert!(!slot.take_emptied(), "nothing was emptied yet");
        slot.empty();
        assert!(slot.take().is_none(), "the waiting picture is gone");
        assert_eq!((slot.size(), slot.stats()), (None, Stats::default()));
        assert!(slot.take_emptied(), "the window is told");
        assert!(!slot.take_emptied(), "once");
        put_flat(&slot, 3).expect("a picture of the next watch");
        assert_eq!(planes(slot.take()).luma[0], 3);
        assert_eq!(slot.stats(), Stats { received: 1, dropped: 0 });
    }

    #[test]
    fn the_newest_picture_wins_and_the_one_it_replaced_is_counted_dropped() {
        let slot = Slot::new();
        put_flat(&slot, 1).expect("first");
        put_flat(&slot, 2).expect("second");
        put_flat(&slot, 3).expect("third");
        assert_eq!(planes(slot.take()).luma[0], 3);
        assert!(slot.take().is_none(), "a taken picture is not offered twice");
        assert_eq!(slot.stats(), Stats { received: 3, dropped: 2 });
        assert_eq!(slot.size(), Some(SIZE));
    }

    #[test]
    fn the_painted_gap_is_the_largest_wait_between_two_pictures_taken_and_starts_over_when_read() {
        let slot = Slot { clock: now, ..Slot::new() };
        let ms = Duration::from_millis;
        let take_after = |wait: Duration| {
            at(wait);
            put_flat(&slot, 1).expect("a picture");
            assert!(slot.take().is_some());
        };
        take_after(ms(0));
        assert_eq!(slot.take_painted_gap(), Duration::ZERO, "one picture has no gap");
        take_after(ms(16));
        take_after(ms(90));
        take_after(ms(20));
        assert_eq!(slot.take_painted_gap(), ms(90), "the largest, not the last");
        assert_eq!(slot.take_painted_gap(), Duration::ZERO, "read once");
        take_after(ms(30));
        assert_eq!(slot.take_painted_gap(), ms(30), "from the last picture taken, not from the reading");
    }

    #[test]
    fn a_take_that_finds_nothing_waiting_is_no_painted_picture_and_an_emptied_slot_forgets_the_gap() {
        let slot = Slot { clock: now, ..Slot::new() };
        put_flat(&slot, 1).expect("a picture");
        assert!(slot.take().is_some());
        at(Duration::from_millis(500));
        assert!(slot.take().is_none());
        put_flat(&slot, 2).expect("a picture");
        at(Duration::from_millis(5));
        assert!(slot.take().is_some());
        assert_eq!(slot.take_painted_gap(), Duration::from_millis(505), "a wait for a picture is the gap");
        slot.empty();
        put_flat(&slot, 3).expect("a picture");
        at(Duration::from_millis(900));
        assert!(slot.take().is_some());
        assert_eq!(slot.take_painted_gap(), Duration::ZERO, "a new watch starts with none");
    }

    #[test]
    fn a_taken_picture_is_not_a_drop_when_the_next_arrives() {
        let slot = Slot::new();
        put_flat(&slot, 1).expect("first");
        let planes = planes(slot.take());
        slot.give_back(planes);
        put_flat(&slot, 2).expect("second");
        assert_eq!(slot.stats(), Stats { received: 2, dropped: 0 });
    }

    #[test]
    fn a_malformed_picture_is_refused_and_leaves_the_waiting_one_alone() {
        let slot = Slot::new();
        put_flat(&slot, 7).expect("first");
        let luma = vec![0; 8];
        let chroma = vec![0; 2];
        let hd = media::Colours::HD;
        assert_eq!(slot.put((0, 2), (&luma, &chroma, &chroma), (4, 2, 2), hd), Err(Malformed::Empty));
        assert_eq!(slot.put(SIZE, (&luma, &chroma, &chroma), (3, 2, 2), hd), Err(Malformed::Stride));
        assert_eq!(slot.put(SIZE, (&luma[..7], &chroma, &chroma), (4, 2, 2), hd), Err(Malformed::Short));
        assert_eq!(slot.stats(), Stats { received: 1, dropped: 0 });
        assert_eq!(planes(slot.take()).luma[0], 7);
    }

    #[test]
    fn the_window_is_woken_by_each_picture_and_not_by_a_refused_one() {
        let slot = Slot::new();
        let woken = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = Arc::clone(&woken);
        slot.wake_with(move || {
            counting.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        put_flat(&slot, 1).expect("first");
        put_flat(&slot, 2).expect("second");
        assert_eq!(slot.put((0, 0), (&[], &[], &[]), (0, 0, 0), media::Colours::HD), Err(Malformed::Empty));
        assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn the_engine_task_and_the_window_can_use_the_slot_from_two_threads() {
        let slot = Slot::new();
        let putting = slot.clone();
        let engine = std::thread::spawn(move || {
            for n in 0..200u8 {
                put_flat(&putting, n).expect("a whole picture");
            }
        });
        let mut last = None;
        while !engine.is_finished() {
            if let Some(frame) = slot.take() {
                let planes = planes(Some(frame));
                last = Some(planes.luma[0]);
                slot.give_back(planes);
            }
        }
        engine.join().expect("the engine thread ends");
        if let Some(frame) = slot.take() {
            last = Some(planes(Some(frame)).luma[0]);
        }
        assert_eq!(last, Some(199), "the newest picture is the one the window ends on");
        let stats = slot.stats();
        assert_eq!(stats.received, 200);
    }

    /// A captured screen whose buffer counts the times it went home.
    #[cfg(target_os = "linux")]
    fn captured(home: &Arc<std::sync::atomic::AtomicU32>) -> capture::Frame {
        struct Home(Arc<std::sync::atomic::AtomicU32>);
        impl Drop for Home {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let fd = std::os::fd::OwnedFd::from(std::fs::File::open("/dev/null").unwrap());
        let dmabuf = capture::DmaBuf::new(fd, 0, 16, capture::Modifier::LINEAR, Home(home.clone())).unwrap();
        let layout = capture::Layout { pixels: capture::Pixels::Bgrx, width: 4, height: 2 };
        capture::Frame::new(layout, capture::Native::DmaBuf(dmabuf), std::time::Instant::now())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_capture_waits_for_the_window_in_the_place_of_one_not_taken_and_only_where_it_imports() {
        let slot = Slot::new();
        let (first, second) = (Arc::default(), Arc::default());
        let (one, two) = (captured(&first), captured(&second));
        let woken = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counted = Arc::clone(&woken);
        slot.wake_with(move || _ = counted.fetch_add(1, Ordering::SeqCst));
        assert!(slot.lend(&one));
        assert!(slot.lend(&two), "the newer replaces the one the window has not taken");
        drop((one, two));
        assert_eq!(first.load(Ordering::SeqCst), 1, "the one not taken went home, so no buffer is kept undrawn");
        assert_eq!(second.load(Ordering::SeqCst), 0, "the newer waits for the window");
        assert_eq!(woken.load(Ordering::SeqCst), 1, "the window is woken once, not for each replacement");
        drop(slot.take());
        assert_eq!(second.load(Ordering::SeqCst), 1);

        slot.refuse_imports();
        let three = captured(&Arc::default());
        assert!(!slot.lend(&three), "a window that cannot import is lent nothing");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_hidden_window_is_lent_nothing_and_gives_back_what_waited() {
        let slot = Slot::new();
        let (waited, while_hidden) = (Arc::default(), Arc::default());
        let one = captured(&waited);
        assert!(slot.lend(&one));
        drop(one);
        slot.show(false);
        assert_eq!(waited.load(Ordering::SeqCst), 1, "the picture waiting went home when the window hid");
        let two = captured(&while_hidden);
        assert!(!slot.lend(&two), "a hidden window is lent nothing");
        slot.show(true);
        assert!(slot.lend(&two), "shown again, it is lent the next");
    }

    #[test]
    fn a_hidden_window_is_put_no_picture_from_memory_and_a_shown_one_is() {
        let slot = Slot::new();
        let (luma, chroma) = (vec![16; 4 * 2], vec![128; 4]);
        slot.show(false);
        slot.put_nv12((4, 2), &luma, &chroma, media::Colours::HD).expect("a whole picture");
        assert_eq!(slot.stats().received, 0, "a hidden window takes none");
        slot.show(true);
        slot.put_nv12((4, 2), &luma, &chroma, media::Colours::HD).expect("a whole picture");
        assert_eq!(slot.stats().received, 1, "shown again it takes the next");
    }
}
