//! The tray icon on Windows: the notification area's icon (`Shell_NotifyIconW`), on a thread of its own whose hidden
//! window takes the icon's messages. A left click opens the window, a right click shows the menu where the pointer
//! is; the icon is added again when Explorer starts again (`TaskbarCreated`).

use super::{Picked, Picture};
use crate::Unavailable;
use screens::{TrayItem, TrayMenu, TrayPick};
use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HICON, ICONINFO, MF_CHECKED,
    MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPED,
};

/// The icon's own messages, as `uCallbackMessage` names them.
const ICON: u32 = WM_APP + 1;
/// A new menu waits in the thread's channel.
const MENU: u32 = WM_APP + 2;
/// The icon's id among this window's icons: one.
const ID: u32 = 1;
/// The icon chosen by the pointer, and by the keyboard (version 4's events).
const SELECTED: u32 = NIN_SELECT;
const KEY_SELECTED: u32 = NIN_SELECT | NINF_KEY;

/// The icon's thread and its window, which the owner posts to.
pub(super) struct Icon {
    window: usize,
    menus: mpsc::Sender<TrayMenu>,
    thread: Option<JoinHandle<()>>,
}

impl Icon {
    /// Shows the icon on a thread of its own; [`Unavailable`] where the notification area takes none.
    pub(super) fn show(menu: TrayMenu, pictures: &[Picture], picked: Picked) -> Result<Self, Unavailable> {
        let picture = nearest(pictures).ok_or(Unavailable)?;
        let (side, rgba) = (picture.side, picture.rgba.clone());
        let (menus, menus_in) = mpsc::channel();
        let (opened, open) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || serve(Told { menu, menus: menus_in, picked, side, rgba }, &opened))
            .map_err(|_| Unavailable)?;
        match open.recv() {
            Ok(Ok(window)) => Ok(Self { window, menus, thread: Some(thread) }),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => Err(Unavailable),
        }
    }

    /// Shows `menu`'s name and entries in place of those shown.
    pub(super) fn update(&self, menu: TrayMenu) {
        if self.menus.send(menu).is_ok() {
            // SAFETY: the thread's window, alive until this is dropped; the message carries nothing.
            unsafe { PostMessageW(self.window as HWND, MENU, 0, 0) };
        }
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: as in `update`; closing the window takes the icon down and ends the thread's loop.
        unsafe { PostMessageW(self.window as HWND, WM_CLOSE, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The picture nearest the notification area's own icon size, the next larger one first.
fn nearest(pictures: &[Picture]) -> Option<&Picture> {
    // SAFETY: reads a system metric.
    let wanted = u32::try_from(unsafe { GetSystemMetrics(SM_CXSMICON) }).unwrap_or(16);
    pictures
        .iter()
        .filter(|picture| picture.side >= wanted)
        .min_by_key(|picture| picture.side)
        .or_else(|| pictures.iter().max_by_key(|picture| picture.side))
}

/// What the thread's window answers with.
struct Told {
    menu: TrayMenu,
    menus: mpsc::Receiver<TrayMenu>,
    picked: Picked,
    side: u32,
    rgba: Vec<u8>,
}

/// The thread's state, which only its window's procedure reads.
struct Serving {
    told: Told,
    icon: HICON,
    /// Explorer's message that the taskbar was made again, when the icon must be added again.
    taskbar_created: u32,
}

thread_local! {
    static SERVING: RefCell<Option<Serving>> = const { RefCell::new(None) };
}

/// Makes the window and the icon, says which, and answers the icon's messages until the owner closes the window.
fn serve(told: Told, opened: &mpsc::SyncSender<Result<usize, Unavailable>>) {
    let class: Vec<u16> = "ChatkordTray".encode_utf16().chain([0]).collect();
    // SAFETY: a null name is this process's own module.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    let registered = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
        lpfnWndProc: Some(procedure),
        hInstance: module,
        lpszClassName: class.as_ptr(),
        // SAFETY: the rest of the class is unused and zero is its absence.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: a class whose name and procedure live for the process. A second icon finds it registered.
    unsafe { RegisterClassExW(&raw const registered) };
    // SAFETY: the class above; a top-level window, never shown, so Explorer's broadcasts reach it.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        )
    };
    let Some(icon) = (!window.is_null()).then(|| picture_icon(told.side, &told.rgba)).flatten() else {
        let _ = opened.send(Err(Unavailable));
        return;
    };
    let name: Vec<u16> = "TaskbarCreated".encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated name alive across the call.
    let taskbar_created = unsafe { RegisterWindowMessageW(name.as_ptr()) };
    let title = told.menu.title.clone();
    SERVING.with(|serving| *serving.borrow_mut() = Some(Serving { told, icon, taskbar_created }));
    if !add(window, icon, &title) {
        // SAFETY: the window made above, on this thread.
        unsafe { DestroyWindow(window) };
        // SAFETY: the icon made above, used by nothing now.
        unsafe { DestroyIcon(icon) };
        let _ = opened.send(Err(Unavailable));
        return;
    }
    let _ = opened.send(Ok(window as usize));
    // SAFETY: an out buffer for each message; zero is a message the loop never reads.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: this thread's own queue, read into `message`.
    while unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        // SAFETY: a message the loop just read.
        unsafe { TranslateMessage(&raw const message) };
        // SAFETY: as above.
        unsafe { DispatchMessageW(&raw const message) };
    }
    SERVING.with(|serving| {
        if let Some(serving) = serving.borrow_mut().take() {
            // SAFETY: the icon made above, the notification area's copy gone with the window.
            unsafe { DestroyIcon(serving.icon) };
        }
    });
}

/// The icon's record in the notification area, with what `flags` says it carries.
fn record(window: HWND, icon: HICON, flags: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: window,
        uID: ID,
        uFlags: flags,
        uCallbackMessage: ICON,
        hIcon: icon,
        // SAFETY: the rest of the record is unused and zero is its absence.
        ..unsafe { std::mem::zeroed() }
    }
}

