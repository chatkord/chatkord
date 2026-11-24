//! The debug report's two errands: the recent log, and the file the person chose to keep it in.

use screens::{Download, Saved};
use std::path::Path;

/// Asks the system where to save `download`, then writes its text there.
pub(crate) async fn save(download: Download) -> Saved {
    let path = match platform::save::file(&download.name).await {
        Ok(Some(path)) => path,
        Ok(None) => return Saved::Declined,
        Err(platform::Unavailable) => return Saved::Failed,
    };
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
    // The pool, not a task that serves sockets: the disk may be slow.
    match crate::errand::blocking(report::word!("write the report"), move || write(&path, &download)).await {
        Ok(Ok(())) => name.map_or(Saved::Failed, Saved::Written),
        Ok(Err(_)) | Err(_) => Saved::Failed,
    }
}

/// `download`'s bytes at `path`, which a person chose; nothing else is written.
fn write(path: &Path, download: &Download) -> std::io::Result<()> {
    std::fs::write(path, download.bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_holds_the_text_shown_byte_for_byte() {
        let text = "Chatkord debug report: client\n  theme: dark\n  caf\u{e9}: \u{2713}\n".to_owned();
        let download = Download::of_text("chatkord-client-report-1.txt".into(), &text);
        let folder = std::env::temp_dir().join(format!("chatkord-errands-report-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(&download.name);
        write(&path, &download).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
