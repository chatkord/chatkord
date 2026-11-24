//! What a release carries for one system: the system's key in the manifest, the file's name, size and digest.

use crate::address::plain_segment;
use data_encoding::HEXLOWER;
use std::fmt;

/// The most a package may be. The AppImage is about a tenth of it; a package past it is refused before a byte is
/// fetched, and a fetch past its stated size is refused at the first byte over.
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The longest a package's file name may be.
pub const MAX_FILE_NAME: usize = 128;

/// A system a release carries a package for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    /// Linux on x86-64: the AppImage.
    LinuxX86_64,
    /// Windows on x86-64: the setup.
    WindowsX86_64,
}

impl Platform {
    /// Every system a release carries a package for.
    pub const ALL: [Platform; 2] = [Platform::LinuxX86_64, Platform::WindowsX86_64];

    /// The key the manifest lists this system's package under.
    pub fn key(self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => "linux-x86_64",
            Platform::WindowsX86_64 => "windows-x86_64",
        }
    }

    /// How this system's package's file name ends.
    pub fn suffix(self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => ".AppImage",
            Platform::WindowsX86_64 => ".exe",
        }
    }

    /// The system listed under `key`; none for a key this build does not know, which a later release may add.
    pub fn of_key(key: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|platform| platform.key() == key)
    }

    /// The system whose package `file` is, by how its name ends.
    pub fn of_file(file: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|platform| file.ends_with(platform.suffix()))
    }

    /// The system this build runs on, when a release carries a package for it.
    pub const fn running() -> Option<Platform> {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Some(Platform::LinuxX86_64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Platform::WindowsX86_64)
        } else {
            None
        }
    }
}

/// A package's file name: ASCII letters, digits, `.`, `_` and `-`, not starting with a dot, ending as its system's
/// package does, at most [`MAX_FILE_NAME`] bytes. So it is one path segment of a URL and of a directory as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageName(String);

impl PackageName {
    /// `name` as `platform`'s package's file name, or refused whole.
    pub fn parse(name: &str, platform: Platform) -> Result<PackageName, PackageRefused> {
        let stem = name.strip_suffix(platform.suffix()).filter(|stem| !stem.is_empty());
        if name.len() > MAX_FILE_NAME || !plain_segment(name) || stem.is_none() {
            return Err(PackageRefused::Name);
        }
        Ok(PackageName(name.to_string()))
    }

    /// The name as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A package's SHA-256, written as 64 lower-case hexadecimal digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest([u8; Digest::LEN]);

impl Digest {
    /// The digest's length in bytes.
    pub const LEN: usize = 32;

    /// The SHA-256 `digest` is; none for any other algorithm's.
    pub fn of(digest: &ring::digest::Digest) -> Option<Digest> {
        if digest.algorithm() != &ring::digest::SHA256 {
            return None;
        }
        digest.as_ref().try_into().ok().map(Digest)
    }

    /// Exactly 64 lower-case hexadecimal digits; anything else is refused whole.
    pub fn parse(text: &str) -> Result<Digest, PackageRefused> {
        if text.len() != 2 * Digest::LEN {
            return Err(PackageRefused::Digest);
        }
        let bytes = HEXLOWER.decode(text.as_bytes()).map_err(|_| PackageRefused::Digest)?;
        bytes.try_into().map(Digest).map_err(|_| PackageRefused::Digest)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&HEXLOWER.encode(&self.0))
    }
}

/// Why a package was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageRefused {
    /// Its file name is not one its system's package may have.
    Name,
    /// It is empty or over [`MAX_PACKAGE_BYTES`].
    Size,
    /// Its digest is not 64 lower-case hexadecimal digits.
    Digest,
}

impl fmt::Display for PackageRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PackageRefused::Name => "a file name that is not the system's package's: letters, digits, '.', '_', '-'",
            PackageRefused::Size => "a package that is empty or over its bound",
            PackageRefused::Digest => "a SHA-256 that is not 64 lower-case hexadecimal digits",
        })
    }
}

impl std::error::Error for PackageRefused {}

/// One system's package in a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    file: PackageName,
    size: u64,
    sha256: Digest,
}

impl Package {
    /// The package `file` of `size` bytes, its SHA-256 `sha256`; a size of none or over [`MAX_PACKAGE_BYTES`] is
    /// refused.
    pub fn new(file: PackageName, size: u64, sha256: Digest) -> Result<Package, PackageRefused> {
        if size == 0 || size > MAX_PACKAGE_BYTES {
            return Err(PackageRefused::Size);
        }
        Ok(Package { file, size, sha256 })
    }

    /// The file's name, as the release lists it.
    pub fn file(&self) -> &PackageName {
        &self.file
    }

    /// The file's exact size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The file's SHA-256.
    pub fn sha256(&self) -> Digest {
        self.sha256
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_name_is_one_plain_segment_ending_as_its_systems_does() {
        let linux = Platform::LinuxX86_64;
        assert!(PackageName::parse("Chatkord-0.2.0-x86_64.AppImage", linux).is_ok());
        assert!(PackageName::parse("Chatkord-setup-0.2.0.exe", Platform::WindowsX86_64).is_ok());
        for name in [
            "a/b.AppImage",
            "a\\b.AppImage",
            "../b.AppImage",
            ".AppImage",
            ".hidden.AppImage",
            "b.exe",
            "b.appimage",
            "b c.AppImage",
            "b%2F.AppImage",
            "b.AppImage?x",
            "",
        ] {
            assert_eq!(PackageName::parse(name, linux), Err(PackageRefused::Name), "{name:?}");
        }
        let long = format!("{}.AppImage", "a".repeat(MAX_FILE_NAME));
        assert_eq!(PackageName::parse(&long, linux), Err(PackageRefused::Name));
    }

    #[test]
    fn a_package_is_bounded_by_its_size() {
        let name = PackageName::parse("a.AppImage", Platform::LinuxX86_64).unwrap();
        let digest = Digest::parse(&"ab".repeat(32)).unwrap();
        assert!(Package::new(name.clone(), MAX_PACKAGE_BYTES, digest).is_ok());
        assert_eq!(Package::new(name.clone(), MAX_PACKAGE_BYTES + 1, digest), Err(PackageRefused::Size));
        assert_eq!(Package::new(name, 0, digest), Err(PackageRefused::Size));
    }

    #[test]
    fn a_digest_is_64_lower_case_digits() {
        let text = "0123456789abcdef".repeat(4);
        assert_eq!(Digest::parse(&text).unwrap().to_string(), text);
        for text in ["AB".repeat(32), "ab".repeat(31), "ab".repeat(33), "zz".repeat(32)] {
            assert_eq!(Digest::parse(&text), Err(PackageRefused::Digest), "{text:?}");
        }
        let sha1 = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, b"");
        assert_eq!(Digest::of(&sha1), None);
    }

    #[test]
    fn a_systems_key_and_suffix_name_it_back() {
        for platform in Platform::ALL {
            assert_eq!(Platform::of_key(platform.key()), Some(platform));
            assert_eq!(Platform::of_file(&format!("a{}", platform.suffix())), Some(platform));
        }
        assert_eq!(Platform::of_key("linux-aarch64"), None);
        assert_eq!(Platform::of_file("a.zip"), None);
    }
}
