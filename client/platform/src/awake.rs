//! Keeping the display or the machine awake while a call needs it: a power request on Windows, the Inhibit portal's
//! request on Linux, each given the reason the system may list it by. One hold is kept for the whole run and told what
//! to keep; dropped, it keeps nothing.

use crate::Unavailable;
use screens::Awake;
use tokio::sync::watch;

/// What is kept, and the reason the system lists it by.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Kept {
    awake: Awake,
    reason: String,
}

/// The hold: told what to keep, it is carried out by a task of its own, so the last thing told is what is kept.
pub struct Hold {
    told: watch::Sender<Kept>,
}

impl Hold {
    /// The hold, keeping nothing yet, and the task that carries out what it is told until it is dropped; the system's
    /// request goes with it. A system with no way to ask ends the task [`Unavailable`].
    pub fn new() -> (Hold, impl Future<Output = Result<(), Unavailable>> + Send + 'static) {
        let (told, kept) = watch::channel(Kept { awake: Awake::Free, reason: String::new() });
        (Hold { told }, serve(kept))
    }

    /// Keeps what `awake` says, naming `reason` to the system; a change of either is carried out, the same again is not.
    pub fn keep(&self, awake: Awake, reason: &str) {
        self.told.send_if_modified(|kept| {
            let changed = kept.awake != awake || kept.reason != reason;
            if changed {
                *kept = Kept { awake, reason: reason.to_string() };
            }
            changed
        });
    }
}

async fn serve(kept: watch::Receiver<Kept>) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        portal::serve(kept).await
    }
    #[cfg(windows)]
    {
        power::serve(kept).await
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        drop(kept);
        Err(Unavailable)
    }
}

/// The Inhibit portal's request for as long as something is kept: idle and suspend for the display, suspend for the
/// machine, closed when what is kept changes or the hold is dropped.
#[cfg(target_os = "linux")]
mod portal {
    use super::{Awake, Kept, Unavailable};
    use ashpd::desktop::Request;
    use ashpd::desktop::inhibit::{InhibitFlags, InhibitOptions, InhibitProxy};
    use tokio::sync::watch;

    pub(super) async fn serve(mut kept: watch::Receiver<Kept>) -> Result<(), Unavailable> {
        let proxy = InhibitProxy::new().await.map_err(|_| Unavailable)?;
        let mut request: Option<Request<()>> = None;
        loop {
            let now = kept.borrow_and_update().clone();
            if let Some(old) = request.take() {
                // A request that cannot be closed ends with the connection, which this hold's end closes.
                let _ = old.close().await;
            }
            let flags = match now.awake {
                Awake::Free => None,
                Awake::System => Some(InhibitFlags::Suspend.into()),
                Awake::Display => Some(InhibitFlags::Idle | InhibitFlags::Suspend),
            };
            if let Some(flags) = flags {
                let options = InhibitOptions::default().set_reason(now.reason.as_str());
                request = Some(proxy.inhibit(None, flags, options).await.map_err(|_| Unavailable)?);
            }
            if kept.changed().await.is_err() {
                break;
            }
        }
        if let Some(old) = request {
            let _ = old.close().await;
        }
        Ok(())
    }
}

/// One power request for the run, its display and system settings following what is kept, cleared with it.
#[cfg(windows)]
mod power {
    use super::{Awake, Kept, Unavailable};
    use tokio::sync::watch;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired, PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    /// `POWER_REQUEST_CONTEXT_VERSION`, the only version of the context there is.
    const CONTEXT_VERSION: u32 = 0;

    /// A power request the system lists by its reason, and what its kinds are set to keep.
    struct Request {
        handle: HANDLE,
        set: Awake,
    }

    // SAFETY: a power request's handle is a kernel object usable from any thread; this one is used by one task at a time.
    unsafe impl Send for Request {}

    impl Request {
        fn new(reason: &str) -> Result<Request, Unavailable> {
            let mut wide: Vec<u16> = reason.encode_utf16().chain(std::iter::once(0)).collect();
            let context = REASON_CONTEXT {
                Version: CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 { SimpleReasonString: wide.as_mut_ptr() },
            };
            // SAFETY: `context` and the null-terminated string it points to live through the call, which copies both.
            let handle = unsafe { PowerCreateRequest(std::ptr::from_ref(&context)) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(Unavailable);
            }
            Ok(Request { handle, set: Awake::Free })
        }

        /// Sets or clears each kind of the request so that it keeps what `awake` says.
        fn keep(&mut self, awake: Awake) -> Result<(), Unavailable> {
            let display = |awake: Awake| awake == Awake::Display;
            let system = |awake: Awake| awake != Awake::Free;
            for (kind, wanted, was) in [
                (PowerRequestDisplayRequired, display(awake), display(self.set)),
                (PowerRequestSystemRequired, system(awake), system(self.set)),
            ] {
                if wanted == was {
                    continue;
                }
                let done = if wanted {
                    // SAFETY: `handle` is the live power request this value owns.
                    unsafe { PowerSetRequest(self.handle, kind) }
                } else {
                    // SAFETY: `handle` is the live power request this value owns.
                    unsafe { PowerClearRequest(self.handle, kind) }
                };
                if done == 0 {
                    return Err(Unavailable);
                }
            }
            self.set = awake;
            Ok(())
        }
    }

    impl Drop for Request {
        fn drop(&mut self) {
            // SAFETY: `handle` is the live power request this value owns; closing it clears what it set.
            unsafe { CloseHandle(self.handle) };
        }
    }

    pub(super) async fn serve(mut kept: watch::Receiver<Kept>) -> Result<(), Unavailable> {
        let mut request: Option<Request> = None;
        loop {
            let now = kept.borrow_and_update().clone();
            if request.is_none() && now.awake != Awake::Free {
                request = Some(Request::new(&now.reason)?);
            }
            if let Some(request) = &mut request {
                request.keep(now.awake)?;
            }
            if kept.changed().await.is_err() {
                return Ok(());
            }
        }
    }
}
