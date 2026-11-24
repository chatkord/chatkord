//! A captured picture goes back to Windows Graphics Capture's pool only once the GPU has done every read of it queued
//! on the device: closing a frame lets the pool draw into it again, and nothing documents that Windows waits for work
//! still queued on the frame (Chromium's capturer waits for its copy on the processor before it lets one go). The wait
//! is on a thread of the capture's own, so the thread that drops a picture (the pool's callback among them) never
//! waits on the GPU.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Graphics::Capture::Direct3D11CaptureFrame;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D11::{D3D11_FENCE_FLAG_NONE, ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::Interface;

use super::Gpu;

/// The longest a picture waits for the GPU before it is closed all the same: a GPU that hung is not waited on for good.
const WAIT_MS: u32 = 1000;

/// Things closed in the order they were queued, each once the GPU has passed the fence value it was queued behind, by
/// a thread of its own.
pub(super) struct Line<T: Send + 'static> {
    sending: Mutex<Sending<T>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// The last fence value taken and the way to the thread, which is gone once the line is shut.
struct Sending<T> {
    last: u64,
    to: Option<Sender<(T, u64)>>,
}

impl<T: Send + 'static> Line<T> {
    /// A line whose thread calls `wait` with each value in turn, then `close` with what was queued behind it; none
    /// where the thread cannot start.
    pub(super) fn new(wait: impl Fn(u64) + Send + 'static, close: impl Fn(T) + Send + 'static) -> Option<Self> {
        let (to, from) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("capture-retire".into())
            .spawn(move || retiring(&from, &wait, &close))
            .ok()?;
        Some(Self { sending: Mutex::new(Sending { last: 0, to: Some(to) }), thread: Mutex::new(Some(thread)) })
    }

    /// Queues `item` behind the reads `signal` marks with the next value, which it returns false for where it could
    /// not mark them (the item is then closed without a wait); never waits on the GPU. The item comes back once the
    /// line is shut, for its owner to close.
    pub(super) fn push(&self, item: T, signal: impl FnOnce(u64) -> bool) -> Result<(), T> {
        let mut sending = self.sending.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(to) = sending.to.clone() else { return Err(item) };
        // Under the lock, so values are signalled and queued in one order.
        let value = sending.last + 1;
        let marked = signal(value);
        if marked {
            sending.last = value;
        }
        to.send((item, if marked { value } else { 0 })).map_err(|failed| failed.0.0)
    }

    /// Closes what is queued, then ends the thread; what is pushed after comes back from [`Line::push`].
    pub(super) fn shut(&self) {
        self.sending.lock().unwrap_or_else(PoisonError::into_inner).to = None;
        let thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(thread) = thread {
            // A thread that panicked closed nothing more; there is nothing left to do for it.
            let _ = thread.join();
        }
    }
}

impl<T: Send + 'static> Drop for Line<T> {
    fn drop(&mut self) {
        self.shut();
    }
}

/// The thread: each queued item once `wait` returns for its value, until every sender is gone and the queue is empty.
fn retiring<T>(from: &Receiver<(T, u64)>, wait: &impl Fn(u64), close: &impl Fn(T)) {
    while let Ok((item, value)) = from.recv() {
        wait(value);
        close(item);
    }
}

/// The fence a capture's pictures wait on before they go back, on the device the pool draws through.
pub(super) struct Retire {
    gpu: Gpu,
    context: ID3D11DeviceContext4,
    fence: ID3D11Fence,
    line: Line<Direct3D11CaptureFrame>,
}

/// Waits on the fence from the retire thread alone, which owns the event.
struct Waiter {
    fence: ID3D11Fence,
    event: HANDLE,
}

// SAFETY: the fence is free-threaded and the event handle is used by the one thread this moves to.
unsafe impl Send for Waiter {}

impl Waiter {
    /// Returns once the GPU has reached `value`, or after [`WAIT_MS`] where it has not.
    fn wait(&self, value: u64) {
        // SAFETY: a fence of the capture's device; reading its completed value waits on nothing.
        if unsafe { self.fence.GetCompletedValue() } >= value {
            return;
        }
        // SAFETY: the event is live and this thread's alone; the fence sets it at `value`.
        if unsafe { self.fence.SetEventOnCompletion(value, self.event) }.is_err() {
            return;
        }
        let until = Instant::now() + Duration::from_millis(u64::from(WAIT_MS));
        // An earlier value that was given up on may still set the event when the GPU reaches it, so a wake is checked.
        loop {
            // SAFETY: a fence of the capture's device; reading its completed value waits on nothing.
            if unsafe { self.fence.GetCompletedValue() } >= value {
                return;
            }
            let left = until.saturating_duration_since(Instant::now());
            let left = u32::try_from(left.as_millis()).unwrap_or(WAIT_MS);
            // SAFETY: the event is live and this thread's alone; the wait is bounded.
            if left == 0 || unsafe { WaitForSingleObject(self.event, left) } != WAIT_OBJECT_0 {
                break;
            }
        }
        report::note!(warn, "a captured picture went back to Windows before the GPU was done with it");
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        // SAFETY: the event was made with this waiter and is closed once.
        let _ = unsafe { CloseHandle(self.event) };
    }
}

