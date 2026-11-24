//! Whether a newer release is out, and its package fetched where the system stages it, checked as it arrives.

use crate::channel::Channel;
use crate::error::UpdateError;
use crate::http::get;
use crate::key::SIGNATURE_BYTES;
use crate::manifest::{MANIFEST_FILE, MAX_MANIFEST_BYTES, Manifest, SIGNATURE_FILE};
use crate::package::{Digest, Package, Platform};
use crate::place::Target;
use crate::version::Version;
use ring::digest::{Context, SHA256};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;

/// A newer release, with this system's package in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    version: Version,
    package: Package,
}

impl Offer {
    /// The newer release's version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Its package for this system.
    pub fn package(&self) -> &Package {
        &self.package
    }
}

/// How much of a package has arrived. Reported for every part, so a window that shows it keeps only the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes arrived and checked, never more than `total`.
    pub received: u64,
    /// The package's size, as its release states it; never zero.
    pub total: u64,
}

impl Progress {
    /// The share arrived, 0 to 100.
    pub fn percent(self) -> u8 {
        let share = u128::from(self.received.min(self.total)) * 100 / u128::from(self.total.max(1));
        u8::try_from(share).unwrap_or(100)
    }
}

/// A package fetched whole and checked, in a file where the system stages it; the file is removed if this is
/// dropped before it is put in place.
#[derive(Debug)]
pub struct Fetched {
    pub(crate) file: TempPath,
    pub(crate) target: Target,
    version: Version,
}

impl Fetched {
    /// The release it is.
    pub fn version(&self) -> Version {
        self.version
    }
}

/// The newer release `channel`'s signed manifest offers this system, when it is newer than `running`; none when it
/// is not, carries no package for this system, or the latest release has no manifest yet (404). A manifest read
/// while its release is being uploaded may not match its signature: that is a failure, and the next check reads it
/// whole.
pub async fn check(channel: &Channel, running: Version) -> Result<Option<Offer>, UpdateError> {
    check_for(channel, running, Platform::running().ok_or(UpdateError::NotOffered)?).await
}

pub(crate) async fn check_for(
    channel: &Channel,
    running: Version,
    platform: Platform,
) -> Result<Option<Offer>, UpdateError> {
    let read = |name| async move {
        let answer = get(&channel.trust, &channel.base.join(name)?, channel.patience).await?;
        Ok::<_, UpdateError>(answer)
    };
    let manifest = match read(MANIFEST_FILE).await {
        // A release is published before its manifest is attached, and a build with no release key publishes none.
        Err(UpdateError::Status(404)) => return Ok(None),
        answered => answered?.whole(MAX_MANIFEST_BYTES).await?,
    };
    let signature = read(SIGNATURE_FILE).await?.whole(SIGNATURE_BYTES).await?;
    let manifest = Manifest::verified(&manifest, &signature, &channel.key)?;
    if manifest.version() <= running {
        return Ok(None);
    }
    let package = manifest.package(platform).cloned();
    Ok(package.map(|package| Offer { version: manifest.version(), package }))
}

/// `offer`'s package, fetched from `channel` into a file staged for `into`, each part reported to `progress`. Refused past
/// its stated size at the first byte over, short of it, or of another SHA-256; the file is gone then.
pub async fn fetch(
    channel: &Channel,
    offer: &Offer,
    into: &Target,
    mut progress: impl FnMut(Progress),
) -> Result<Fetched, UpdateError> {
    let total = offer.package.size();
    let mut answer = get(&channel.trust, &channel.base.join(offer.package.file().as_str())?, channel.patience).await?;
    match answer.stated_length() {
        Some(stated) if stated > total => return Err(UpdateError::TooLarge),
        Some(stated) if stated < total => return Err(UpdateError::Short),
        Some(_) | None => {}
    }
    let (file, path) = into.staged().await?.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut digest = Context::new(&SHA256);
    let mut received: u64 = 0;
    while let Some(part) = answer.next().await? {
        received = u64::try_from(part.len()).ok().and_then(|len| received.checked_add(len)).unwrap_or(u64::MAX);
        if received > total {
            return Err(UpdateError::TooLarge);
        }
        digest.update(&part);
        file.write_all(&part).await.map_err(|failed| UpdateError::of_disk(&failed))?;
        progress(Progress { received, total });
    }
    if received < total {
        return Err(UpdateError::Short);
    }
    if Digest::of(&digest.finish()) != Some(offer.package.sha256()) {
        return Err(UpdateError::WrongHash);
    }
    file.flush().await.map_err(|failed| UpdateError::of_disk(&failed))?;
    file.sync_all().await.map_err(|failed| UpdateError::of_disk(&failed))?;
    drop(file);
    Ok(Fetched { file: path, target: into.clone(), version: offer.version })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_a_share_of_a_hundred() {
        let at = |received, total| Progress { received, total }.percent();
        assert_eq!((at(0, 10), at(5, 10), at(10, 10), at(1, 3)), (0, 50, 100, 33));
        assert_eq!(at(u64::MAX, u64::MAX), 100);
    }
}
