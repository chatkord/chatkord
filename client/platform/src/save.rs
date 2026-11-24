//! The system's own Save dialog, for where a downloaded file goes: the portal's FileChooser on Linux,
//! `IFileSaveDialog` on Windows. The name offered is the file's own, without any folder it names, and the person
//! chooses the place; nothing is written here.

use std::path::PathBuf;

use crate::Unavailable;

const TITLE: &str = "Save file";

/// Asks the person where to save a file, offering `suggested` as its name; none when they closed the dialog, and
/// [`Unavailable`] when this system has no dialog.
pub async fn file(suggested: &str) -> Result<Option<PathBuf>, Unavailable> {
    chosen(offered(suggested)).await
}

/// The last part of `suggested`, so a name that carries a folder cannot steer where the dialog opens.
fn offered(suggested: &str) -> String {
    suggested.rsplit(['/', '\\']).next().unwrap_or_default().trim().to_owned()
}

#[cfg(target_os = "linux")]
async fn chosen(name: String) -> Result<Option<PathBuf>, Unavailable> {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::file_chooser::SelectedFiles;
    let name = Some(name.as_str()).filter(|name| !name.is_empty());
    let asked = SelectedFiles::save_file().title(TITLE).modal(true).current_name(name).send().await;
    let answered = match asked.and_then(|request| request.response()) {
        Ok(files) => files,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(_) => return Err(Unavailable),
    };
    let Some(uri) = answered.uris().first() else { return Ok(None) };
    url::Url::parse(uri.as_str()).ok().and_then(|url| url.to_file_path().ok()).map(Some).ok_or(Unavailable)
}

#[cfg(windows)]
async fn chosen(name: String) -> Result<Option<PathBuf>, Unavailable> {
    crate::blocking(move || windows_dialog(&name)).await
}

/// The dialog on a blocking thread of its own, which it initialises for COM as a single-threaded apartment.
#[cfg(windows)]
fn windows_dialog(name: &str) -> Result<Option<PathBuf>, Unavailable> {
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FileSaveDialog, IFileSaveDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{HRESULT, HSTRING};

    // SAFETY: COM is initialised on this thread before any call below and uninitialised after the last.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok().map_err(|_| Unavailable)?;
    let picked = (|| -> Result<Option<PathBuf>, Unavailable> {
        // SAFETY: COM is initialised on this thread, above.
        let dialog: IFileSaveDialog =
            unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog; the title is copied by the call.
        unsafe { dialog.SetTitle(&HSTRING::from(TITLE)) }.map_err(|_| Unavailable)?;
        if !name.is_empty() {
            // SAFETY: a live dialog; the name is copied by the call.
            unsafe { dialog.SetFileName(&HSTRING::from(name)) }.map_err(|_| Unavailable)?;
        }
        // Windows hides a known extension in the name's box, so a name typed over it would otherwise be saved bare.
        if let Some(extension) = std::path::Path::new(name).extension() {
            // SAFETY: a live dialog; the extension is copied by the call.
            unsafe { dialog.SetDefaultExtension(&HSTRING::from(extension)) }.map_err(|_| Unavailable)?;
        }
        // SAFETY: a live dialog.
        let options = unsafe { dialog.GetOptions() }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog.
        unsafe { dialog.SetOptions(options | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM) }.map_err(|_| Unavailable)?;
        // SAFETY: a live dialog, shown on this thread's apartment.
        if let Err(failed) = unsafe { dialog.Show(None) } {
            return if failed.code() == HRESULT::from_win32(ERROR_CANCELLED.0) { Ok(None) } else { Err(Unavailable) };
        }
        // SAFETY: a live dialog that was shown.
        let item = unsafe { dialog.GetResult() }.map_err(|_| Unavailable)?;
        // SAFETY: a live item; the path's buffer is the shell's, freed once below.
        let path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|_| Unavailable)?;
        // SAFETY: the shell's buffer, a terminated wide string until it is freed below.
        let read = unsafe { path.to_string() }.map_err(|_| Unavailable);
        // SAFETY: the path's buffer is the shell's, freed once.
        unsafe { CoTaskMemFree(Some(path.0.cast_const().cast())) };
        read.map(|path| Some(PathBuf::from(path)))
    })();
    // SAFETY: paired with the initialisation above, after the closure dropped every interface.
    unsafe { CoUninitialize() };
    picked
}

#[cfg(not(any(target_os = "linux", windows)))]
async fn chosen(_name: String) -> Result<Option<PathBuf>, Unavailable> {
    Err(Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_offered_carries_no_folder() {
        assert_eq!(offered("report.pdf"), "report.pdf");
        assert_eq!(offered("../../.bashrc"), ".bashrc");
        assert_eq!(offered("..\\..\\start.bat"), "start.bat");
        assert_eq!(offered("folder/"), "");
    }
}
