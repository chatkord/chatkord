//! MMCSS, the service Windows schedules sound and video threads through.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW};
use windows::core::w;

/// Why Windows did not take a thread into the task.
#[derive(report::Public)]
pub(super) enum Refused {
    /// MMCSS said no, or does not run: its HRESULT.
    Mmcss(#[public] i32),
}

/// The calling thread's place in MMCSS's "Pro Audio" task, given back when dropped on that thread.
pub(super) struct Held(HANDLE);

pub(super) fn ask() -> Result<Held, Refused> {
    let mut index = 0;
    // SAFETY: registers the calling thread alone under a task Windows ships; `index` is written during the call.
    unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &raw mut index) }
        .map(Held)
        .map_err(|why| Refused::Mmcss(why.code().0))
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the handle `ask` got for this thread, which a `Held` never leaves, given back once.
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

/// Whether Windows runs the calling thread above the priority a thread starts at.
#[cfg(test)]
pub(super) fn ahead() -> bool {
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_NORMAL};
    // SAFETY: takes nothing; the pseudo-handle it returns means the calling thread.
    let this = unsafe { GetCurrentThread() };
    // SAFETY: the calling thread's own pseudo-handle.
    unsafe { GetThreadPriority(this) > THREAD_PRIORITY_NORMAL.0 }
}
