//! Windows' audio endpoints coming and going: every device added, removed, enabled or disabled, and every new default,
//! is one tick of the listener's count and a wake of the owner. A property changing (a name, a format) is no device
//! coming or going.

use super::Wake;
use tokio::sync::watch;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl,
};
use windows::core::{PCWSTR, Result, implement};

/// The registration with the owner's enumerator; dropping this unregisters it.
pub(crate) struct Endpoints {
    devices: IMMDeviceEnumerator,
    told: IMMNotificationClient,
}

impl Endpoints {
    /// Registers with `devices` for the endpoints' notifications, which Windows tells from threads of its own: each
    /// ticks `changed` and calls `wake`.
    pub(crate) fn listen(devices: &IMMDeviceEnumerator, changed: watch::Sender<u64>, wake: Wake) -> Result<Self> {
        let told: IMMNotificationClient = Told { changed, wake }.into();
        // SAFETY: `told` is a live COM object of ours, unregistered by `Drop` before it goes.
        unsafe { devices.RegisterEndpointNotificationCallback(&told) }?;
        Ok(Self { devices: devices.clone(), told })
    }
}

impl Drop for Endpoints {
    fn drop(&mut self) {
        // SAFETY: `told` was registered on this enumerator by `listen`, and both are alive until this ends.
        let _ = unsafe { self.devices.UnregisterEndpointNotificationCallback(&self.told) };
    }
}

/// What Windows tells, from threads of its own: one tick per endpoint change.
#[implement(IMMNotificationClient)]
struct Told {
    changed: watch::Sender<u64>,
    wake: Wake,
}

impl Told {
    fn tick(&self) -> Result<()> {
        self.changed.send_modify(|count| *count = count.wrapping_add(1));
        (self.wake)();
        Ok(())
    }
}

impl IMMNotificationClient_Impl for Told_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        self.tick()
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        self.tick()
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        self.tick()
    }

    fn OnDefaultDeviceChanged(&self, _: EDataFlow, _: ERole, _: &PCWSTR) -> Result<()> {
        self.tick()
    }

    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, Served};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, eCapture, eCommunications};
    use windows::core::w;

    #[test]
    fn every_endpoint_coming_going_or_becoming_the_default_ticks_and_wakes_and_a_property_does_not() {
        let (tell, seen) = watch::channel(0u64);
        let woken = Arc::new(AtomicUsize::new(0));
        let wake: Wake = {
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        };
        let told: IMMNotificationClient = Told { changed: tell, wake }.into();
        let id = w!("an endpoint");
        // SAFETY: `told` is our own live object, and the string is a 'static literal.
        unsafe { told.OnDeviceAdded(id) }.unwrap();
        // SAFETY: as above.
        unsafe { told.OnDeviceRemoved(id) }.unwrap();
        // SAFETY: as above.
        unsafe { told.OnDeviceStateChanged(id, DEVICE_STATE_ACTIVE) }.unwrap();
        // SAFETY: as above.
        unsafe { told.OnDefaultDeviceChanged(eCapture, eCommunications, id) }.unwrap();
        // SAFETY: as above.
        unsafe { told.OnPropertyValueChanged(id, PROPERTYKEY::default()) }.unwrap();
        assert_eq!(*seen.borrow(), 4, "four changes ticked, the property did not");
        assert_eq!(woken.load(Ordering::Relaxed), 4, "and woke the owner as often");
    }

    #[test]
    fn the_registration_is_made_with_the_system_and_unregistered_when_dropped() {
        let host = Host::enter(Served::default()).unwrap();
        let (tell, _seen) = watch::channel(0u64);
        let endpoints = Endpoints::listen(host.stage().devices(), tell, Arc::new(|| {})).unwrap();
        drop(endpoints);
    }
}
