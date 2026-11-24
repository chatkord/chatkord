//! Windows' sources: every monitor and every window a person would call an application's, listed without any system
//! dialog, and the one chosen made into the item Windows Graphics Capture opens. They are shown in our own picker (the
//! window's); Windows' picker is not used. The command line, which has no picker, takes the primary monitor.

use crate::pick::{Cursor, Kind, Owner, Refused, Source, Want};
use std::ffi::c_void;
use std::num::NonZeroU32;
use windows::Graphics::Capture::{GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DEVMODEW, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow, SelectObject,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    DI_NORMAL, DrawIconEx, EnumWindows, GCLP_HICON, GCLP_HICONSM, GW_OWNER, GWL_EXSTYLE, GetClassLongPtrW, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HICON, ICON_BIG, ICON_SMALL2,
    IsIconic, IsWindowVisible, MONITORINFOF_PRIMARY, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETICON,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;
use windows::core::{PCWSTR, PWSTR};

/// The side of an application's icon as it is read: the picker shows it at 20 px, twice that on a dense screen.
pub(crate) const ICON_SIDE: u32 = 40;

/// How long a window may take to answer for its icon before it is listed without one: a window that hangs never holds
/// the picker up.
const ICON_WAIT_MS: u32 = 50;

/// A pick: the monitor or window chosen, and whether the pointer is drawn into it.
pub struct Selection {
    /// Which monitor or window was picked, as Windows names it: a window's capture asks by it whether the window is
    /// still there.
    pub id: Id,
    /// What was picked.
    pub source: Source,
    /// The capture item Windows made for it.
    pub item: GraphicsCaptureItem,
    /// Whether the pointer is drawn in.
    pub cursor: Cursor,
    /// A window's program, whose sound the share hears; none for a monitor, or a window whose program is gone.
    pub owner: Option<Owner>,
    /// How many times a second the monitor showing it refreshes, as Windows says (a rate such as 59.94 as the whole
    /// number below it); none where it does not say.
    pub refresh: Option<NonZeroU32>,
}

impl Selection {
    /// Nothing to give back: Windows asks no permission that outlives the capture.
    pub async fn close(self) {}
}

/// Which monitor or window a candidate is, as Windows names it while it exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    /// A monitor, by its `HMONITOR`.
    Monitor(isize),
    /// A window, by its `HWND`.
    Window(isize),
}

/// A monitor or window that can be shared, as our picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Which it is.
    pub id: Id,
    /// A monitor or a window.
    pub kind: Kind,
    /// A window's title; a monitor has none and is named by its place in the list.
    pub title: Option<String>,
    /// A monitor's size in pixels; a window's is not shown.
    pub size: Option<(u32, u32)>,
    /// A window's application icon, where it has one.
    pub icon: Option<Icon>,
}

/// An application's icon, `ICON_SIDE` square, as RGBA rows top down, the colour not premultiplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    /// Its width and height, in pixels.
    pub side: u32,
    /// Four bytes a pixel, `side * side` pixels.
    pub rgba: Vec<u8>,
}

/// Every monitor, the primary first and the rest left to right, then every window a person would choose to share, in
/// the order Windows stacks them (topmost first), Chatkord's own among them.
pub fn candidates() -> Vec<Candidate> {
    let mut list = monitors();
    list.extend(windows());
    list
}

/// The item Windows Graphics Capture opens for `id`; a monitor unplugged or a window closed since it was listed is
/// `Failed`.
pub fn select(id: Id, cursor: Cursor) -> Result<Selection, Refused> {
    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(Refused::Unavailable);
    }
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|_| Refused::Unavailable)?;
    let (owner, monitor) = match id {
        Id::Monitor(handle) => (None, HMONITOR(handle as *mut c_void)),
        // SAFETY: a window handle that is no longer valid gives the nearest monitor, never a fault.
        Id::Window(handle) => (owner(HWND(handle as *mut c_void)), unsafe {
            MonitorFromWindow(HWND(handle as *mut c_void), MONITOR_DEFAULTTONEAREST)
        }),
    };
    let (item, kind) = match id {
        // SAFETY: a monitor handle that is no longer valid is refused by the call, not used.
        Id::Monitor(handle) => (unsafe { interop.CreateForMonitor(HMONITOR(handle as *mut c_void)) }, Kind::Monitor),
        // SAFETY: as above, for a window handle.
        Id::Window(handle) => (unsafe { interop.CreateForWindow(HWND(handle as *mut c_void)) }, Kind::Window),
    };
    let item: GraphicsCaptureItem = item.map_err(|_| Refused::Failed)?;
    let size =
        item.Size().ok().and_then(|size| Some((u32::try_from(size.Width).ok()?, u32::try_from(size.Height).ok()?)));
    Ok(Selection { id, source: Source { kind: Some(kind), size }, item, cursor, owner, refresh: refresh(monitor) })
}

