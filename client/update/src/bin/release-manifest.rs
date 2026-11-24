//! `release-manifest`: a release's `latest.json`, unsigned, for CI; never in a package.
//!
//! `release-manifest --version <version> --out <directory> <package>...` classifies each package by its
//! name (`*.AppImage` Linux, `*.exe` Windows) and writes `latest.json` into the directory, reading no key: CI's
//! signing job, which builds nothing, signs its bytes. `release-manifest --public-key <file>` writes the public half
//! of the key in `CHATKORD_RELEASE_KEY` (the base64 of its PKCS#8 DER, as
//! `openssl genpkey -algorithm ed25519 | openssl pkey -outform DER | base64 -w0` writes it), 32 raw bytes, as
//! the channel's public key file in `client/update` holds it.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;
use update::release::{self, ReleaseError};
use update::{SigningKey, SigningKeyRefused, Version, VersionRefused};
use zeroize::Zeroizing;

/// The variable the release key is read from.
const KEY_VARIABLE: &str = "CHATKORD_RELEASE_KEY";

/// What was asked.
enum Asked {
    /// The manifest of release `version` for `packages`, into `out`.
    Manifest { version: Version, out: PathBuf, packages: Vec<PathBuf> },
    /// The key's public half, into `out`.
    PublicKey { out: PathBuf },
}

/// Why nothing, or not all, was written.
enum Failed {
    /// The arguments are not one of the two forms.
    Usage,
    /// The version is not of this channel's form.
    Version(VersionRefused),
    /// The key's variable is unset or not text.
    NoKey,
    /// The key is not one.
    Key(SigningKeyRefused),
    /// The manifest was not made.
    Release(ReleaseError),
    /// The public key was not written.
    Unwritten(PathBuf, std::io::Error),
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failed::Usage => f.write_str(
                "usage: release-manifest --version <version> --out <directory> <package>...\n       \
                 release-manifest --public-key <file>",
            ),
            Failed::Version(refused) => write!(f, "--version: {refused}"),
            Failed::NoKey => write!(f, "{KEY_VARIABLE} is not set"),
            Failed::Key(refused) => write!(f, "{KEY_VARIABLE}: {refused}"),
            Failed::Release(failed) => write!(f, "{failed}"),
            Failed::Unwritten(file, failed) => write!(f, "{}: {failed}", file.display()),
        }
    }
}

#[expect(clippy::print_stderr, reason = "a command's one way to say why it failed")]
fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            eprintln!("release-manifest: {failed}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<OsString>) -> Result<(), Failed> {
    match parse(arguments)? {
        Asked::Manifest { version, out, packages } => {
            release::write(version, &packages, &out).map(drop).map_err(Failed::Release)
        }
        Asked::PublicKey { out } => {
            let text = Zeroizing::new(std::env::var(KEY_VARIABLE).map_err(|_| Failed::NoKey)?);
            let key = SigningKey::from_base64(&text).map_err(Failed::Key)?;
            std::fs::write(&out, key.verifying_key().bytes()).map_err(|failed| Failed::Unwritten(out, failed))
        }
    }
}

fn parse(arguments: Vec<OsString>) -> Result<Asked, Failed> {
    let mut arguments = arguments.into_iter();
    let (mut version, mut out, mut packages) = (None, None, Vec::new());
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--public-key") => {
                let out = arguments.next().ok_or(Failed::Usage)?;
                let rest = arguments.next();
                return match (version, packages.is_empty(), rest) {
                    (None, true, None) => Ok(Asked::PublicKey { out: out.into() }),
                    _ => Err(Failed::Usage),
                };
            }
            Some("--version") if version.is_none() => {
                let text = arguments.next().ok_or(Failed::Usage)?;
                let text = text.to_str().ok_or(Failed::Version(VersionRefused))?;
                version = Some(text.parse().map_err(Failed::Version)?);
            }
            Some("--out") if out.is_none() => out = Some(PathBuf::from(arguments.next().ok_or(Failed::Usage)?)),
            Some(flag) if flag.starts_with("--") => return Err(Failed::Usage),
            _ => packages.push(PathBuf::from(argument)),
        }
    }
    match (version, out) {
        (Some(version), Some(out)) => Ok(Asked::Manifest { version, out, packages }),
        _ => Err(Failed::Usage),
    }
}
