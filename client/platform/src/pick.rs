//! The system's own file picker: the portal's FileChooser on Linux, `IFileOpenDialog` on Windows. For a picture it
//! offers PNG, JPEG and WebP, and what was picked is read here, at most one byte past the largest upload, so a file
//! too big is known as one without reading it whole; for files to attach it offers any kind, several at once, and
//! answers their paths unread.

use std::io::Read;
use std::path::Path;

use crate::{Unavailable, blocking};
use proto::files::FileKind;

/// What the picker came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Picked {
    /// The person closed the picker without choosing.
    Nothing,
    /// The file chosen, read to at most one byte past the kind's bound.
    File(Vec<u8>),
}

/// Asks the person for a picture; [`Unavailable`] when this system has no picker or the file could not be read.
pub async fn picture() -> Result<Picked, Unavailable> {
    match chosen().await? {
        None => Ok(Picked::Nothing),
        Some(path) => {
            blocking(move || read_bounded(&path, FileKind::ServerPicture.upload_bytes()).map(Picked::File)).await
        }
    }
}

/// A file named on a command line, read as a picked one is.
pub async fn file(path: std::path::PathBuf) -> Result<Vec<u8>, Unavailable> {
    blocking(move || read_bounded(&path, FileKind::ServerPicture.upload_bytes())).await
}

fn read_bounded(path: &Path, most: u64) -> Result<Vec<u8>, Unavailable> {
    let file = std::fs::File::open(path).map_err(|_| Unavailable)?;
    let mut bytes = Vec::new();
    file.take(most.saturating_add(1)).read_to_end(&mut bytes).map_err(|_| Unavailable)?;
    Ok(bytes)
}

/// Asks the person for files of any kind, several at once; none when they closed the picker, [`Unavailable`] when
/// this system has no picker. Nothing is read here.
pub async fn files() -> Result<Vec<std::path::PathBuf>, Unavailable> {
    chosen_files().await
}

const TITLE: &str = "Choose a picture";
const FILES_TITLE: &str = "Choose files";

#[cfg(target_os = "linux")]
async fn chosen() -> Result<Option<std::path::PathBuf>, Unavailable> {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
    let pictures =
        FileFilter::new("PNG, JPEG or WebP").mimetype("image/png").mimetype("image/jpeg").mimetype("image/webp");
    let asked = SelectedFiles::open_file().title(TITLE).modal(true).multiple(false).filter(pictures).send().await;
    let answered = match asked.and_then(|request| request.response()) {
        Ok(files) => files,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(_) => return Err(Unavailable),
    };
    let Some(uri) = answered.uris().first() else { return Ok(None) };
    url::Url::parse(uri.as_str()).ok().and_then(|url| url.to_file_path().ok()).map(Some).ok_or(Unavailable)
}

#[cfg(target_os = "linux")]
async fn chosen_files() -> Result<Vec<std::path::PathBuf>, Unavailable> {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::file_chooser::SelectedFiles;
    let asked = SelectedFiles::open_file().title(FILES_TITLE).modal(true).multiple(true).send().await;
    let answered = match asked.and_then(|request| request.response()) {
        Ok(files) => files,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(Vec::new()),
        Err(_) => return Err(Unavailable),
    };
    answered
        .uris()
        .iter()
        .map(|uri| url::Url::parse(uri.as_str()).ok().and_then(|url| url.to_file_path().ok()).ok_or(Unavailable))
        .collect()
}

#[cfg(windows)]
async fn chosen() -> Result<Option<std::path::PathBuf>, Unavailable> {
    blocking(windows_dialog).await
}

#[cfg(windows)]
async fn chosen_files() -> Result<Vec<std::path::PathBuf>, Unavailable> {
    blocking(windows_files_dialog).await
}