/// The command line's pick, which has no picker: the primary monitor; a window cannot be named from it.
pub async fn choose(want: Want, cursor: Cursor) -> Result<Selection, Refused> {
    if want == Want::Window {
        return Err(Refused::Unavailable);
    }
    let primary = monitors().into_iter().next().ok_or(Refused::Failed)?;
    select(primary.id, cursor)
}

fn monitors() -> Vec<Candidate> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the list `monitors` passed, alive for the whole enumeration and touched by nothing else.
        let found = unsafe { &mut *(found.0 as *mut Vec<(HMONITOR, MONITORINFOEXW)>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_u32::<MONITORINFOEXW>();
        // SAFETY: a monitor handle Windows just gave, and the extended value, whose size is said in it.
        if unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
            found.push((monitor, info));
        }
        true.into()
    }
    let mut found: Vec<(HMONITOR, MONITORINFOEXW)> = Vec::new();
    // SAFETY: the callback only pushes into `found`, which outlives the call.
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&raw mut found as isize)) };
    found.sort_by_key(|(_, info)| {
        let info = &info.monitorInfo;
        (info.dwFlags & MONITORINFOF_PRIMARY == 0, info.rcMonitor.left, info.rcMonitor.top)
    });
    found
        .into_iter()
        .map(|(monitor, info)| Candidate {
            id: Id::Monitor(monitor.0 as isize),
            kind: Kind::Monitor,
            title: None,
            size: pixels(&info.szDevice),
            icon: None,
        })
        .collect()
}

/// The display's size in pixels, from its current mode: the monitor's rectangle is scaled for a process that is not
/// aware of the display's scale, and the mode is not.
fn pixels(device: &[u16; 32]) -> Option<(u32, u32)> {
    let mode = mode(device)?;
    (mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0).then_some((mode.dmPelsWidth, mode.dmPelsHeight))
}

/// How many times a second `monitor` refreshes, from its current mode; 0 and 1 are the hardware's default, unsaid.
fn refresh(monitor: HMONITOR) -> Option<NonZeroU32> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_u32::<MONITORINFOEXW>();
    // SAFETY: a monitor handle, read only, and the extended value, whose size is said in it.
    if !unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
        return None;
    }
    NonZeroU32::new(mode(&info.szDevice)?.dmDisplayFrequency).filter(|hertz| hertz.get() > 1)
}

/// The display's current mode.
fn mode(device: &[u16; 32]) -> Option<DEVMODEW> {
    #[expect(clippy::cast_possible_truncation, reason = "DEVMODEW's size, far under u16::MAX")]
    let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: the device name is the NUL-terminated one Windows filled in; the mode's size is said in it.
    let read = unsafe { EnumDisplaySettingsW(PCWSTR(device.as_ptr()), ENUM_CURRENT_SETTINGS, &raw mut mode) };
    read.as_bool().then_some(mode)
}

fn windows() -> Vec<Candidate> {
    unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the list `windows` passed, alive for the whole enumeration and touched by nothing else.
        let found = unsafe { &mut *(found.0 as *mut Vec<HWND>) };
        if shareable(window) {
            found.push(window);
        }
        true.into()
    }
    let mut found: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `found`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut found as isize)) };
    found
        .into_iter()
        .filter_map(|window| {
            let title = title(window)?;
            Some(Candidate {
                id: Id::Window(window.0 as isize),
                kind: Kind::Window,
                title: Some(title),
                size: None,
                icon: icon(window),
            })
        })
        .collect()
}

