//! The camera as a person chose it: which one, the picture and the rate asked of it, whether it is shown mirrored, and
//! what that asks of the camera when it opens.

use crate::Picked;

/// The camera a person turns on, as they chose it in the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraAsk {
    /// The system's id of the camera; none for the first the system lists, until one is chosen.
    pub device: Option<String>,
    /// The picture asked for: the camera's own mode nearest it is taken, never a larger one.
    pub width: u32,
    /// As above.
    pub height: u32,
    /// Pictures a second asked for.
    pub frames_per_second: u32,
}

impl Default for CameraAsk {
    /// The first camera at 720p30, as every peer starts one.
    fn default() -> Self {
        CameraSettings::default().ask()
    }
}

/// The picture a camera is asked for, 16:9; the camera's own mode nearest it, never a larger one, is taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum CameraResolution {
    /// 1920 by 1080.
    P1080,
    /// 1280 by 720.
    #[default]
    P720,
    /// 854 by 480.
    P480,
}

impl CameraResolution {
    /// Every choice, in the order the row lists them.
    pub const ALL: [CameraResolution; 3] = [CameraResolution::P1080, CameraResolution::P720, CameraResolution::P480];

    /// Pixels across and down, each even as NV12 needs.
    pub fn size(self) -> (u32, u32) {
        match self {
            CameraResolution::P1080 => (1920, 1080),
            CameraResolution::P720 => (1280, 720),
            CameraResolution::P480 => (854, 480),
        }
    }
}

/// The pictures a second a camera is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, report::Public)]
pub enum CameraRate {
    /// 60 a second, offered where the camera captures it.
    Sixty,
    /// 30 a second.
    #[default]
    Thirty,
    /// 15 a second.
    Fifteen,
}

impl CameraRate {
    /// Every choice, in the order the row lists them.
    pub const ALL: [CameraRate; 3] = [CameraRate::Sixty, CameraRate::Thirty, CameraRate::Fifteen];

    /// Pictures a second.
    pub fn per_second(self) -> u32 {
        match self {
            CameraRate::Sixty => 60,
            CameraRate::Thirty => 30,
            CameraRate::Fifteen => 15,
        }
    }
}

/// Whether one's own picture is shown mirrored, on one's own card and in the preview; what is sent never is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, report::Public)]
pub enum Mirror {
    /// Shown as in a mirror.
    #[default]
    On,
    /// Shown as others see it.
    Off,
}

impl Mirror {
    /// The other one, as the switch flips it.
    pub fn toggled(self) -> Mirror {
        match self {
            Mirror::On => Mirror::Off,
            Mirror::Off => Mirror::On,
        }
    }
}

/// The camera as this device keeps it chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CameraSettings {
    /// The camera picked; none for the first the system lists, until one is.
    pub device: Option<Picked>,
    /// The picture asked for.
    pub resolution: CameraResolution,
    /// The pictures a second asked for.
    pub rate: CameraRate,
    /// Whether one's own picture is shown mirrored.
    pub mirror: Mirror,
}

impl CameraSettings {
    /// What the call or the preview opens the camera with.
    pub fn ask(&self) -> CameraAsk {
        let (width, height) = self.resolution.size();
        CameraAsk {
            device: self.device.as_ref().map(|picked| picked.id.clone()),
            width,
            height,
            frames_per_second: self.rate.per_second(),
        }
    }
}
