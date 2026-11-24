//! Everything this program keeps on a device, taken away: each remembered account's token, the cache, the runs' logs,
//! the launcher's entry an AppImage made, the entry that opens it at sign-in, then the folder of what a run leaves for
//! the next. For an uninstall that asks for it.

use crate::instance::Instance;
use crate::{Platform, Unavailable, cache, keyring, launcher, logs, startup, state};

/// Takes everything away, blocking: never called from an async task. `held` proves no window runs, and is let go
/// just before the folder its lock may be in goes. A token the keyring will not remove stops it before anything else
/// goes, so it can be asked again.
pub fn everything(platform: &Platform, held: Instance) -> Result<(), Unavailable> {
    for (server, identity) in state::accounts(platform)? {
        keyring::forget(&server, &identity)?;
    }
    cache::remove(platform)?;
    logs::remove(platform)?;
    launcher::remove()?;
    startup::forget(platform)?;
    drop(held);
    state::remove(platform)
}