/// `data` carrying `tip`, the name a pointer over the icon shows, cut to the record's room and its NUL kept.
fn tipped(mut data: NOTIFYICONDATAW, tip: &str) -> NOTIFYICONDATAW {
    data.uFlags |= NIF_TIP;
    let room = data.szTip.len().saturating_sub(1);
    data.szTip.fill(0);
    for (into, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(room)) {
        *into = unit;
    }
    data
}

/// Puts the icon up named `title` and asks for version 4's messages; whether the notification area took it.
fn add(window: HWND, icon: HICON, title: &str) -> bool {
    let data = tipped(record(window, icon, NIF_ICON | NIF_MESSAGE), title);
    // SAFETY: a record of this window's icon, alive across the call.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) } == 0 {
        return false;
    }
    let mut versioned = record(window, icon, 0);
    versioned.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: as above.
    unsafe { Shell_NotifyIconW(NIM_SETVERSION, &raw const versioned) };
    true
}

/// An icon of `side` pixels square from RGBA, row by row, its alpha kept.
fn picture_icon(side: u32, rgba: &[u8]) -> Option<HICON> {
    let side = i32::try_from(side).ok()?;
    let header = BITMAPINFOHEADER {
        biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
        biWidth: side,
        // Negative: the rows top down, as the picture's are.
        biHeight: -side,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        // SAFETY: the rest of the header is unused and zero is its absence.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: zero is an empty colour table, which a 32-bit bitmap has.
    let info = BITMAPINFO { bmiHeader: header, ..unsafe { std::mem::zeroed() } };
    let mut bits = std::ptr::null_mut();
    // SAFETY: a header alive across the call and a valid out pointer for the pixels Windows allocates.
    let colour = unsafe {
        CreateDIBSection(std::ptr::null_mut(), &raw const info, DIB_RGB_COLORS, &raw mut bits, std::ptr::null_mut(), 0)
    };
    if colour.is_null() || bits.is_null() {
        return None;
    }
    let pixels = usize::try_from(side).ok()?.pow(2);
    // SAFETY: Windows allocated `side` rows of `side` four-byte pixels at `bits`, ours until the bitmap is deleted.
    let into = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), pixels * 4) };
    for (into, &[r, g, b, a]) in into.as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
        *into = [b, g, r, a];
    }
    // SAFETY: a one-bit mask of the same size; its bits are ignored where the colour has alpha.
    let mask: HBITMAP = unsafe { CreateBitmap(side, side, 1, 1, std::ptr::null()) };
    let made = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: colour };
    // SAFETY: an icon description of two live bitmaps, which the icon copies.
    let icon = unsafe { CreateIconIndirect(&raw const made) };
    // SAFETY: the two bitmaps made above, copied into the icon and used by nothing else.
    unsafe {
        DeleteObject(colour);
    }
    // SAFETY: as above.
    unsafe {
        DeleteObject(mask);
    }
    (!icon.is_null()).then_some(icon)
}

/// Shows the menu at the pointer and tells what was picked.
fn pop_up(window: HWND, items: &[TrayItem], picked: &Picked) {
    // SAFETY: makes an empty menu, destroyed below.
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    for (index, item) in items.iter().enumerate() {
        let label: Vec<u16> = item.label.encode_utf16().chain([0]).collect();
        let checked = if item.ticked == Some(true) { MF_CHECKED } else { 0 };
        // SAFETY: the menu made above and a NUL-terminated label alive across the call; an id one past the index.
        unsafe { AppendMenuW(menu, MF_STRING | checked, index + 1, label.as_ptr()) };
    }
    let mut at = POINT { x: 0, y: 0 };
    // SAFETY: a valid out pointer.
    unsafe { GetCursorPos(&raw mut at) };
    // The menu closes when the person clicks elsewhere only if its window is in front first.
    // SAFETY: this thread's own window.
    unsafe { SetForegroundWindow(window) };
    // SAFETY: the menu and window above; with `TPM_RETURNCMD` the pick is returned, not posted.
    let chosen =
        unsafe { TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, window, std::ptr::null()) };
    // SAFETY: the menu made above, shown no more.
    unsafe { DestroyMenu(menu) };
    let item = usize::try_from(chosen).ok().and_then(|id| id.checked_sub(1)).and_then(|index| items.get(index));
    if let Some(item) = item {
        picked(item.pick);
    }
}

