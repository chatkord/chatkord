//! A release's manifest made from its packages, unsigned: what `release-manifest` does for CI, whose signing job
//! signs its exact bytes with the key no build ever sees.

use crate::manifest::{MANIFEST_FILE, Manifest};
use crate::package::{Digest, MAX_PACKAGE_BYTES, Package, PackageName, PackageRefused, Platform};
use crate::version::Version;
use ring::digest::{Context, SHA256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Why a release's manifest was not made.
#[derive(Debug)]
pub enum ReleaseError {
    /// No package was named.
    NoPackages,
    /// This file's name is no system's package's: `*.AppImage` for Linux, `*.exe` for Windows.
    Unclassified(PathBuf),
    /// Two packages were named for this system.
    Twice(Platform),
    /// This package was refused, for the reason given.
    Refused(PathBuf, PackageRefused),
    /// This package could not be read.
    Unread(PathBuf, io::Error),
    /// The manifest could not be written into this directory.
    Unwritten(PathBuf, io::Error),
    /// The manifest could not be written as JSON.
    Json(crate::UpdateError),
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseError::NoPackages => f.write_str("no package was named"),
            ReleaseError::Unclassified(file) => {
                write!(f, "{}: not a system's package (*.AppImage, *.exe)", file.display())
            }
            ReleaseError::Twice(platform) => write!(f, "two packages for {}", platform.key()),
            ReleaseError::Refused(file, refused) => write!(f, "{}: {refused}", file.display()),
            ReleaseError::Unread(file, failed) => write!(f, "{}: {failed}", file.display()),
            ReleaseError::Unwritten(directory, failed) => write!(f, "{}: {failed}", directory.display()),
            ReleaseError::Json(failed) => write!(f, "{failed}"),
        }
    }
}

impl std::error::Error for ReleaseError {}

/// The manifest of release `version` carrying `packages`, each classified by its name's ending, written into `out`
/// as `latest.json`; the bytes written.
pub fn write(version: Version, packages: &[PathBuf], out: &Path) -> Result<Vec<u8>, ReleaseError> {
    if packages.is_empty() {
        return Err(ReleaseError::NoPackages);
    }
    let mut listed = BTreeMap::new();
    for file in packages {
        let (platform, package) = read(file)?;
        if listed.insert(platform, package).is_some() {
            return Err(ReleaseError::Twice(platform));
        }
    }
    let manifest = Manifest::new(version, listed).text().map_err(ReleaseError::Json)?;
    std::fs::write(out.join(MANIFEST_FILE), &manifest)
        .map_err(|failed| ReleaseError::Unwritten(out.to_path_buf(), failed))?;
    Ok(manifest)
}

/// The package `file` is: its system by its name, its size and its SHA-256; past [`MAX_PACKAGE_BYTES`] it is refused
/// at the first byte over.
fn read(file: &Path) -> Result<(Platform, Package), ReleaseError> {
    let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let platform = Platform::of_file(name).ok_or_else(|| ReleaseError::Unclassified(file.to_path_buf()))?;
    let refused = |refused| ReleaseError::Refused(file.to_path_buf(), refused);
    let name = PackageName::parse(name, platform).map_err(refused)?;
    let unread = |failed| ReleaseError::Unread(file.to_path_buf(), failed);
    let opened = std::fs::File::open(file).map_err(unread)?;
    let mut hashing = Hashing(Context::new(&SHA256));
    let size = io::copy(&mut opened.take(MAX_PACKAGE_BYTES + 1), &mut hashing).map_err(unread)?;
    let sha256 = Digest::of(&hashing.0.finish()).ok_or_else(|| refused(PackageRefused::Digest))?;
    Ok((platform, Package::new(name, size, sha256).map_err(refused)?))
}

/// A writer that only hashes what it is given.
struct Hashing(Context);

impl Write for Hashing {
    fn write(&mut self, part: &[u8]) -> io::Result<usize> {
        self.0.update(part);
        Ok(part.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::signing_key;

    #[test]
    fn a_release_is_written_and_reads_back_once_its_bytes_are_signed() {
        let out = tempfile::tempdir().unwrap();
        let linux = out.path().join("Chatkord-0.2.0-x86_64.AppImage");
        let windows = out.path().join("Chatkord-setup-0.2.0.exe");
        std::fs::write(&linux, b"an appimage").unwrap();
        std::fs::write(&windows, b"a setup").unwrap();
        let written = write(Version::new(0, 2, 0), &[linux, windows], out.path()).unwrap();
        let manifest = std::fs::read(out.path().join(MANIFEST_FILE)).unwrap();
        assert_eq!(manifest, written);
        let key = signing_key();
        let read = Manifest::verified(&manifest, &key.sign(&manifest), &key.verifying_key()).unwrap();
        assert_eq!(read.version(), Version::new(0, 2, 0));
        let package = read.package(Platform::LinuxX86_64).unwrap();
        assert_eq!(package.file().as_str(), "Chatkord-0.2.0-x86_64.AppImage");
        assert_eq!(package.size(), 11);
        let expected = ring::digest::digest(&SHA256, b"an appimage");
        assert_eq!(Some(package.sha256()), Digest::of(&expected));
        assert_eq!(read.package(Platform::WindowsX86_64).map(Package::size), Some(7));
    }

    #[test]
    fn a_file_of_no_system_two_for_one_or_none_writes_nothing() {
        let out = tempfile::tempdir().unwrap();
        let named = |name: &str| {
            let file = out.path().join(name);
            std::fs::write(&file, b"bytes").unwrap();
            file
        };
        let version = Version::new(0, 2, 0);
        let zip = named("Chatkord.zip");
        assert!(matches!(write(version, &[zip], out.path()), Err(ReleaseError::Unclassified(_))));
        let twice = [named("a.AppImage"), named("b.AppImage")];
        assert!(matches!(write(version, &twice, out.path()), Err(ReleaseError::Twice(Platform::LinuxX86_64))));
        let spaced = named("Chatkord Setup.exe");
        assert!(matches!(write(version, &[spaced], out.path()), Err(ReleaseError::Refused(_, PackageRefused::Name))));
        let empty = out.path().join("empty.AppImage");
        std::fs::write(&empty, b"").unwrap();
        assert!(matches!(write(version, &[empty], out.path()), Err(ReleaseError::Refused(_, PackageRefused::Size))));
        assert!(matches!(write(version, &[], out.path()), Err(ReleaseError::NoPackages)));
        assert!(!out.path().join(MANIFEST_FILE).exists());
    }
}
