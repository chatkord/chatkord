//! A system whose half is not written: nothing is replaced, and that is said, not assumed.

use super::Target;
use crate::error::UpdateError;
use std::path::{Path, PathBuf};
use tempfile::TempPath;

/// No package here is one an update replaces yet.
pub(super) async fn running() -> Result<PathBuf, UpdateError> {
    Err(UpdateError::NotOffered)
}

/// Where a download would be staged: beside the package, though none is fetched for it.
pub(super) fn staging(directory: &Path) -> PathBuf {
    directory.to_path_buf()
}

/// Nothing is placed; `file` is removed as it is dropped.
pub(super) fn replace(file: TempPath, _: &Target) -> Result<(), UpdateError> {
    drop(file);
    Err(UpdateError::NotOffered)
}

/// Nothing is placed here, so nothing is started.
pub(super) fn start(_: &Target) -> Result<(), UpdateError> {
    Err(UpdateError::NotOffered)
}
