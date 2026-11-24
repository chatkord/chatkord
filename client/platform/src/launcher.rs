//! The launcher's entry for a window run from an AppImage, which installs nothing: each start keeps
//! `<data>/applications/<id>.desktop` and its icons naming the image's own file, so the launcher offers it wherever
//! the file was moved, and a Wayland desktop, which takes a window's icon only from the entry its app id names, shows
//! it. An entry something else made (a package, the tree's `scripts/desktop-entry.sh`) is left as it is; `forget` takes
//! this one away. Its `TryExec` names the file too, so a launcher hides the entry once the file is gone, and the
//! autostart entry the same file made follows it. Nothing is written under `DESKTOPINTEGRATION`, which an AppImage
//! integration tool sets, as it makes the entries itself.
//!
//! On Windows the setup's Start Menu shortcut is the entry, and each start names the process by the id that shortcut
//! carries, so the taskbar files the window under it and a toast is the program's.
//!
//! The count on the app's icon goes to the launcher as Unity's `LauncherEntry` on the session bus: a dock that reads
//! it (Ubuntu's, Plasma's task manager) shows it in place of the number of our popups it holds.

use crate::Unavailable;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod unity;

/// The desktop's entry for this program, which a launcher and the notification server know it by.
#[cfg(target_os = "linux")]
pub(crate) const DESKTOP_ENTRY: &str = channel::THIS.id;

/// The app icon at each size the desktop asks for, the window's own (`client/shell/assets/icon`).
#[cfg(target_os = "linux")]
const ICONS: [(u32, &[u8]); 8] = [
    (16, include_bytes!("../../shell/assets/icon/chatkord-16.png")),
    (24, include_bytes!("../../shell/assets/icon/chatkord-24.png")),
    (32, include_bytes!("../../shell/assets/icon/chatkord-32.png")),
    (48, include_bytes!("../../shell/assets/icon/chatkord-48.png")),
    (64, include_bytes!("../../shell/assets/icon/chatkord-64.png")),
    (128, include_bytes!("../../shell/assets/icon/chatkord-128.png")),
    (256, include_bytes!("../../shell/assets/icon/chatkord-256.png")),
    (512, include_bytes!("../../shell/assets/icon/chatkord-512.png")),
];

/// The launcher's entry kept for the AppImage this window runs from, if it runs from one, and on Windows the process
/// named as the Start Menu shortcut names it. Blocking, and before the window opens: a Wayland desktop looks for the
/// entry once, when the window appears, and Windows' taskbar reads the process's id as its first window is made.
pub fn keep(platform: &crate::Platform) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        let Some(image) = image(std::env::var_os("APPIMAGE"), std::env::var_os("DESKTOPINTEGRATION").as_deref()) else {
            return Ok(());
        };
        keep_in(&data().ok_or(Unavailable)?, &image)?;
        crate::startup::follow(platform.config().ok_or(Unavailable)?, &image)
    }
    #[cfg(windows)]
    {
        let _ = platform;
        let id = windows::core::HSTRING::from(crate::notify::APP_USER_MODEL_ID);
        // SAFETY: `id` is a live, null-terminated wide string for the call, which copies it.
        unsafe { windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&id) }.map_err(|_| Unavailable)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = platform;
        Ok(())
    }
}

/// The count on the app's icon as the launcher shows it, told by a task of its own, so the last one shown is the one
/// told; dropped, it is taken away.
pub struct Count {
    told: tokio::sync::watch::Sender<Option<u32>>,
}

impl Count {
    /// The count, none shown yet, and the task that tells the launcher each change until the count is dropped. A
    /// desktop with no session bus ends it [`Unavailable`]; Windows' taskbar is told nothing yet.
    pub fn new() -> (Count, impl Future<Output = Result<(), Unavailable>> + Send + 'static) {
        let (told, counts) = tokio::sync::watch::channel(None);
        (Count { told }, tell(counts))
    }

    /// Shows the count `badge` makes, or none where it makes no number.
    pub fn show(&self, badge: screens::Badge) {
        let count = badge.count();
        self.told.send_if_modified(|shown| {
            let changed = *shown != count;
            *shown = count;
            changed
        });
    }
}

