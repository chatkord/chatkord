//! The Vulkan loader scans and unloads driver libraries while it makes or ends a device, so two threads inside it at
//! once crash in `libvulkan`: one lock covers every entry into it, and nothing else. It guards no data, so a panic
//! while holding it is harmless. A software codec and a survey's walk do not enter the loader and do not take it; the
//! entries are Vulkan's instance and device makers, a hardware codec's open, and (kept until a measurement drops it) a
//! VA-API device's open.

#[cfg(target_os = "linux")]
use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
#[cfg(target_os = "linux")]
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(target_os = "linux")]
static LOADER: Mutex<()> = Mutex::new(());

#[cfg(target_os = "linux")]
thread_local! {
    /// How many [`Alone`]s this thread holds: only the outermost holds the lock.
    static HELD: Cell<u32> = const { Cell::new(0) };
}

/// The loader's lock, held until dropped. Drop it after the work's own contexts are released.
#[cfg(target_os = "linux")]
pub struct Alone {
    _held: Option<MutexGuard<'static, ()>>,
}

#[cfg(target_os = "linux")]
impl Drop for Alone {
    fn drop(&mut self) {
        HELD.with(|held| held.set(held.get().saturating_sub(1)));
    }
}

/// Takes the loader's lock for as long as the value lives. A thread that holds it already takes it again at once,
/// so a device released while its owner holds the lock (a reopen) ends under it without waiting on itself.
#[cfg(target_os = "linux")]
pub fn alone() -> Alone {
    // Everything that may make FFmpeg log enters the loader first.
    crate::log::forward();
    let outermost = HELD.with(|held| {
        let was = held.get();
        held.set(was.saturating_add(1));
        was == 0
    });
    Alone { _held: outermost.then(|| LOADER.lock().unwrap_or_else(PoisonError::into_inner)) }
}

/// A handle whose end may enter the loader (a GPU instance, adapter or device a toolkit holds), dropped under its
/// lock wherever its owner lets go of it; where there is no lock, simply dropped.
pub struct EndsAlone<T>(ManuallyDrop<T>);

impl<T> EndsAlone<T> {
    /// Holds `value` until it ends under the loader's lock.
    pub fn new(value: T) -> Self {
        Self(ManuallyDrop::new(value))
    }
}

impl<T> Deref for EndsAlone<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for EndsAlone<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Drop for EndsAlone<T> {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        let _alone = alone();
        // SAFETY: the value is dropped once, here, and never read after: `self` is being dropped.
        unsafe { ManuallyDrop::drop(&mut self.0) };
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn a_thread_that_holds_the_lock_takes_it_again_and_another_waits_for_the_outermost() {
        let outer = alone();
        let inner = alone();
        let (taken, told) = mpsc::channel();
        let other = thread::spawn(move || {
            let _alone = alone();
            let _ = taken.send(());
        });
        drop(inner);
        assert!(told.recv_timeout(Duration::from_millis(200)).is_err(), "the outer hold still keeps the lock");
        drop(outer);
        assert!(told.recv_timeout(Duration::from_secs(5)).is_ok(), "the lock is free once the outermost ends");
        other.join().expect("no thread panics");
    }
}
