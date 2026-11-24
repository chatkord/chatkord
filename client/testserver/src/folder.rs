//! A server's local folder, its logs among them: removed when its test passes, kept for the reader when it fails.

use std::path::{Path, PathBuf};

/// A folder made for one server; dropped while its thread unwinds, it stays and says where it is.
pub(crate) struct Folder(PathBuf);

impl Folder {
    /// Makes the folder at `path`.
    pub(crate) fn make(path: PathBuf) -> Self {
        std::fs::create_dir_all(&path).expect("a directory");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        if std::thread::panicking() {
            #[expect(clippy::print_stderr, reason = "a failing test's output is where its reader looks for the logs")]
            {
                eprintln!("the server's logs are kept in {}", self.0.display());
            }
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("chatkord-folder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn a_failing_test_keeps_its_server_s_folder_and_a_passing_one_removes_it() {
        let failed = made("failed");
        let kept = failed.clone();
        let unwound = std::thread::spawn(move || {
            let _folder = Folder::make(kept);
            panic!("the test failed");
        })
        .join();
        assert!(unwound.is_err());
        assert!(failed.is_dir(), "the failed test's folder is kept");
        let _ = std::fs::remove_dir_all(&failed);

        let passed = made("passed");
        drop(Folder::make(passed.clone()));
        assert!(!passed.exists(), "a passing test leaves nothing");
    }
}
