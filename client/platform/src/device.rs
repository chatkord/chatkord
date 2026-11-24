//! The folders this run keeps things in, named once at start and handed to whoever needs them: one value, so no
//! service reads a global and a test names its own scratch folders.

use std::path::{Path, PathBuf};

/// This device's folders for a run: where the choices and servers are kept, the pictures fetched, and the logs. A
/// folder not named keeps nothing there. Cheap to clone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Platform {
    config: Option<PathBuf>,
    cache: Option<PathBuf>,
    logs: Option<PathBuf>,
}

impl Platform {
    /// The system's own folders for this user.
    pub fn system() -> Self {
        Self { config: crate::state::directory(), cache: crate::cache::directory(), logs: crate::logs::directory() }
    }

    /// This value with the configuration folder `folder`: a test's scratch folder, a portable build's own.
    #[must_use]
    pub fn config_in(mut self, folder: PathBuf) -> Self {
        self.config = Some(folder);
        self
    }

    /// This value with the cache folder `folder`.
    #[must_use]
    pub fn cache_in(mut self, folder: PathBuf) -> Self {
        self.cache = Some(folder);
        self
    }

    /// This value with the folder `folder` for the logs.
    #[must_use]
    pub fn logs_in(mut self, folder: PathBuf) -> Self {
        self.logs = Some(folder);
        self
    }

    /// The configuration directory named for this run, where the desktop's own folders (`autostart`) are too.
    pub(crate) fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub(crate) fn cache(&self) -> Option<&Path> {
        self.cache.as_deref()
    }

    pub(crate) fn logs(&self) -> Option<&Path> {
        self.logs.as_deref()
    }

    /// This program's folder inside the configuration directory: where `instance` keeps its lock when the system has
    /// no runtime folder, and what the program learns about this machine is kept.
    pub fn folder(&self) -> Option<PathBuf> {
        self.config().map(|one| one.join(crate::state::FOLDER))
    }
}