async fn tell(counts: tokio::sync::watch::Receiver<Option<u32>>) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        unity::serve(counts).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(counts);
        Ok(())
    }
}

/// The entry this module made, and its icons, taken away; any other entry stays.
pub(crate) fn remove() -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        data().map_or(Ok(()), |data| remove_in(&data))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(())
    }
}

/// The system's data directory: `XDG_DATA_HOME`, else `$HOME/.local/share`. Only a path stated from the root counts.
#[cfg(target_os = "linux")]
fn data() -> Option<PathBuf> {
    crate::state::from_env("XDG_DATA_HOME")
        .or_else(|| crate::state::from_env("HOME").map(|home| home.join(".local").join("share")))
}

/// The AppImage's own file, when this module keeps entries for it: `appimage` is whole, and no integration tool,
/// which sets `integration`, has made entries of its own.
#[cfg(target_os = "linux")]
fn image(appimage: Option<std::ffi::OsString>, integration: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if integration.is_some() {
        return None;
    }
    appimage.map(PathBuf::from).filter(|image| image.is_absolute())
}

/// The folders the entry and its icons are written in, where the system has a data directory.
#[cfg(target_os = "linux")]
pub(crate) fn data_folders() -> Option<Vec<PathBuf>> {
    let data = data()?;
    let mut folders = vec![data.join("applications")];
    for (side, _) in ICONS {
        folders.extend(icon_path(&data, side).parent().map(Path::to_path_buf));
    }
    Some(folders)
}

#[cfg(target_os = "linux")]
fn entry_path(data: &Path) -> PathBuf {
    data.join("applications").join(format!("{DESKTOP_ENTRY}.desktop"))
}

#[cfg(target_os = "linux")]
fn icon_path(data: &Path, side: u32) -> PathBuf {
    data.join("icons").join("hicolor").join(format!("{side}x{side}")).join("apps").join(format!("{DESKTOP_ENTRY}.png"))
}

/// The entry this module made: none there, or one it marked.
#[cfg(target_os = "linux")]
enum Found {
    None,
    Ours(String),
    Another,
}

#[cfg(target_os = "linux")]
fn found(data: &Path) -> Result<Found, Unavailable> {
    match std::fs::read_to_string(entry_path(data)) {
        Ok(entry) if entry.lines().any(|line| line == crate::startup::MARK) => Ok(Found::Ours(entry)),
        Ok(_) => Ok(Found::Another),
        Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => Ok(Found::None),
        Err(_) => Err(Unavailable),
    }
}

#[cfg(target_os = "linux")]
fn keep_in(data: &Path, image: &Path) -> Result<(), Unavailable> {
    let wanted = crate::startup::entry(image, None) + crate::startup::MARK + "\n";
    match found(data)? {
        Found::Another => return Ok(()),
        Found::Ours(entry) if entry == wanted => return Ok(()),
        Found::Ours(_) | Found::None => {}
    }
    for (side, png) in ICONS {
        crate::startup::replace(&icon_path(data, side), png)?;
    }
    crate::startup::replace(&entry_path(data), wanted.as_bytes())
}

