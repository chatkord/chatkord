//! Compiles in the release's public key from `release.pub` (32 raw bytes) when the file is there: without it this
//! build has no update channel. A file of any other length stops the build.

use std::error::Error;
use std::path::Path;
use std::{env, fs, io};

/// The key's file, beside this crate's manifest.
const KEY_FILE: &str = "release.pub";

fn main() -> Result<(), Box<dyn Error>> {
    let here = env::var("CARGO_MANIFEST_DIR")?;
    // The directory, not the file: cargo reruns a script whose watched file is missing on every build.
    println!("cargo::rerun-if-changed={here}");
    let key = match fs::read(Path::new(&here).join(KEY_FILE)) {
        Ok(bytes) => {
            let key: [u8; 32] =
                bytes.try_into().map_err(|_| format!("{KEY_FILE} holds an Ed25519 public key as 32 raw bytes"))?;
            format!("Some({key:?})")
        }
        Err(missing) if missing.kind() == io::ErrorKind::NotFound => "None".to_string(),
        Err(unread) => return Err(unread.into()),
    };
    fs::write(Path::new(&env::var("OUT_DIR")?).join("release_key.rs"), key)?;
    Ok(())
}