/// Whether a person would call `window` an application's: shown, not minimized (Windows Graphics Capture draws a
/// minimized window as nothing), not hidden by the shell (cloaked), a top-level window of its own or one that asks to be
/// on the taskbar and not a tool window.
fn shareable(window: HWND) -> bool {
    // Each call reads a property of a window handle Windows just listed; a window gone since answers no.
    // SAFETY: a window handle, read only.
    if !unsafe { IsWindowVisible(window) }.as_bool() {
        return false;
    }
    // SAFETY: a window handle, read only.
    if unsafe { IsIconic(window) }.as_bool() {
        return false;
    }
    let mut cloaked = 0u32;
    // SAFETY: DWMWA_CLOAKED writes one u32, the size given, into `cloaked`.
    let hidden = unsafe { DwmGetWindowAttribute(window, DWMWA_CLOAKED, (&raw mut cloaked).cast(), size_u32::<u32>()) };
    if hidden.is_ok() && cloaked != 0 {
        return false;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the extended style is 32 bits of flags, which Windows hands back widened"
    )]
    // SAFETY: a window handle, read only.
    let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
    // SAFETY: a window handle, read only.
    let owned = unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid());
    style & WS_EX_TOOLWINDOW.0 == 0 && (!owned || style & WS_EX_APPWINDOW.0 != 0)
}

/// `T`'s size as a Win32 struct's size field takes it.
#[expect(clippy::cast_possible_truncation, reason = "a Win32 struct's size, far under u32::MAX")]
const fn size_u32<T>() -> u32 {
    size_of::<T>() as u32
}

/// The program that shows `window`: its process, named by its file's name without `.exe`.
fn owner(window: HWND) -> Option<Owner> {
    let mut pid = 0u32;
    // SAFETY: a window handle the picker listed; a window gone since leaves the id at zero.
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: a process id Windows just gave; the handle is closed below whatever the query answers.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut path = [0u16; 1024];
    let mut length = u32::try_from(path.len()).ok()?;
    // SAFETY: the buffer and its length in characters, which the call rewrites to the length it filled.
    let named =
        unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &raw mut length) };
    // SAFETY: the handle opened above, closed once.
    let _ = unsafe { CloseHandle(process) };
    named.ok()?;
    let path = String::from_utf16(path.get(..length as usize)?).ok()?;
    let app_id = std::path::Path::new(&path).file_stem()?.to_str()?.to_owned();
    Some(Owner { name: app_id.clone(), app_id, pids: vec![pid] })
}

