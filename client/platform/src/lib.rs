//! The desktop operating system's services, one API whatever the system: what differs between Linux, Windows and
//! later macOS is chosen here, never in a caller. Nothing a person sees is decided here.

pub mod activation;
pub mod alert;
pub mod appearance;
pub mod auto_update;
pub mod awake;
pub mod cache;
pub mod camera;
mod device;
pub mod emoji;
pub mod event_target;
pub mod folds;
pub mod forget;
pub mod idle;
pub mod instance;
pub mod kept;
pub mod keyring;
pub mod language;
pub mod launcher;
pub mod logs;
pub mod look;
pub mod network;
pub mod notify;
pub mod open;
pub mod package;
pub mod partial;
pub mod pick;
pub mod raise;
pub mod save;
pub mod session;
pub mod share;
pub mod startup;
pub mod state;
pub mod system;
pub mod tray;
pub mod voice;
pub mod volume;
pub mod window;
pub mod zone;

pub use device::Platform;

/// A service of this system that a call needed could not be used: no keyring to read a token from, no
/// configuration directory or kept file to remember a sign-in or a choice in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

/// A folder taken away with all it holds; one that is not there is taken already.
fn removed(folder: &std::path::Path) -> Result<(), Unavailable> {
    match std::fs::remove_dir_all(folder) {
        Ok(()) => Ok(()),
        Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Unavailable),
    }
}

/// Runs blocking work (the keyring, a file in the configuration directory) off the tasks that serve sockets; a
/// pool that has gone is [`Unavailable`].
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Unavailable> + Send + 'static,
) -> Result<T, Unavailable> {
    tokio::task::spawn_blocking(work).await.map_err(|_| Unavailable)?
}
