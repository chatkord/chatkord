//! Windows: the executable the setup installed, updated by the release's setup run over it unseen once this run has
//! ended, as Inno Setup's updaters do; the setup opens the new run when it is done (`scripts/package/windows.iss`).
//! The update writes nothing into the install: the setup is fetched and kept in this user's temporary directory.

use super::Target;
use crate::error::UpdateError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempPath;

/// The uninstaller Inno Setup writes beside what it installed: a run without one is not from the setup's install.
const UNINSTALLER: &str = "unins000.exe";

/// The fetched setup's name where it is kept, replaced by the next update's.
pub(crate) const SETUP: &str = "chatkord-setup.exe";

/// The setup's own switch for an update: it opens the new run once it is installed, which a person's silent install
/// does not.
const UPDATE_SWITCH: &str = "/update";

/// How the setup is run: no window, no question, no restart of Windows, and as an update.
const SETUP_ARGUMENTS: [&str; 4] = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", UPDATE_SWITCH];

/// This run's executable, when the setup installed it.
pub(super) async fn running() -> Result<PathBuf, UpdateError> {
    installed(std::env::current_exe().map_err(|_| UpdateError::NotPackaged)?).await
}

/// `file`, when the setup's uninstaller is beside it.
async fn installed(file: PathBuf) -> Result<PathBuf, UpdateError> {
    match tokio::fs::metadata(file.with_file_name(UNINSTALLER)).await {
        Ok(metadata) if metadata.is_file() => Ok(file),
        Ok(_) | Err(_) => Err(UpdateError::NotPackaged),
    }
}

/// Where a download is staged and the setup kept: this user's temporary directory, whatever the install's.
pub(super) fn staging(_: &Path) -> PathBuf {
    std::env::temp_dir()
}

/// Where `target`'s fetched setup is kept.
fn setup(target: &Target) -> PathBuf {
    target.staging.join(SETUP)
}

/// `file` made the setup [`start`] runs, over the one an earlier update kept. The install is untouched.
pub(super) fn replace(file: TempPath, target: &Target) -> Result<(), UpdateError> {
    // No directory sync: std opens no directory as a file on Windows; the setup was synced whole before its rename.
    file.persist(setup(target)).map_err(|failed| UpdateError::of_disk(&failed.error))
}

/// The setup at `setup` run as [`SETUP_ARGUMENTS`] say, with no input.
fn command(setup: &Path) -> Command {
    let mut command = Command::new(setup);
    command.args(SETUP_ARGUMENTS).stdin(Stdio::null());
    command
}

/// `target`'s kept setup run as a new process, which this one does not wait for: it outlives this run, closing nothing
/// once this run has let its window go.
pub(super) fn start(target: &Target) -> Result<(), UpdateError> {
    let started = command(&setup(target)).spawn();
    started.map(drop).map_err(|failed| UpdateError::NotStarted(failed.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[tokio::test]
    async fn a_run_is_from_the_install_only_with_the_setups_uninstaller_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("Chatkord.exe");
        std::fs::write(&file, b"the installed window").unwrap();
        assert_eq!(installed(file.clone()).await, Err(UpdateError::NotPackaged));
        std::fs::create_dir(directory.path().join(UNINSTALLER)).unwrap();
        assert_eq!(installed(file.clone()).await, Err(UpdateError::NotPackaged), "a directory is no uninstaller");
        std::fs::remove_dir(directory.path().join(UNINSTALLER)).unwrap();
        std::fs::write(directory.path().join(UNINSTALLER), b"the uninstaller").unwrap();
        assert_eq!(installed(file.clone()).await, Ok(file));
    }

    #[test]
    fn a_download_is_staged_in_this_users_temporary_directory_not_the_install() {
        let target = Target::at(PathBuf::from(r"C:\Programs\Chatkord\Chatkord.exe")).unwrap();
        assert_eq!(setup(&target), std::env::temp_dir().join("chatkord-setup.exe"));
    }

    #[test]
    fn the_kept_setup_is_run_unseen_as_an_update() {
        let command = command(Path::new(r"C:\Temp\chatkord-setup.exe"));
        assert_eq!(command.get_program(), OsStr::new(r"C:\Temp\chatkord-setup.exe"));
        let arguments: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(arguments, ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/update"]);
    }

    #[tokio::test]
    async fn a_setup_put_in_place_replaces_the_one_kept_and_leaves_the_install() {
        let (install, staging) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let target = Target::at(install.path().join("Chatkord.exe")).unwrap().staged_in(staging.path().to_path_buf());
        std::fs::write(target.file(), b"the installed window").unwrap();
        std::fs::write(setup(&target), b"an earlier update's setup").unwrap();
        let fetched = target.staged().await.unwrap();
        assert_eq!(fetched.path().parent(), Some(staging.path()));
        std::fs::write(fetched.path(), b"this update's setup").unwrap();
        replace(fetched.into_temp_path(), &target).unwrap();
        assert_eq!(std::fs::read(setup(&target)).unwrap(), b"this update's setup");
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(target.file()).unwrap(), b"the installed window");
        assert_eq!(std::fs::read_dir(install.path()).unwrap().count(), 1);
    }
}
