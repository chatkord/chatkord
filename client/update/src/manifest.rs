//! `latest.json`: a release's version and each system's package, and its signature, `latest.json.sig`. One type
//! writes it for a release and verifies and reads it for a client, so the two cannot disagree on its form.
//!
//! The JSON is read only after the signature over its exact bytes holds. A field or a system this build does not know
//! is passed over, so a later release may add one; a system listed twice is refused.

use crate::error::UpdateError;
use crate::key::{SIGNATURE_BYTES, SigningKey, VerifyingKey};
use crate::package::{Digest, Package, PackageName, PackageRefused, Platform};
use crate::version::Version;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The most `latest.json` may be: a few hundred bytes a system.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;

/// The manifest's name beside the packages.
pub const MANIFEST_FILE: &str = "latest.json";

/// Its signature's name beside it.
pub const SIGNATURE_FILE: &str = "latest.json.sig";

/// A release: its version and the package it carries for each system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    version: Version,
    packages: BTreeMap<Platform, Package>,
}

/// A manifest as it is published: its exact bytes and the release key's signature over them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    manifest: Vec<u8>,
    signature: [u8; SIGNATURE_BYTES],
}

impl Manifest {
    /// The release `version` carrying `packages`.
    pub fn new(version: Version, packages: BTreeMap<Platform, Package>) -> Manifest {
        Manifest { version, packages }
    }

    /// The release's version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The package for `platform`, when the release carries one.
    pub fn package(&self, platform: Platform) -> Option<&Package> {
        self.packages.get(&platform)
    }

    /// The manifest written and signed by `key`.
    pub fn sign(&self, key: &SigningKey) -> Result<Signed, UpdateError> {
        let manifest = self.text()?;
        let signature = key.sign(&manifest);
        Ok(Signed { manifest, signature })
    }

    /// `latest.json`'s exact bytes, which its signature is over. Within [`MAX_MANIFEST_BYTES`] by its parts' bounds.
    pub fn text(&self) -> Result<Vec<u8>, UpdateError> {
        let packages = self
            .packages
            .iter()
            .map(|(platform, package)| {
                let written = Written {
                    file: package.file().as_str().to_string(),
                    size: package.size(),
                    sha256: package.sha256().to_string(),
                };
                (platform.key(), written)
            })
            .collect();
        let whole = Whole { version: self.version.to_string(), packages };
        let mut manifest = serde_json::to_vec_pretty(&whole).map_err(|_| UpdateError::Malformed)?;
        manifest.push(b'\n');
        Ok(manifest)
    }

    /// The manifest `bytes` are, once `signature` is `key`'s over exactly them; nothing of them is read before.
    pub fn verified(bytes: &[u8], signature: &[u8], key: &VerifyingKey) -> Result<Manifest, UpdateError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::TooLarge);
        }
        if signature.len() != SIGNATURE_BYTES || !key.verifies(bytes, signature) {
            return Err(UpdateError::BadSignature);
        }
        let read: Read = serde_json::from_slice(bytes).map_err(|_| UpdateError::Malformed)?;
        let version = read.version.parse().map_err(|_| UpdateError::Malformed)?;
        Ok(Manifest { version, packages: read.packages.0 })
    }
}

impl Signed {
    /// `latest.json`'s bytes.
    pub fn manifest(&self) -> &[u8] {
        &self.manifest
    }

    /// `latest.json.sig`'s bytes: the raw signature.
    pub fn signature(&self) -> &[u8; SIGNATURE_BYTES] {
        &self.signature
    }
}

/// The manifest as it is written.
#[derive(Serialize)]
struct Whole {
    version: String,
    packages: BTreeMap<&'static str, Written>,
}

/// A package as it is written and read, before its fields are parsed.
#[derive(Serialize, Deserialize)]
struct Written {
    file: String,
    size: u64,
    sha256: String,
}

/// The manifest as it is read, before its version is parsed.
#[derive(Deserialize)]
struct Read {
    version: String,
    packages: Listed,
}

/// The packages of the systems this build knows, each parsed.
struct Listed(BTreeMap<Platform, Package>);

impl<'de> Deserialize<'de> for Listed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Listed, D::Error> {
        deserializer.deserialize_map(ListedVisitor)
    }
}

struct ListedVisitor;

impl<'de> Visitor<'de> for ListedVisitor {
    type Value = Listed;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("each system's package by its key")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Listed, A::Error> {
        let mut packages = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let Some(platform) = Platform::of_key(&key) else {
                map.next_value::<IgnoredAny>()?;
                continue;
            };
            let written: Written = map.next_value()?;
            let package = parsed(platform, &written).map_err(de::Error::custom)?;
            if packages.insert(platform, package).is_some() {
                return Err(de::Error::custom("a system listed twice"));
            }
        }
        Ok(Listed(packages))
    }
}

