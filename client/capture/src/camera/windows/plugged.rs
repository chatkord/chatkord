//! Cameras coming and going, as Windows' Configuration Manager tells them to the one registration a process holds and
//! it tells the hub: each video camera interface that arrives or is removed wakes whoever lists the cameras, and a
//! removed one that is open is stopped, which ends a read waiting on it. Media Foundation does not always tell a reader of a lost device;
//! Microsoft's "Handling Video Device Loss" matches the removed interface's symbolic link, as this does.

use super::super::CameraError;
use super::super::hub::{Hub, hub};
use super::Stopper;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
    CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_0, CM_NOTIFY_FILTER_0_0,
    CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CM_Register_Notification, CM_Unregister_Notification, CR_SUCCESS,
    HCMNOTIFICATION,
};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Media::KernelStreaming::KSCATEGORY_VIDEO_CAMERA;

/// A camera's symbolic link as the hub knows it: Windows writes one link in more than one case.
pub(super) fn device(link: &str) -> String {
    link.to_lowercase()
}

/// The process's registration, alive while a watch or an open camera holds it.
struct Plugged {
    handle: HCMNOTIFICATION,
}

// SAFETY: the handle is only used again by `drop`.
unsafe impl Send for Plugged {}
// SAFETY: nothing is reached through `&Plugged`.
unsafe impl Sync for Plugged {}

impl Drop for Plugged {
    fn drop(&mut self) {
        // SAFETY: our registration, unregistered once; it returns after a callback under way has.
        unsafe { CM_Unregister_Notification(self.handle) };
    }
}

/// The registration, made by the first holder and shared by the rest.
fn plugged() -> Result<Arc<Plugged>, CameraError> {
    static ONE: Mutex<Weak<Plugged>> = Mutex::new(Weak::new());
    let mut held = ONE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(running) = held.upgrade() {
        return Ok(running);
    }
    let filter = CM_NOTIFY_FILTER {
        cbSize: u32::try_from(size_of::<CM_NOTIFY_FILTER>()).map_err(|_| CameraError::Unavailable)?,
        FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
        u: CM_NOTIFY_FILTER_0 { DeviceInterface: CM_NOTIFY_FILTER_0_0 { ClassGuid: KSCATEGORY_VIDEO_CAMERA } },
        ..CM_NOTIFY_FILTER::default()
    };
    let mut handle = HCMNOTIFICATION::default();
    let context: *const Hub = hub();
    // SAFETY: the filter lives across the call; the hub is a static, so its pointer outlives the registration.
    let said =
        unsafe { CM_Register_Notification(&raw const filter, Some(context.cast()), Some(told), &raw mut handle) };
    if said != CR_SUCCESS {
        return Err(CameraError::Unavailable);
    }
    let running = Arc::new(Plugged { handle });
    *held = Arc::downgrade(&running);
    Ok(running)
}

/// The cameras coming and going, told to a listener until this is dropped.
pub struct Watch {
    _plugged: Arc<Plugged>,
    id: u64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        hub().unlisten(self.id);
    }
}

/// Calls `changed`, from a thread of the system's, whenever a camera arrives or is removed.
pub fn watch(changed: impl Fn() + Send + Sync + 'static) -> Result<Watch, CameraError> {
    let plugged = plugged()?;
    Ok(Watch { _plugged: plugged, id: hub().listen(changed) })
}

/// An open camera known by its link, stopped if it is removed, until this is dropped.
pub(super) struct Held {
    _plugged: Arc<Plugged>,
    id: u64,
}

impl Drop for Held {
    fn drop(&mut self) {
        hub().release(self.id);
    }
}

/// Holds the camera of `link` for the hub, to be stopped by `stopper` if it is removed. None where the system will not
/// tell, when only Media Foundation's own answers end a read.
pub(super) fn hold(link: &str, stopper: Stopper) -> Option<Held> {
    let plugged = plugged().ok()?;
    Some(Held { _plugged: plugged, id: hub().hold(&device(link), stopper) })
}

/// As the registration does for a removal of `link`: for a test, which cannot unplug a camera.
#[cfg(test)]
pub(super) fn removed(link: &str) {
    hub().removed(&device(link));
}

