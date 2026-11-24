//! What a person may pick to share and what came of it, on every system; the picker itself is each system's own.

/// What the person may pick from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// A monitor alone.
    Monitor,
    /// A window alone.
    Window,
    /// The picker offers both.
    Either,
}

/// Whether the pointer is drawn into the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    /// Left out.
    Hidden,
    /// Drawn in each frame.
    Embedded,
}

/// What kind of thing was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A monitor.
    Monitor,
    /// One window.
    Window,
    /// A screen the portal made for the share, shown on no monitor.
    Virtual,
}

/// The thing picked, as the picker describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    /// The PipeWire node that carries it.
    #[cfg(target_os = "linux")]
    pub node_id: u32,
    /// What the portal says it is; absent when it does not say.
    pub kind: Option<Kind>,
    /// Its size in pixels at the moment of picking; absent when the portal does not say.
    pub size: Option<(u32, u32)>,
}

/// The app a picked window belongs to, and the processes showing its windows: whose sound the share hears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    /// The app's id: a desktop file's on Linux, the program's file name on Windows.
    pub app_id: String,
    /// The processes showing its windows, by id.
    pub pids: Vec<u32>,
    /// The app's name: what a person reads.
    pub name: String,
}

/// Why nothing was picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// This system has no picker (on Linux, no screen-cast portal is installed).
    Unavailable,
    /// The person closed the picker.
    Cancelled,
    /// The picker failed, or picked nothing.
    Failed,
}
