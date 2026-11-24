//! What a call sends: a share or a camera from capture to the engine, its health and its sound, and the sending
//! lanes the room runs them in.

pub mod broadcast;
pub mod camera;
pub(crate) mod log;
pub(crate) mod own;
pub(crate) mod own_camera;
pub mod screen;
pub(crate) mod sender;
pub(crate) mod sent_look;
#[cfg(any(target_os = "linux", windows))]
pub(crate) mod share_sound;
pub mod source;
pub(crate) mod stage_log;
#[cfg(any(target_os = "linux", windows))]
pub(crate) mod stats;
pub(crate) mod worker;