fn parsed(platform: Platform, written: &Written) -> Result<Package, PackageRefused> {
    Package::new(PackageName::parse(&written.file, platform)?, written.size, Digest::parse(&written.sha256)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{MAX_FILE_NAME, MAX_PACKAGE_BYTES};
    use crate::testing::{package, signing_key};

    fn manifest() -> Manifest {
        let packages = BTreeMap::from([
            (Platform::LinuxX86_64, package(Platform::LinuxX86_64, "Chatkord-x86_64.AppImage", b"linux")),
            (Platform::WindowsX86_64, package(Platform::WindowsX86_64, "Chatkord-setup.exe", b"windows")),
        ]);
        Manifest::new(Version::new(1, 2, 3), packages)
    }

    #[test]
    fn a_signed_manifest_reads_back_as_it_was_written() {
        let key = signing_key();
        let signed = manifest().sign(&key).unwrap();
        let read = Manifest::verified(signed.manifest(), signed.signature(), &key.verifying_key());
        assert_eq!(read, Ok(manifest()));
    }

    #[test]
    fn a_manifest_is_not_read_unless_its_signature_is_the_keys_over_every_byte() {
        let key = signing_key();
        let signed = manifest().sign(&key).unwrap();
        let other = signing_key().verifying_key();
        assert_eq!(Manifest::verified(signed.manifest(), signed.signature(), &other), Err(UpdateError::BadSignature));
        let mut tampered = signed.manifest().to_vec();
        tampered[20] ^= 1;
        let verifying = key.verifying_key();
        assert_eq!(Manifest::verified(&tampered, signed.signature(), &verifying), Err(UpdateError::BadSignature));
        let mut longer = signed.signature().to_vec();
        longer.push(0);
        assert_eq!(Manifest::verified(signed.manifest(), &longer, &verifying), Err(UpdateError::BadSignature));
        assert_eq!(Manifest::verified(signed.manifest(), &[], &verifying), Err(UpdateError::BadSignature));
    }

    /// `text` signed by `key`, read back.
    fn read(text: &str, key: &SigningKey) -> Result<Manifest, UpdateError> {
        let signature = key.sign(text.as_bytes());
        Manifest::verified(text.as_bytes(), &signature, &key.verifying_key())
    }

    #[test]
    fn a_system_or_field_this_build_does_not_know_is_passed_over() {
        let key = signing_key();
        let digest = "ab".repeat(32);
        let text = format!(
            r#"{{"version":"2.0.0","notes":["later"],"packages":{{"linux-aarch64":{{"anything":1}},
            "linux-x86_64":{{"file":"a.AppImage","size":5,"sha256":"{digest}","delta":true}}}}}}"#
        );
        let manifest = read(&text, &key).unwrap();
        assert_eq!(manifest.version(), Version::new(2, 0, 0));
        assert_eq!(manifest.package(Platform::LinuxX86_64).map(Package::size), Some(5));
        assert_eq!(manifest.package(Platform::WindowsX86_64), None);
    }

    #[test]
    fn a_signed_manifest_out_of_shape_is_refused_whole() {
        let key = signing_key();
        let digest = "ab".repeat(32);
        let package =
            |file: &str, size: u64, sha256: &str| format!(r#"{{"file":"{file}","size":{size},"sha256":"{sha256}"}}"#);
        let good = package("a.AppImage", 5, &digest);
        for text in [
            format!(r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{}}}}}"#, package("a/b.AppImage", 5, &digest)),
            format!(r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{}}}}}"#, package("a.exe", 5, &digest)),
            format!(r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{}}}}}"#, package("a.AppImage", 0, &digest)),
            format!(
                r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{}}}}}"#,
                package("a.AppImage", MAX_PACKAGE_BYTES_PLUS_ONE, &digest)
            ),
            format!(r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{}}}}}"#, package("a.AppImage", 5, "AB")),
            format!(r#"{{"version":"2.0.0","packages":{{"linux-x86_64":{good},"linux-x86_64":{good}}}}}"#),
            format!(r#"{{"version":"2.0.0","version":"3.0.0","packages":{{"linux-x86_64":{good}}}}}"#),
            format!(r#"{{"version":"2.0.0-rc.1","packages":{{"linux-x86_64":{good}}}}}"#),
            format!(r#"{{"packages":{{"linux-x86_64":{good}}}}}"#),
            r#"{"version":"2.0.0"}"#.to_string(),
            "not json".to_string(),
        ] {
            assert_eq!(read(&text, &key), Err(UpdateError::Malformed), "{text}");
        }
    }

    const MAX_PACKAGE_BYTES_PLUS_ONE: u64 = MAX_PACKAGE_BYTES + 1;

    #[test]
    fn the_largest_manifest_a_release_can_sign_is_within_the_bound() {
        let largest = Platform::ALL.map(|platform| {
            let file = format!("{}{}", "a".repeat(MAX_FILE_NAME - platform.suffix().len()), platform.suffix());
            let file = PackageName::parse(&file, platform).unwrap();
            (platform, Package::new(file, MAX_PACKAGE_BYTES, Digest::parse(&"ab".repeat(32)).unwrap()).unwrap())
        });
        let manifest = Manifest::new(Version::new(u32::MAX, u32::MAX, u32::MAX), BTreeMap::from(largest));
        let signed = manifest.sign(&signing_key()).unwrap();
        assert!(signed.manifest().len() <= MAX_MANIFEST_BYTES / 4, "{} bytes", signed.manifest().len());
    }

    #[test]
    fn a_manifest_over_its_bound_is_not_read() {
        let key = signing_key();
        let over = vec![b' '; MAX_MANIFEST_BYTES + 1];
        let signature = key.sign(&over);
        assert_eq!(Manifest::verified(&over, &signature, &key.verifying_key()), Err(UpdateError::TooLarge));
    }
}