/// The window's title; none for a window without one, which nobody could tell apart.
fn title(window: HWND) -> Option<String> {
    // SAFETY: a window handle, read only.
    let length = usize::try_from(unsafe { GetWindowTextLengthW(window) }).ok().filter(|length| *length > 0)?;
    let mut text = vec![0u16; length + 1];
    // SAFETY: the buffer is as long as Windows said the title is, plus its NUL, and the call writes no further.
    let read = usize::try_from(unsafe { GetWindowTextW(window, &mut text) }).ok()?;
    let title = String::from_utf16_lossy(&text[..read.min(length)]);
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// The window's own icon, else its class's, drawn at `ICON_SIDE`.
fn icon(window: HWND) -> Option<Icon> {
    let asked = |kind: u32| {
        let mut answer = 0usize;
        // SAFETY: a window handle and a message whose answer is an icon handle or nothing; a window that hangs is
        // given up on after `ICON_WAIT_MS`.
        let sent = unsafe {
            SendMessageTimeoutW(
                window,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                ICON_WAIT_MS,
                Some(&raw mut answer),
            )
        };
        (sent.0 != 0 && answer != 0).then_some(answer)
    };
    // SAFETY: reads a class property of a window handle; zero is none.
    let class = |index| Some(unsafe { GetClassLongPtrW(window, index) }).filter(|handle| *handle != 0);
    let handle =
        asked(ICON_BIG).or_else(|| asked(ICON_SMALL2)).or_else(|| class(GCLP_HICON)).or_else(|| class(GCLP_HICONSM))?;
    draw(HICON(handle as *mut c_void))
}

/// `icon` drawn into a 32-bit bitmap of our own and read back as RGBA. An icon with no alpha of its own (an old one,
/// drawn through its mask) is taken as opaque where it drew.
fn draw(icon: HICON) -> Option<Icon> {
    let side = i32::try_from(ICON_SIDE).ok()?;
    let header = BITMAPINFOHEADER {
        biSize: size_u32::<BITMAPINFOHEADER>(),
        biWidth: side,
        // Negative: the rows run top down.
        biHeight: -side,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // A memory DC and a DIB section of our own, each released below whatever happens; the icon handle belongs to its
    // window or class and is only drawn, never destroyed.
    // SAFETY: no arguments but the screen's, which None names.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    // SAFETY: the DC made above, a header that lives through the call, and `bits` for the section's memory.
    let made = unsafe { CreateDIBSection(Some(dc), &raw const info, DIB_RGB_COLORS, &raw mut bits, None, 0) };
    let bitmap = match made {
        Ok(bitmap) if !bits.is_null() => bitmap,
        Ok(bitmap) => {
            // SAFETY: the section made above, never selected, deleted once.
            let _ = unsafe { DeleteObject(bitmap.into()) };
            // SAFETY: the DC made above, deleted once.
            let _ = unsafe { DeleteDC(dc) };
            return None;
        }
        Err(_) => {
            // SAFETY: the DC made above, deleted once.
            let _ = unsafe { DeleteDC(dc) };
            return None;
        }
    };
    // SAFETY: the DC and section made above; what it held before is put back below.
    let before = unsafe { SelectObject(dc, bitmap.into()) };
    // SAFETY: the DC with our section selected, which is as large as the icon is drawn.
    let drawn = unsafe { DrawIconEx(dc, 0, 0, icon, side, side, 0, None, DI_NORMAL) }.is_ok();
    let length = (ICON_SIDE * ICON_SIDE * 4) as usize;
    // SAFETY: `bits` is the section's memory, 32 bits for each of ICON_SIDE² pixels, alive until the section goes.
    let mut rgba = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), length) }.to_vec();
    // SAFETY: the DC made above, given back what it held.
    unsafe { SelectObject(dc, before) };
    // SAFETY: the section, no longer selected, deleted once.
    let _ = unsafe { DeleteObject(bitmap.into()) };
    // SAFETY: the DC made above, deleted once.
    let _ = unsafe { DeleteDC(dc) };
    if !drawn {
        return None;
    }
    let has_alpha = rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0);
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        if !has_alpha {
            pixel[3] = if pixel[..3] == [0, 0, 0] { 0 } else { 255 };
        } else if pixel[3] != 0 {
            // Windows draws premultiplied: the colour is taken back out of the alpha.
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Some(Icon { side: ICON_SIDE, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_monitor_is_listed_first_the_primary_first_with_its_size() {
        let list = candidates();
        let monitors: Vec<_> = list.iter().take_while(|candidate| candidate.kind == Kind::Monitor).collect();
        assert!(!monitors.is_empty(), "a desktop has a monitor");
        assert!(list[monitors.len()..].iter().all(|candidate| candidate.kind == Kind::Window), "monitors come first");
        assert!(
            monitors.iter().all(|monitor| monitor.size.is_some_and(|(w, h)| w > 0 && h > 0) && monitor.title.is_none())
        );
        eprintln!("{} monitors, {} windows", monitors.len(), list.len() - monitors.len());
    }

    #[test]
    fn a_listed_monitor_selects_into_an_item_of_its_size() {
        let primary = candidates().into_iter().next().expect("a monitor");
        let selection = select(primary.id, Cursor::Embedded).expect("an item");
        assert_eq!(selection.source.size, primary.size);
        assert_eq!(selection.source.kind, Some(Kind::Monitor));
    }

    #[test]
    fn every_window_listed_has_a_title_and_an_icon_of_the_side_shown() {
        for window in candidates().into_iter().filter(|candidate| candidate.kind == Kind::Window) {
            assert!(window.title.as_deref().is_some_and(|title| !title.trim().is_empty()));
            if let Some(icon) = window.icon {
                assert_eq!((icon.side, icon.rgba.len()), (ICON_SIDE, (ICON_SIDE * ICON_SIDE * 4) as usize));
            }
        }
    }

    #[test]
    fn a_window_that_no_longer_exists_is_refused() {
        // Handles are multiples of four, and Windows never gives this one: it stands for a window closed since listing.
        let gone = Id::Window(0x7FFF_FFF0);
        assert!(matches!(select(gone, Cursor::Embedded), Err(Refused::Failed)));
    }
}
