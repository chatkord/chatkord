//! Files fetched from a server, kept on this device by their id (a file never changes, so its id is its
//! version): `<cache>/chatkord/files/<id>`, written whole and checked again whenever it is read. The folder is named
//! by [`Platform`]; a program that named none keeps nothing.

use std::path::{Path, PathBuf};

use crate::{Platform, Unavailable, blocking};
use portrait::{NotKept, Square};
use proto::files::FileId;

const FOLDER: &str = "chatkord";
const FILES: &str = "files";

/// The system's cache directory: `XDG_CACHE_HOME`, else `$HOME/.cache` on Linux; `LOCALAPPDATA` on Windows. Only a
/// path stated from the root counts.
#[cfg(target_os = "linux")]
pub(crate) fn directory() -> Option<PathBuf> {
    from_env("XDG_CACHE_HOME").or_else(|| from_env("HOME").map(|home| home.join(".cache")))
}

#[cfg(target_os = "windows")]
pub(crate) fn directory() -> Option<PathBuf> {
    from_env("LOCALAPPDATA")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn directory() -> Option<PathBuf> {
    None
}

fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var(name).ok().map(PathBuf::from).filter(|stated| stated.is_absolute())
}

fn path_in(folder: &Path, id: FileId) -> PathBuf {
    folder.join(FOLDER).join(FILES).join(id.to_string())
}

/// Takes away the partial files a dead run left among the pictures.
pub(crate) fn sweep(platform: &Platform, running: impl Fn(u32) -> bool, started: std::time::SystemTime) -> usize {
    platform.cache().map_or(0, |here| {
        crate::partial::sweep(&here.join(FOLDER).join(FILES), crate::partial::SUFFIX, running, started)
    })
}

/// The picture kept for `id`, if one is and it still reads as one; one that does not is taken away, to be fetched again.
pub async fn picture(platform: &Platform, id: FileId) -> Option<Square> {
    let here = platform.cache()?.to_path_buf();
    blocking(move || Ok(picture_in(&here, id))).await.ok().flatten()
}

/// Checks a fetched picture and keeps it for `id`: its pixels, or [`NotKept`] when it is not the PNG the server makes.
/// Keeping it is best effort: a device with no cache folder still gets the pixels.
pub async fn keep_picture(platform: &Platform, id: FileId, bytes: Vec<u8>) -> Result<Square, NotKept> {
    let here = platform.cache().map(Path::to_path_buf);
    blocking(move || {
        let square = portrait::after_fetch(&bytes);
        if let (Ok(_), Some(here)) = (&square, here) {
            let _ = write_in(&here, id, &bytes);
        }
        Ok(square)
    })
    .await
    .unwrap_or(Err(NotKept))
}

/// Takes away what is kept for `id`: a picture the server replaced.
pub async fn forget(platform: &Platform, id: FileId) {
    let Some(here) = platform.cache().map(Path::to_path_buf) else { return };
    let _ = blocking(move || {
        let _ = std::fs::remove_file(path_in(&here, id));
        Ok(())
    })
    .await;
}

/// Everything kept, taken away; nothing kept is done already.
pub(crate) fn remove(platform: &Platform) -> Result<(), Unavailable> {
    match platform.cache() {
        Some(here) => crate::removed(&here.join(FOLDER)),
        None => Ok(()),
    }
}

fn picture_in(folder: &Path, id: FileId) -> Option<Square> {
    let path = path_in(folder, id);
    let bytes = std::fs::read(&path).ok()?;
    match portrait::after_fetch(&bytes) {
        Ok(square) => Some(square),
        Err(NotKept) => {
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// Writes `bytes` beside `path` and renames them over it, so a reader never sees half a file.
fn whole(path: &Path, bytes: &[u8]) -> Result<(), Unavailable> {
    let parent = path.parent().ok_or(Unavailable)?;
    std::fs::create_dir_all(parent).map_err(|_| Unavailable)?;
    let mut partial = path.as_os_str().to_owned();
    partial.push(format!(".{}{}", std::process::id(), crate::partial::SUFFIX));
    std::fs::write(&partial, bytes).map_err(|_| Unavailable)?;
    std::fs::rename(&partial, path).map_err(|_| Unavailable)
}

/// Written beside its name and renamed over it, so a reader never sees half a file.
fn write_in(folder: &Path, id: FileId, bytes: &[u8]) -> Result<(), Unavailable> {
    whole(&path_in(folder, id), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::files::PICTURE_SIDE;

    const ID: &str = "0fa1000000000000000000000000ff10";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chatkord-cache-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn png(side: u32) -> Vec<u8> {
        let picture = image::RgbaImage::from_pixel(side, side, image::Rgba([120, 30, 60, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn a_kept_picture_reads_back_whole_and_a_spoiled_one_is_taken_away() {
        let dir = scratch("round");
        let id = FileId::parse(ID).unwrap();
        assert_eq!(picture_in(&dir, id), None, "nothing kept yet");
        write_in(&dir, id, &png(PICTURE_SIDE)).unwrap();
        let square = picture_in(&dir, id).expect("kept");
        assert_eq!(square, portrait::after_fetch(&png(PICTURE_SIDE)).unwrap());
        assert!(!dir.join(FOLDER).join(FILES).join(format!("{id}.part")).exists(), "no half file is left");

        std::fs::write(path_in(&dir, id), b"not a picture").unwrap();
        assert_eq!(picture_in(&dir, id), None, "a file that no longer reads is not shown");
        assert!(!path_in(&dir, id).exists(), "and is taken away, to be fetched again");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_a_picture_the_server_makes_is_kept() {
        let dir = scratch("keep");
        let platform = Platform::default().cache_in(dir.clone());
        let id = FileId::parse(ID).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        runtime.block_on(async {
            assert_eq!(keep_picture(&platform, id, png(PICTURE_SIDE - 1)).await, Err(NotKept));
            assert_eq!(picture(&platform, id).await, None, "a refused picture is not kept");
            let kept = keep_picture(&platform, id, png(PICTURE_SIDE)).await.expect("kept");
            assert_eq!(picture(&platform, id).await, Some(kept));
            forget(&platform, id).await;
            assert_eq!(picture(&platform, id).await, None, "forgotten");
        });
        let _ = std::fs::remove_dir_all(dir);
    }
}
