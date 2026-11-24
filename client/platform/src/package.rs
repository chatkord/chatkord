//! What the installed package carries beside the program: its licences folder (scripts/package/notices.sh fills it),
//! and in it the notices About Chatkord shows. A build run from its own tree carries none.

use std::fmt;
use std::io::{self, Read};
use std::path::PathBuf;

use notices::{MOST_BYTES, Notices, Refused};

const NOTICES: &str = "notices.json";

/// Why the package's notices could not be had.
#[derive(Debug, report::Public)]
pub enum NotRead {
    /// The file could not be read.
    Io(io::Error),
    /// It is not what scripts/package/notices.sh writes.
    Refused(Refused),
    /// The blocking pool has gone.
    Unavailable,
}

impl fmt::Display for NotRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotRead::Io(why) => write!(f, "the package's notices could not be read: {why}"),
            NotRead::Refused(why) => why.fmt(f),
            NotRead::Unavailable => write!(f, "the package's notices could not be read: no blocking pool"),
        }
    }
}

impl std::error::Error for NotRead {}

/// The package's licences folder: inside the AppImage's mount on Linux, beside the program on Windows; none in a
/// build run from its tree.
pub fn licences() -> Option<PathBuf> {
    folder().filter(|folder| folder.is_dir())
}

#[cfg(target_os = "linux")]
fn folder() -> Option<PathBuf> {
    let mount = PathBuf::from(std::env::var_os("APPDIR")?);
    mount.is_absolute().then(|| mount.join("usr").join("share").join("licenses").join("chatkord"))
}

#[cfg(target_os = "windows")]
fn folder() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("licenses"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn folder() -> Option<PathBuf> {
    None
}

/// The package's notices, read on the blocking pool; none without a package.
pub async fn notices() -> Option<Result<Notices, NotRead>> {
    let file = licences()?.join(NOTICES);
    let read = tokio::task::spawn_blocking(move || read(&file)).await;
    Some(read.unwrap_or(Err(NotRead::Unavailable)))
}

/// The notices `file` holds, read no further than one byte past their bound, so a larger file is refused unread.
fn read(file: &std::path::Path) -> Result<Notices, NotRead> {
    let mut bytes = Vec::new();
    let most = u64::try_from(MOST_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    std::fs::File::open(file).and_then(|opened| opened.take(most).read_to_end(&mut bytes)).map_err(NotRead::Io)?;
    Notices::parse(&bytes).map_err(NotRead::Refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-notices-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a scratch folder");
        folder.join(NOTICES)
    }

    #[test]
    fn a_written_file_is_read_and_one_past_the_bound_is_refused_unread() {
        let file = scratch("read");
        let written =
            r#"{"groups": [{"name": "MIT", "what": "1 Rust package: serde", "licence": "MIT", "text": "t"}]}"#;
        std::fs::write(&file, written).expect("written");
        assert!(matches!(read(&file), Ok(notices) if notices.groups().len() == 1));
        std::fs::write(&file, vec![b' '; MOST_BYTES * 2]).expect("written");
        assert!(matches!(read(&file), Err(NotRead::Refused(Refused::TooLarge))));
        std::fs::remove_file(&file).expect("removed");
        assert!(matches!(read(&file), Err(NotRead::Io(_))));
    }
}
