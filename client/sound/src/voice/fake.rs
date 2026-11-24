//! Devices a unit test makes and sets, in place of the system's sound server: each by name, `default` the one opened
//! with none, refused until the test gives it a health.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::{Capture, Health, Playback, VoiceError};

/// What a fake speaker is asked to fill each quantum with.
pub(super) type Fill = Box<dyn FnMut(&mut [i16]) + Send>;

/// What a fake device does and was asked.
#[derive(Default)]
pub(super) struct Device {
    pub(super) health: Option<Health>,
    pub(super) opened: usize,
    pub(super) rates: Vec<f32>,
    pub(super) fill: Option<Fill>,
}

thread_local! {
    static DEVICES: RefCell<Vec<(String, Arc<Mutex<Device>>)>> = const { RefCell::new(Vec::new()) };
}

/// The device `name`, made on first ask.
pub(super) fn device(name: &str) -> Arc<Mutex<Device>> {
    DEVICES.with_borrow_mut(|devices| {
        if let Some((_, device)) = devices.iter().find(|(named, _)| named == name) {
            return Arc::clone(device);
        }
        let device = Arc::new(Mutex::new(Device::default()));
        devices.push((name.to_owned(), Arc::clone(&device)));
        device
    })
}

/// A device, held.
pub(super) fn held(device: &Mutex<Device>) -> MutexGuard<'_, Device> {
    device.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Opens the device `name` (none: `default`), refused until the test gave it a health.
fn opened(name: Option<&str>) -> Result<Arc<Mutex<Device>>, VoiceError> {
    let device = device(name.unwrap_or("default"));
    let mut device_held = held(&device);
    device_held.health.ok_or(VoiceError::Stream)?;
    device_held.opened += 1;
    drop(device_held);
    Ok(device)
}

fn health(device: &Mutex<Device>) -> Health {
    held(device).health.unwrap_or(Health::Stopped)
}

/// A fake microphone's stream.
pub(super) struct Input(Arc<Mutex<Device>>);

impl Capture for Input {
    fn open(device: Option<&str>, _hear: impl FnMut(&[i16]) + Send + 'static) -> Result<Self, VoiceError> {
        opened(device).map(Self)
    }

    fn health(&self) -> Health {
        health(&self.0)
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(5)
    }
}

/// A fake speaker's stream.
pub(super) struct Output(Arc<Mutex<Device>>);

impl Playback for Output {
    fn open(device: Option<&str>, fill: impl FnMut(&mut [i16]) + Send + 'static) -> Result<Self, VoiceError> {
        let device = opened(device)?;
        held(&device).fill = Some(Box::new(fill));
        Ok(Self(device))
    }

    fn health(&self) -> Health {
        health(&self.0)
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(5)
    }

    fn set_rate(&self, ratio: f32) {
        held(&self.0).rates.push(ratio);
    }
}
