//! A web link handed to the system's browser: the portal's OpenURI on Linux, `ShellExecuteW` on Windows. Only
//! `http` and `https` leave the window; every other scheme is refused here, whatever the caller asks, so a link a
//! message carries can never start a program or open a file.

/// Why a link was not opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum OpenError {
    /// The text is no URL.
    Malformed,
    /// A URL of a scheme other than `http` or `https`.
    NotWeb,
    /// This system has no way to open it, or refused to.
    Unavailable,
}

/// Opens `link` in the system's browser once it reads as an `http` or `https` URL.
pub async fn url(link: &str) -> Result<(), OpenError> {
    let link = web(link)?;
    launch(&link).await
}

/// `link` read as a web URL, or why it is not one.
fn web(link: &str) -> Result<url::Url, OpenError> {
    let parsed = url::Url::parse(link).map_err(|_| OpenError::Malformed)?;
    match parsed.scheme() {
        "http" | "https" if parsed.host().is_some() => Ok(parsed),
        "http" | "https" => Err(OpenError::Malformed),
        _ => Err(OpenError::NotWeb),
    }
}

#[cfg(target_os = "linux")]
async fn launch(link: &url::Url) -> Result<(), OpenError> {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::open_uri::OpenFileRequest;
    let uri = ashpd::Uri::parse(link.as_str()).map_err(|_| OpenError::Malformed)?;
    match OpenFileRequest::default().send_uri(&uri).await.and_then(|request| request.response()) {
        Ok(()) | Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(()),
        Err(_) => Err(OpenError::Unavailable),
    }
}

#[cfg(windows)]
async fn launch(link: &url::Url) -> Result<(), OpenError> {
    let link = link.as_str().to_owned();
    crate::blocking(move || Ok(windows_open(&link))).await.map_err(|_| OpenError::Unavailable)?
}

/// `ShellExecuteW` on a blocking thread, which it initialises for COM as the call's documentation asks, since the
/// browser's handler may be a shell extension.
#[cfg(windows)]
fn windows_open(link: &str) -> Result<(), OpenError> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::core::{HSTRING, w};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // SAFETY: COM is initialised on this thread before the call below and uninitialised after it.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|_| OpenError::Unavailable)?;
    let target = HSTRING::from(link);
    // SAFETY: both strings are terminated wide strings that outlive the call; no window, parameters or directory.
    let done = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            w!("open").0,
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // SAFETY: paired with the initialisation above.
    unsafe { CoUninitialize() };
    // A value above 32 is success, as the call's documentation states; at or below it is an error code.
    if done.addr() > 32 { Ok(()) } else { Err(OpenError::Unavailable) }
}

#[cfg(not(any(target_os = "linux", windows)))]
async fn launch(_link: &url::Url) -> Result<(), OpenError> {
    Err(OpenError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_web_url_with_a_host_is_opened() {
        assert!(web("https://example.invalid/a?b#c").is_ok());
        assert!(web("http://example.invalid").is_ok());
        assert_eq!(web("file:///etc/passwd").map(|_| ()), Err(OpenError::NotWeb));
        assert_eq!(web("ftp://example.invalid/channel/7").map(|_| ()), Err(OpenError::NotWeb));
        assert_eq!(web("javascript:alert(1)").map(|_| ()), Err(OpenError::NotWeb));
        assert_eq!(web("ms-settings:").map(|_| ()), Err(OpenError::NotWeb));
        assert_eq!(web("not a link").map(|_| ()), Err(OpenError::Malformed));
        assert_eq!(web("https://").map(|_| ()), Err(OpenError::Malformed));
    }
}