impl Retire {
    /// The fence of `gpu`'s device, and the thread that closes pictures behind it; none where the device has no fences
    /// (before Windows 10's Creators Update) or the thread cannot start.
    pub(super) fn new(gpu: &Gpu) -> Option<Self> {
        let context = gpu.context().cast().ok()?;
        let made: ID3D11Device5 = gpu.device().cast().ok()?;
        let mut fence: Option<ID3D11Fence> = None;
        // SAFETY: the out-pointer is a live `Option` of the interface asked for.
        unsafe { made.CreateFence(0, D3D11_FENCE_FLAG_NONE, &raw mut fence) }.ok()?;
        let fence = fence?;
        // SAFETY: no name and no security attributes: an event of this process alone, closed with its waiter.
        let event = unsafe { CreateEventW(None, false, false, None) }.ok()?;
        let waiter = Waiter { fence: fence.clone(), event };
        let line = Line::new(
            move |value| waiter.wait(value),
            |frame: Direct3D11CaptureFrame| {
                // A frame that will not close is released with this value all the same.
                let _ = frame.Close();
            },
        )?;
        Some(Self { gpu: gpu.clone(), context, fence, line })
    }

    /// Gives `frame` back to the pool once the GPU has done the reads queued on the device so far, on the retire
    /// thread; returns at once.
    pub(super) fn retire(&self, frame: Direct3D11CaptureFrame) {
        // The device's lock first, then the line's, wherever both are taken, and the fence's values grow in the order
        // the signals are queued on the context.
        let _locked = self.gpu.lock();
        let signal = |value| {
            // SAFETY: the fence is this device's; the context is held under the device's lock, and the signal follows
            // every read of the picture queued on it.
            if unsafe { self.context.Signal(&self.fence, value) }.is_err() {
                return false;
            }
            // SAFETY: as above.
            unsafe { self.context.Flush() };
            true
        };
        if let Err(frame) = self.line.push(frame, signal) {
            let _ = frame.Close();
        }
    }

    /// Closes the pictures still queued and ends the thread: the capture is over, and what is dropped after closes
    /// at once.
    pub(super) fn shut(&self) {
        self.line.shut();
    }
}

// SAFETY: the D3D11 objects belong to a multithread-protected device and the context is used only under its lock.
unsafe impl Send for Retire {}
// SAFETY: as above.
unsafe impl Sync for Retire {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Long enough for a thread that is not blocked to act, short against a test that would hang.
    const PROMPTLY: Duration = Duration::from_secs(5);

    type Closed = Arc<Mutex<Vec<u32>>>;

    fn closes() -> (Closed, impl Fn(u32) + Send + 'static) {
        let closed = Closed::default();
        let into = Arc::clone(&closed);
        (closed, move |item| into.lock().unwrap().push(item))
    }

    #[test]
    fn dropping_a_picture_never_waits_for_a_gpu_that_has_not_finished() {
        let (entered, entering) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let (closed, close) = closes();
        let line = Arc::new(
            Line::new(
                move |_| {
                    let _ = entered.send(());
                    // Never completes until the test says.
                    let _ = released.lock().unwrap().recv();
                },
                close,
            )
            .unwrap(),
        );
        let (dropped, dropping) = mpsc::channel();
        let queue = Arc::clone(&line);
        std::thread::spawn(move || {
            queue.push(7, |_| true).unwrap();
            let _ = dropped.send(());
        });
        assert!(dropping.recv_timeout(PROMPTLY).is_ok(), "the thread that drops the picture was held by the GPU");
        assert!(entering.recv_timeout(PROMPTLY).is_ok(), "the retire thread waits on its own");
        assert!(closed.lock().unwrap().is_empty(), "closed before the GPU was done");
        release.send(()).unwrap();
        line.shut();
        assert_eq!(*closed.lock().unwrap(), [7]);
    }

    #[test]
    fn pictures_close_in_the_order_of_their_fence_values() {
        let waited = Arc::new(Mutex::new(Vec::new()));
        let waiting = Arc::clone(&waited);
        let (closed, close) = closes();
        let line = Line::new(move |value| waiting.lock().unwrap().push(value), close).unwrap();
        for item in 0..5 {
            line.push(item, |_| true).unwrap();
        }
        line.shut();
        assert_eq!(*waited.lock().unwrap(), [1, 2, 3, 4, 5], "values only grow");
        assert_eq!(*closed.lock().unwrap(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_picture_whose_reads_could_not_be_marked_is_closed_without_a_wait() {
        let waited = Arc::new(Mutex::new(Vec::new()));
        let waiting = Arc::clone(&waited);
        let (closed, close) = closes();
        let line = Line::new(move |value| waiting.lock().unwrap().push(value), close).unwrap();
        line.push(0, |_| false).unwrap();
        line.push(1, |_| true).unwrap();
        line.shut();
        assert_eq!(*waited.lock().unwrap(), [0, 1], "the failed mark waits on nothing and no value is skipped");
        assert_eq!(*closed.lock().unwrap(), [0, 1]);
    }

    #[test]
    fn dropping_the_line_closes_what_is_queued_and_ends_the_thread() {
        let finished = Arc::new(AtomicBool::new(false));
        let ending = Arc::clone(&finished);
        let (closed, close) = closes();
        let line = Line::new(
            |_| std::thread::sleep(Duration::from_millis(20)),
            move |item| {
                close(item);
                ending.store(true, Ordering::SeqCst);
            },
        )
        .unwrap();
        for item in 0..3 {
            line.push(item, |_| true).unwrap();
        }
        drop(line);
        assert_eq!(*closed.lock().unwrap(), [0, 1, 2], "drained before the drop returned");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn a_picture_dropped_after_the_line_is_shut_comes_back_to_be_closed_by_its_owner() {
        let (_, close) = closes();
        let line = Line::new(|_| {}, close).unwrap();
        line.shut();
        assert_eq!(line.push(9, |_| true), Err(9));
    }
}