/// The symbolic link an interface's event names: UTF-16 to its end or its first NUL, within the `size` bytes given.
///
/// # Safety
///
/// `data` points at `size` readable bytes the Configuration Manager wrote for an interface's event.
unsafe fn link_of(data: *const CM_NOTIFY_EVENT_DATA, size: u32) -> String {
    let start = std::mem::offset_of!(CM_NOTIFY_EVENT_DATA, u);
    let units = (size as usize).saturating_sub(start + size_of::<windows::core::GUID>()) / 2;
    // SAFETY: the interface's link follows its class's GUID in the union, and the caller gave `size` readable bytes.
    let first = unsafe { (&raw const (*data).u.DeviceInterface.SymbolicLink).cast::<u16>() };
    // SAFETY: `units` whole UTF-16 units lie from `first` inside the bytes given.
    let link = unsafe { std::slice::from_raw_parts(first, units) };
    let end = link.iter().position(|unit| *unit == 0).unwrap_or(link.len());
    String::from_utf16_lossy(&link[..end])
}

/// Whether an action is a camera coming or going.
fn moves(action: CM_NOTIFY_ACTION) -> bool {
    action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL || action == CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL
}

unsafe extern "system" fn told(
    _: HCMNOTIFICATION,
    context: *const core::ffi::c_void,
    action: CM_NOTIFY_ACTION,
    data: *const CM_NOTIFY_EVENT_DATA,
    size: u32,
) -> u32 {
    if !moves(action) {
        return ERROR_SUCCESS.0;
    }
    // SAFETY: the context is the hub `plugged` registered, a static.
    let hub = unsafe { &*context.cast::<Hub>() };
    if action == CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL && !data.is_null() {
        // SAFETY: the Configuration Manager's event data for an interface, `size` bytes of it.
        hub.removed(&device(&unsafe { link_of(data, size) }));
    }
    hub.changed();
    ERROR_SUCCESS.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::Win32::Devices::DeviceAndDriverInstallation::CM_NOTIFY_ACTION_DEVICEQUERYREMOVE;

    /// An interface's event as the Configuration Manager writes it: its header, its class, then its link.
    fn event(link: &str) -> Vec<u16> {
        let head = (std::mem::offset_of!(CM_NOTIFY_EVENT_DATA, u) + size_of::<windows::core::GUID>()) / 2;
        let mut data = vec![0_u16; head];
        data.extend(link.encode_utf16());
        data.push(0);
        data
    }

    #[test]
    fn an_event_s_link_is_read_to_its_end_and_never_past_its_size() {
        let data = event(r"\\?\USB#VID_0000&PID_0000#1#{e5323777-f976-4f5b-9b55-b94699c46e44}\GLOBAL");
        let size = u32::try_from(data.len() * 2).unwrap();
        // SAFETY: the vector holds `size` bytes, laid out as an event's.
        let link = unsafe { link_of(data.as_ptr().cast(), size) };
        assert_eq!(link, r"\\?\USB#VID_0000&PID_0000#1#{e5323777-f976-4f5b-9b55-b94699c46e44}\GLOBAL");
        // SAFETY: as above, told fewer bytes.
        let cut = unsafe { link_of(data.as_ptr().cast(), size - 20) };
        assert!(link.starts_with(&cut) && cut.len() < link.len(), "read within the size given: {cut}");
    }

    #[test]
    fn a_camera_coming_or_going_wakes_the_listeners_and_another_action_does_not() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&woken);
        let _watch = watch(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        })
        .expect("the registration");
        let context: *const Hub = hub();
        let data = event("a camera");
        let size = u32::try_from(data.len() * 2).unwrap();
        for action in [
            CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL,
            CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL,
            CM_NOTIFY_ACTION_DEVICEQUERYREMOVE,
        ] {
            // SAFETY: the hub is a static, and the event's bytes are `size` long.
            unsafe { told(HCMNOTIFICATION::default(), context.cast(), action, data.as_ptr().cast(), size) };
        }
        assert_eq!(woken.load(Ordering::Relaxed), 2);
    }
}
