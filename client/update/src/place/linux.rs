//! Linux: the AppImage `$APPIMAGE` names, replaced under its own name as an AppImage's updaters do.

use super::Target;
use crate::error::UpdateError;
use std::fs::{File, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempPath;

/// The variable the AppImage runtime names the running image's path in.
const APPIMAGE: &str = "APPIMAGE";

/// The mode a placed AppImage has: run by anyone, written by its owner.
const MODE: u32 = 0o755;

/// The running AppImage's file, links resolved so the file itself is replaced; a run not from one is not packaged.
pub(super) async fn running() -> Result<PathBuf, UpdateError> {
    let named = std::env::var_os(APPIMAGE).filter(|named| !named.is_empty()).ok_or(UpdateError::NotPackaged)?;
    let file = tokio::fs::canonicalize(named).await.map_err(|_| UpdateError::NotPackaged)?;
    let metadata = tokio::fs::metadata(&file).await.map_err(|_| UpdateError::NotPackaged)?;
    if !metadata.is_file() {
        return Err(UpdateError::NotPackaged);
    }
    Ok(file)
}

/// Where a download is staged: beside the AppImage, so its rename over it stays on one filesystem.
pub(super) fn staging(directory: &Path) -> PathBuf {
    directory.to_path_buf()
}

/// `file` made `target`'s package: its mode set, renamed over it, and the rename itself written to disk.
pub(super) fn replace(file: TempPath, target: &Target) -> Result<(), UpdateError> {
    let disk = |failed: std::io::Error| UpdateError::of_disk(&failed);
    std::fs::set_permissions(&file, Permissions::from_mode(MODE)).map_err(disk)?;
    file.persist(&target.file).map_err(|failed| disk(failed.error))?;
    File::open(&target.directory).and_then(|directory| directory.sync_all()).map_err(disk)
}

/// `target`'s AppImage run as a new process, which this one does not wait for: it outlives this run.
pub(super) fn start(target: &Target) -> Result<(), UpdateError> {
    let started = Command::new(&target.file).stdin(Stdio::null()).spawn();
    started.map(drop).map_err(|failed| UpdateError::NotStarted(failed.kind()))
}