/// What the window does with a message, read from the thread's state and done once the state is let go: a call into
/// Windows can send this window another message at once (`DestroyWindow` sends `WM_DESTROY`, a menu runs a loop of its
/// own), which reads the state again.
enum Act {
    Open(Picked),
    Menu(Vec<TrayItem>, Picked),
    Rename(HICON, String),
    Remove(HICON),
    AddAgain(HICON, String),
    Quit,
    Done,
}

/// The icon's messages, a new menu, Explorer started again, and the close that ends the thread.
unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let act = SERVING.with(|serving| {
        let mut serving = serving.borrow_mut();
        let serving = serving.as_mut()?;
        Some(match message {
            // Version 4 puts the icon's own event in the low word.
            ICON => match u32::try_from(lparam & 0xffff).unwrap_or(0) {
                SELECTED | KEY_SELECTED => Act::Open(serving.told.picked.clone()),
                WM_CONTEXTMENU => Act::Menu(serving.told.menu.items.clone(), serving.told.picked.clone()),
                _ => Act::Done,
            },
            MENU => match serving.told.menus.try_iter().last() {
                Some(menu) => {
                    let renamed = menu.title != serving.told.menu.title;
                    serving.told.menu = menu;
                    if renamed { Act::Rename(serving.icon, serving.told.menu.title.clone()) } else { Act::Done }
                }
                None => Act::Done,
            },
            WM_CLOSE => Act::Remove(serving.icon),
            WM_DESTROY => Act::Quit,
            created if created == serving.taskbar_created && created != 0 => {
                Act::AddAgain(serving.icon, serving.told.menu.title.clone())
            }
            _ => return None,
        })
    });
    match act {
        Some(Act::Open(picked)) => picked(TrayPick::Open),
        Some(Act::Menu(items, picked)) => pop_up(window, &items, &picked),
        Some(Act::Rename(icon, title)) => {
            let data = tipped(record(window, icon, 0), &title);
            // SAFETY: the record of the icon this window added, alive across the call.
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) };
        }
        Some(Act::Remove(icon)) => {
            let data = record(window, icon, 0);
            // SAFETY: the record of the icon this window added.
            unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
            // SAFETY: this thread's own window.
            unsafe { DestroyWindow(window) };
        }
        Some(Act::AddAgain(icon, title)) => {
            add(window, icon, &title);
        }
        Some(Act::Quit) => {
            // SAFETY: ends this thread's own loop.
            unsafe { PostQuitMessage(0) };
        }
        Some(Act::Done) => {}
        // SAFETY: the window's own message, handed on as it came.
        None => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_nearest_the_small_icon_size_is_shown_the_larger_first() {
        let pictures: Vec<Picture> = [16, 32, 64].into_iter().map(|side| Picture { side, rgba: Vec::new() }).collect();
        // SAFETY: as in `nearest`.
        let wanted = u32::try_from(unsafe { GetSystemMetrics(SM_CXSMICON) }).unwrap();
        let shown = nearest(&pictures).unwrap().side;
        assert!(shown >= wanted.min(64), "{shown} for {wanted}");
        assert!(pictures.iter().all(|picture| picture.side < wanted || picture.side >= shown), "the nearest above");
    }

    #[test]
    fn the_name_is_carried_cut_to_the_records_room_and_ended() {
        let long = "Chatkord ".repeat(20);
        let data = tipped(record(std::ptr::null_mut(), std::ptr::null_mut(), NIF_ICON), &long);
        assert_eq!(data.uFlags, NIF_ICON | NIF_TIP);
        let kept: Vec<u16> = long.encode_utf16().take(data.szTip.len() - 1).collect();
        assert_eq!(&data.szTip[..kept.len()], kept.as_slice());
        assert_eq!(data.szTip.last(), Some(&0));
        let short = tipped(record(std::ptr::null_mut(), std::ptr::null_mut(), 0), "Chatkord");
        let said: String = char::decode_utf16(short.szTip.iter().copied().take_while(|unit| *unit != 0))
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(said, "Chatkord");
    }

    #[test]
    fn an_icon_is_made_from_rgba() {
        let rgba = [255u8, 0, 0, 255].repeat(16 * 16);
        let icon = picture_icon(16, &rgba).expect("an icon");
        // SAFETY: the icon made above.
        unsafe { DestroyIcon(icon) };
    }
}
