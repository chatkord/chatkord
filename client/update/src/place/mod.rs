//! The running package and what replaces it. On Linux the fetched AppImage is made the package by one rename in its
//! directory, so the package is the old one or the new one whole, never a part; on Windows the fetched setup is kept
//! in this user's temporary directory, and installs over the package once this run has ended. Each system's half is
//! behind this one API.

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod system;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[path = "other.rs"]
mod system;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod system;

#[cfg(all(test, target_os = "windows"))]
pub(crate) use system::SETUP;

use crate::error::UpdateError;
use crate::fetch::Fetched;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// The package this run is from, which an update replaces: its file's path, its directory, and the directory this
/// system's half stages a download in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    file: PathBuf,
    directory: PathBuf,
    staging: PathBuf,
}

impl Target {
    /// The package this run is from: on Linux the AppImage `$APPIMAGE` names, on Windows the executable the setup
    /// installed. Not offered on a system whose half is not written, nor for a run not from a package.
    pub async fn of_this_run() -> Result<Target, UpdateError> {
        let file = system::running().await?;
        Target::at(file)
    }

    /// The package at `file`, an absolute path with a directory.
    pub(crate) fn at(file: PathBuf) -> Result<Target, UpdateError> {
        let directory = file.parent().filter(|_| file.is_absolute()).map(Path::to_path_buf);
        let directory = directory.ok_or(UpdateError::NotPackaged)?;
        Ok(Target { staging: system::staging(&directory), directory, file })
    }

    /// This target with its downloads staged in `staging` instead of where this system stages them.
    #[cfg(all(test, target_os = "windows"))]
    pub(crate) fn staged_in(self, staging: PathBuf) -> Target {
        Target { staging, ..self }
    }

    /// The package's path.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The directory a download is staged in.
    #[cfg(test)]
    pub(crate) fn staging(&self) -> &Path {
        &self.staging
    }

    /// Starts the new run from what [`place`] put in place, with this run's environment and no input, and does not
    /// wait for it: on Linux the AppImage itself; on Windows its setup, which installs it unseen and then opens it.
    /// Called once this run has let its window go.
    pub fn start(&self) -> Result<(), UpdateError> {
        system::start(self)
    }

    /// A new, empty file in the directory a download is staged in (on Linux beside the package), removed when it is
    /// dropped.
    pub(crate) async fn staged(&self) -> Result<NamedTempFile, UpdateError> {
        let staging = self.staging.clone();
        let made = tokio::task::spawn_blocking(move || {
            tempfile::Builder::new().prefix(".chatkord-update-").suffix(".part").tempfile_in(staging)
        });
        made.await.map_err(|_| UpdateError::Stopped)?.map_err(|failed| UpdateError::of_disk(&failed))
    }
}

/// `fetched` put in place and written to disk: on Linux made executable and renamed over the package it replaces; on
/// Windows renamed to the setup [`Target::start`] runs.
pub async fn place(fetched: Fetched) -> Result<(), UpdateError> {
    let placed = tokio::task::spawn_blocking(move || system::replace(fetched.file, &fetched.target));
    placed.await.map_err(|_| UpdateError::Stopped)?
}