/// The dialog for any files, several at once, on a blocking thread it initialises for COM as `windows_dialog` does.
#[cfg(windows)]
fn windows_files_dialog() -> Result<Vec<std::path::PathBuf>, Unavailable> {
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
        SIGDN_FILESYSPATH,
    };
    use windows::core::{HRESULT, HSTRING};

    // SAFETY: COM is initialised on this thread before any call below and uninitialised after the last.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok().map_err(|_| Unavailable)?;
    let picked = (|| -> Result<Vec<std::path::PathBuf>, Unavailable> {
        // SAFETY: COM is initialised on this thread, above.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog; the title is copied by the call.
        unsafe { dialog.SetTitle(&HSTRING::from(FILES_TITLE)) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog.
        let options = unsafe { dialog.GetOptions() }.map_err(|_| Unavailable)?;
        let options = options | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT;
        // SAFETY: a live dialog.
        unsafe { dialog.SetOptions(options) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog, shown on this thread's apartment.
        if let Err(failed) = unsafe { dialog.Show(None) } {
            return if failed.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                Ok(Vec::new())
            } else {
                Err(Unavailable)
            };
        }
        // SAFETY: a live dialog that was shown.
        let items = unsafe { dialog.GetResults() }.map_err(|_| Unavailable)?;
        // SAFETY: a live item array.
        let count = unsafe { items.GetCount() }.map_err(|_| Unavailable)?;
        (0..count)
            .map(|at| {
                // SAFETY: a live item array, `at` below its count.
                let item = unsafe { items.GetItemAt(at) }.map_err(|_| Unavailable)?;
                // SAFETY: a live item; the path's buffer is the shell's, freed once below.
                let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|_| Unavailable)?;
                // SAFETY: the shell's buffer, a terminated wide string until it is freed below.
                let path = unsafe { name.to_string() }.map_err(|_| Unavailable);
                // SAFETY: the path's buffer is the shell's, freed once.
                unsafe { CoTaskMemFree(Some(name.0.cast_const().cast())) };
                path.map(std::path::PathBuf::from)
            })
            .collect()
    })();
    // SAFETY: paired with the initialisation above, after the closure dropped every interface.
    unsafe { CoUninitialize() };
    picked
}

/// The dialog on a blocking thread of its own, which it initialises for COM as a single-threaded apartment.
#[cfg(windows)]
fn windows_dialog() -> Result<Option<std::path::PathBuf>, Unavailable> {
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{HRESULT, HSTRING, w};

    // SAFETY: COM is initialised on this thread before any call below and uninitialised after the last.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok().map_err(|_| Unavailable)?;
    let picked = (|| -> Result<Option<std::path::PathBuf>, Unavailable> {
        // SAFETY: COM is initialised on this thread, above.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.map_err(|_| Unavailable)?;
        let filters =
            [COMDLG_FILTERSPEC { pszName: w!("PNG, JPEG or WebP"), pszSpec: w!("*.png;*.jpg;*.jpeg;*.webp") }];
        // SAFETY: the filter strings are 'static wide literals.
        unsafe { dialog.SetFileTypes(&filters) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog; the title is copied by the call.
        unsafe { dialog.SetTitle(&HSTRING::from(TITLE)) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog.
        let options = unsafe { dialog.GetOptions() }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog.
        unsafe { dialog.SetOptions(options | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog, shown on this thread's apartment.
        if let Err(failed) = unsafe { dialog.Show(None) } {
            return if failed.code() == HRESULT::from_win32(ERROR_CANCELLED.0) { Ok(None) } else { Err(Unavailable) };
        }
        // SAFETY: a live dialog that was shown.
        let item = unsafe { dialog.GetResult() }.map_err(|_| Unavailable)?;
        // SAFETY: a live item; the path's buffer is the shell's, freed once below.
        let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|_| Unavailable)?;
        // SAFETY: the shell's buffer, a terminated wide string until it is freed below.
        let path = unsafe { name.to_string() }.map_err(|_| Unavailable);
        // SAFETY: the path's buffer is the shell's, freed once.
        unsafe { CoTaskMemFree(Some(name.0.cast_const().cast())) };
        path.map(|path| Some(std::path::PathBuf::from(path)))
    })();
    // SAFETY: paired with the initialisation above, after the closure dropped every interface.
    unsafe { CoUninitialize() };
    picked
}

#[cfg(not(any(target_os = "linux", windows)))]
async fn chosen() -> Result<Option<std::path::PathBuf>, Unavailable> {
    Err(Unavailable)
}

#[cfg(not(any(target_os = "linux", windows)))]
async fn chosen_files() -> Result<Vec<std::path::PathBuf>, Unavailable> {
    Err(Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_read_to_one_byte_past_the_bound_and_no_further() {
        let path = std::env::temp_dir().join(format!("chatkord-pick-{}", std::process::id()));
        std::fs::write(&path, vec![7u8; 100]).unwrap();
        assert_eq!(read_bounded(&path, 200).unwrap().len(), 100, "a small file whole");
        assert_eq!(read_bounded(&path, 10).unwrap().len(), 11, "one past, so it is known as too big");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_bounded(&path, 10), Err(Unavailable), "a file gone is said");
    }
}