#[cfg(target_os = "linux")]
fn remove_in(data: &Path) -> Result<(), Unavailable> {
    let Found::Ours(_) = found(data)? else { return Ok(()) };
    let gone = |path: PathBuf| match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Unavailable),
    };
    for (side, _) in ICONS {
        gone(icon_path(data, side))?;
    }
    gone(entry_path(data))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-launcher-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn an_appimage_keeps_its_entry_and_icons_following_its_file_and_forget_takes_them() {
        let data = scratch("kept");
        keep_in(&data, Path::new("/home/x/Applications/Chatkord.AppImage")).unwrap();
        let entry = std::fs::read_to_string(entry_path(&data)).unwrap();
        assert!(entry.contains("Exec=\"/home/x/Applications/Chatkord.AppImage\"\n"), "{entry}");
        let id = channel::THIS.id;
        assert!(
            entry.contains(&format!("Icon={id}\n")) && entry.contains(&format!("StartupWMClass={id}\n")),
            "{entry}"
        );
        assert!(entry.contains("TryExec=/home/x/Applications/Chatkord.AppImage\n"), "{entry}");
        assert!(ICONS.iter().all(|(side, _)| icon_path(&data, *side).exists()));
        assert_eq!(
            entry_path(&data).file_name().and_then(|name| name.to_str()),
            Some(format!("{id}.desktop").as_str())
        );
        assert!(icon_path(&data, 48).ends_with(format!("48x48/apps/{id}.png")));
        keep_in(&data, Path::new("/home/x/Downloads/Chatkord.AppImage")).unwrap();
        let moved = std::fs::read_to_string(entry_path(&data)).unwrap();
        assert!(moved.contains("Exec=\"/home/x/Downloads/Chatkord.AppImage\"\n"), "the file moved: {moved}");
        remove_in(&data).unwrap();
        assert!(!entry_path(&data).exists() && ICONS.iter().all(|(side, _)| !icon_path(&data, *side).exists()));
    }

    /// A desktop that caches its menu (Plasma's ksycoca) rereads an entry when its folder changes, which a rename does
    /// and a write in place does not: an entry rewritten in place kept starting the file from where it was.
    #[test]
    fn an_entry_that_follows_its_file_is_a_new_file_its_folder_shows() {
        use std::os::unix::fs::MetadataExt;
        let data = scratch("renamed");
        keep_in(&data, Path::new("/home/x/Old.AppImage")).unwrap();
        let was = std::fs::metadata(entry_path(&data)).unwrap().ino();
        keep_in(&data, Path::new("/home/x/Applications/Chatkord.AppImage")).unwrap();
        assert_ne!(std::fs::metadata(entry_path(&data)).unwrap().ino(), was, "rewritten in place");
        let parts = std::fs::read_dir(entry_path(&data).parent().unwrap()).unwrap().count();
        assert_eq!(parts, 1, "nothing left beside it");
    }

    #[test]
    fn only_a_whole_appimage_path_with_no_integration_tool_is_kept() {
        let appimage = || Some(std::ffi::OsString::from("/x/C.AppImage"));
        assert_eq!(image(appimage(), None), Some(PathBuf::from("/x/C.AppImage")));
        assert_eq!(image(appimage(), Some(std::ffi::OsStr::new("1"))), None, "an integration tool made the entries");
        assert_eq!(image(Some("C.AppImage".into()), None), None);
        assert_eq!(image(None, None), None);
    }

    #[test]
    fn an_entry_something_else_made_is_neither_replaced_nor_removed() {
        let data = scratch("another");
        let path = entry_path(&data);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[Desktop Entry]\nExec=/usr/bin/chatkord\n").unwrap();
        keep_in(&data, Path::new("/home/x/Chatkord.AppImage")).unwrap();
        remove_in(&data).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[Desktop Entry]\nExec=/usr/bin/chatkord\n");
    }

    #[test]
    fn the_icon_s_count_is_the_badge_s_and_a_dot_shows_none() {
        let badge = |shown| screens::Badge { shown, stale: false };
        assert_eq!(unity::app_uri(), format!("application://{}.desktop", channel::THIS.id));
        let counted = unity::properties(badge(screens::BadgeShown::Count(3)).count());
        assert_eq!((&counted["count"], &counted["count-visible"]), (&Value::from(3i64), &Value::from(true)));
        let dot = unity::properties(badge(screens::BadgeShown::Dot).count());
        assert_eq!(dot["count-visible"], Value::from(false));
    }

    #[test]
    fn every_icon_the_window_has_is_kept() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../shell/assets/icon");
        let mut sides: Vec<u32> = std::fs::read_dir(folder)
            .unwrap()
            .filter_map(|file| {
                let name = file.unwrap().file_name().into_string().unwrap();
                name.strip_prefix("chatkord-")?.strip_suffix(".png")?.parse().ok()
            })
            .collect();
        sides.sort_unstable();
        assert_eq!(sides, ICONS.map(|(side, _)| side));
    }
}
