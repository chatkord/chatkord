//! A system whose camera capture is not built: every camera is unavailable.

use super::{CameraError, Device, Mode, Shot};
use std::time::Instant;

/// No camera is offered.
pub fn list() -> Result<Vec<Device>, CameraError> {
    Err(CameraError::Unavailable)
}

/// The cameras coming and going, never told here.
#[derive(Debug)]
pub enum Watch {}

/// Refused: this system's cameras are not watched.
pub fn watch(_changed: impl Fn() + Send + Sync + 'static) -> Result<Watch, CameraError> {
    Err(CameraError::Unavailable)
}

/// What ends a camera, which never opens here.
#[derive(Clone, Debug)]
pub enum Stopper {}

impl Stopper {
    /// Never called: no camera opens.
    pub fn stop(&self) {
        match *self {}
    }
}

/// No camera's modes are read here.
pub fn modes(_id: &str) -> Result<Vec<Mode>, CameraError> {
    Err(CameraError::Unavailable)
}

/// A camera that never opens here.
#[derive(Debug)]
pub enum Camera {}

impl Camera {
    /// Refused: this system has no camera capture.
    pub fn open(_id: &str, _asked: Mode) -> Result<Self, CameraError> {
        Err(CameraError::Unavailable)
    }

    /// The mode it captures at.
    pub fn mode(&self) -> Mode {
        match *self {}
    }

    /// How many pictures the camera has handed over since it opened.
    pub fn delivered(&self) -> u64 {
        match *self {}
    }

    /// What ends it from another thread.
    pub fn stopper(&self) -> Stopper {
        match *self {}
    }

    /// The next picture.
    pub fn read(&mut self) -> Result<(Shot, Instant), CameraError> {
        match *self {}
    }

    /// Gives a picture's memory back for the next.
    pub fn recycle(&self, _shot: Shot) {
        match *self {}
    }
}
