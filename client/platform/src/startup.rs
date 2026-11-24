//! How Chatkord starts with this device and what closing its window does. Opening at sign-in is the system's own
//! entry, read and written here and kept nowhere else: on Linux the desktop's autostart entry
//! (`<config>/autostart/chatkord.desktop`, the XDG autostart rule), on Windows the user's `Run` value, as Discord's
//! and Slack's are, read with Task Manager's switch for it. Where a start at sign-in opens and what a close does are
//! kept in the kept file's `startup` section.

use crate::state::change_kept;
#[cfg(any(target_os = "linux", windows))]
use crate::state::read_kept;
use crate::{Platform, Unavailable};
#[cfg(any(target_os = "linux", windows))]
use choices::AtSignIn;
use choices::{CloseTo, StartsIn, Startup};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

const FILE: &str = "startup";

/// What a start at sign-in carries, so it is told from a start by a person.
pub const AUTOSTART_ARGUMENT: &str = "--autostart";

/// The desktop entry the package installs, whose keys an autostart entry repeats with its own command.
#[cfg(target_os = "linux")]
const ENTRY: &str = include_str!("../../shell/assets/chatkord.desktop");

/// What says a desktop entry is an AppImage's, which follows its file: the launcher's entry and the autostart entry
/// both carry it.
#[cfg(target_os = "linux")]
pub(crate) const MARK: &str = "X-Chatkord-AppImage=true";

/// The choices kept: `{"starts_in":"tray","close_to":"quit"}`, each absent at its default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Kept {
    #[serde(default, with = "starts_word", skip_serializing_if = "in_window")]
    starts_in: StartsIn,
    #[serde(default, with = "close_word", skip_serializing_if = "to_tray")]
    close_to: CloseTo,
}

impl crate::kept::Section for Kept {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

fn in_window(starts_in: &StartsIn) -> bool {
    *starts_in == StartsIn::Window
}

fn to_tray(close_to: &CloseTo) -> bool {
    *close_to == CloseTo::Tray
}

/// A word in the file, the default for any other.
macro_rules! word {
    ($module:ident, $type:ty, $default:expr, $other:expr, $word:literal) => {
        mod $module {
            use super::*;
            use serde::{Deserializer, Serializer};

            pub(super) fn serialize<S: Serializer>(value: &$type, into: S) -> Result<S::Ok, S::Error> {
                into.serialize_str(if *value == $other { $word } else { "" })
            }

            pub(super) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<$type, D::Error> {
                let word = serde_json::Value::deserialize(from)?;
                Ok(if word.as_str() == Some($word) { $other } else { $default })
            }
        }
    };
}

word!(starts_word, StartsIn, StartsIn::Window, StartsIn::Tray, "tray");
word!(close_word, CloseTo, CloseTo::Tray, CloseTo::Quit, "quit");

/// How Chatkord starts and closes here; none where this system offers no choice yet.
pub async fn startup(platform: &Platform) -> Option<Startup> {
    #[cfg(target_os = "linux")]
    {
        let kept = read_kept::<Kept>(platform).await;
        let at_sign_in = match platform.config().map(Path::to_path_buf) {
            Some(config) => crate::blocking(move || Ok(at_sign_in(&config))).await.unwrap_or_default(),
            None => AtSignIn::Not,
        };
        Some(Startup { at_sign_in, starts_in: kept.starts_in, close_to: kept.close_to })
    }
    #[cfg(windows)]
    {
        let kept = read_kept::<Kept>(platform).await;
        let at_sign_in = crate::blocking(|| Ok(run_key::at_sign_in(&run_key::user()))).await.unwrap_or(AtSignIn::Not);
        Some(Startup { at_sign_in, starts_in: kept.starts_in, close_to: kept.close_to })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = platform;
        None
    }
}

/// Keeps `startup`: the autostart entry written for this executable or removed, and the rest in the file.
pub async fn keep(platform: &Platform, startup: Startup) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        let config = platform.config().ok_or(Unavailable)?.to_path_buf();
        let program = program(std::env::var_os("APPIMAGE"), std::env::current_exe()).ok_or(Unavailable)?;
        crate::blocking(move || set_at_sign_in(&config, startup.at_sign_in, &program)).await?;
    }
    #[cfg(windows)]
    {
        let program = std::env::current_exe().map_err(|_| Unavailable)?;
        crate::blocking(move || run_key::set_at_sign_in(&run_key::user(), startup.at_sign_in, &program)).await?;
    }
    change_kept::<Kept, _>(platform, move |kept| {
        *kept = Kept { starts_in: startup.starts_in, close_to: startup.close_to };
    })
    .await
}

/// What starts this program again.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
enum Program {
    /// The AppImage's own file, which its runtime names in `appimage` (the executable is inside a mount that ends
    /// with it).
    Image(PathBuf),
    /// The executable, for a program not run from an AppImage.
    Executable(PathBuf),
}

#[cfg(target_os = "linux")]
fn program(appimage: Option<std::ffi::OsString>, exe: std::io::Result<PathBuf>) -> Option<Program> {
    match appimage.map(PathBuf::from).filter(|image| image.is_absolute()) {
        Some(image) => Some(Program::Image(image)),
        None => exe.ok().map(Program::Executable),
    }
}

#[cfg(target_os = "linux")]
fn entry_path(config: &Path) -> PathBuf {
    config.join("autostart").join("chatkord.desktop")
}

/// Whether the autostart entry is there and not hidden, as the desktop reads it.
#[cfg(target_os = "linux")]
fn at_sign_in(config: &Path) -> AtSignIn {
    match std::fs::read_to_string(entry_path(config)) {
        Ok(entry) if !hidden(&entry) => AtSignIn::Opens,
        Ok(_) | Err(_) => AtSignIn::Not,
    }
}

/// Whether `entry` is turned off in the desktop's own settings.
#[cfg(target_os = "linux")]
fn hidden(entry: &str) -> bool {
    entry.lines().any(|line| line.trim() == "Hidden=true")
}

/// The package's desktop entry with `program` as its command, given `argument` if any, quoted as the desktop entry rule
/// asks, and as `TryExec`, a plain string value, so a launcher hides the entry once the file is gone.
#[cfg(target_os = "linux")]
pub(crate) fn entry(program: &Path, argument: Option<&str>) -> String {
    let quoted: String = program
        .to_string_lossy()
        .chars()
        .flat_map(|c| matches!(c, '"' | '`' | '$' | '\\').then_some('\\').into_iter().chain(std::iter::once(c)))
        .collect();
    let tried: String = program
        .to_string_lossy()
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\r' => "\\r".to_owned(),
            c => c.to_string(),
        })
        .collect();
    let command = match argument {
        Some(argument) => format!("Exec=\"{quoted}\" {argument}"),
        None => format!("Exec=\"{quoted}\""),
    };
    ENTRY
        .lines()
        .flat_map(|line| {
            if line.starts_with("Exec=") {
                vec![command.clone(), format!("TryExec={tried}")]
            } else {
                vec![line.to_owned()]
            }
        })
        .flat_map(|line| [line, "\n".to_owned()])
        .collect()
}

/// The autostart entry for `program`; an AppImage's is marked, so a start of it can move the entry with the file.
#[cfg(target_os = "linux")]
fn autostart_entry(program: &Program) -> String {
    let (Program::Image(path) | Program::Executable(path)) = program;
    let entry = entry(path, Some(AUTOSTART_ARGUMENT)) + "X-GNOME-Autostart-enabled=true\n";
    match program {
        Program::Image(_) => entry + MARK + "\n",
        Program::Executable(_) => entry,
    }
}

/// Writes `bytes` beside `path` and renames them over it, its folder made first: a desktop that caches its menu
/// (Plasma's ksycoca) rereads an entry when its folder changes, which a rename does and a write in place does not.
#[cfg(target_os = "linux")]
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<(), Unavailable> {
    let folder = path.parent().ok_or(Unavailable)?;
    std::fs::create_dir_all(folder).map_err(|_| Unavailable)?;
    let name = path.file_name().ok_or(Unavailable)?.to_string_lossy();
    let partial = folder.join(format!(".{name}.{}{}", std::process::id(), crate::partial::SUFFIX));
    let written = std::fs::write(&partial, bytes).and_then(|()| std::fs::rename(&partial, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written.map_err(|_| Unavailable)
}

/// Takes away the partial files a dead run left in the desktop's folders for the entries this program keeps.
pub(crate) fn sweep(
    platform: &Platform,
    running: impl Fn(u32) -> bool + Copy,
    started: std::time::SystemTime,
) -> usize {
    #[cfg(target_os = "linux")]
    {
        let in_each = |folders: Vec<PathBuf>| -> usize {
            folders.iter().map(|folder| crate::partial::sweep(folder, crate::partial::SUFFIX, running, started)).sum()
        };
        let mut swept = platform.config().map_or(0, |config| in_each(vec![config.join("autostart")]));
        swept += crate::launcher::data_folders().map_or(0, in_each);
        swept
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (platform, running, started);
        0
    }
}

/// Moves a marked autostart entry to `image`, where the file is now. An entry that is absent, another's or turned off
/// in the desktop's settings stays as it is: this never turns a start at sign-in on.
#[cfg(target_os = "linux")]
pub(crate) fn follow(config: &Path, image: &Path) -> Result<(), Unavailable> {
    let path = entry_path(config);
    let entry = match std::fs::read_to_string(&path) {
        Ok(entry) => entry,
        Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Unavailable),
    };
    let ours = entry.lines().any(|line| line == MARK) && !hidden(&entry);
    let wanted = autostart_entry(&Program::Image(image.to_path_buf()));
    if !ours || entry == wanted {
        return Ok(());
    }
    replace(&path, wanted.as_bytes())
}

#[cfg(target_os = "linux")]
fn set_at_sign_in(config: &Path, at_sign_in: AtSignIn, program: &Program) -> Result<(), Unavailable> {
    let path = entry_path(config);
    match at_sign_in {
        AtSignIn::Opens => replace(&path, autostart_entry(program).as_bytes()),
        AtSignIn::Not => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(Unavailable),
        },
    }
}

/// Takes the entry that opens Chatkord at sign-in away, for an uninstall.
pub(crate) fn forget(platform: &Platform) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        let config = platform.config().ok_or(Unavailable)?;
        set_at_sign_in(config, AtSignIn::Not, &Program::Executable(PathBuf::new()))
    }
    #[cfg(windows)]
    {
        let _ = platform;
        run_key::set_at_sign_in(&run_key::user(), AtSignIn::Not, std::path::Path::new(""))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = platform;
        Ok(())
    }
}

/// The user's `Run` value, which Windows starts at sign-in, and Task Manager's switch for it, which leaves the value
/// and marks it off.
#[cfg(windows)]
mod run_key {
    use super::{AUTOSTART_ARGUMENT, AtSignIn, Unavailable};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW,
    };

    /// Where the entry and its switch are, under the user's own hive: Windows' keys, or a test's.
    pub(super) struct Keys {
        pub(super) run: &'static str,
        pub(super) approved: &'static str,
        /// The value's name: the product's, as the setup removes it.
        pub(super) name: String,
    }

    /// Windows' own keys, the value named as the setup names the product (the catalog's window title).
    pub(super) fn user() -> Keys {
        Keys {
            run: r"Software\Microsoft\Windows\CurrentVersion\Run",
            approved: r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            name: words::Words::english().phrase(words::Phrase::WindowTitle).into_owned(),
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// Whether Windows opens Chatkord at sign-in: the value there, and not switched off in Task Manager, which marks
    /// it with an odd first byte (3; 2 is on).
    pub(super) fn at_sign_in(keys: &Keys) -> AtSignIn {
        let (run, approved, name) = (wide(keys.run), wide(keys.approved), wide(&keys.name));
        let mut size = 0u32;
        // SAFETY: NUL-terminated names alive across the call; no buffer, so only the size is written.
        let there = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                run.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut size,
            )
        } == ERROR_SUCCESS;
        let mut switch = [0u8; 12];
        let mut size = u32::try_from(switch.len()).unwrap_or(0);
        // SAFETY: as above, and a buffer of the size stated.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                approved.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_BINARY,
                std::ptr::null_mut(),
                switch.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        let off = read == ERROR_SUCCESS && size > 0 && switch[0] & 1 == 1;
        if there && !off { AtSignIn::Opens } else { AtSignIn::Not }
    }

    /// Writes the value starting `program` with [`AUTOSTART_ARGUMENT`], or removes it; either way Task Manager's mark
    /// goes, so the choice made here is the one Windows follows.
    pub(super) fn set_at_sign_in(keys: &Keys, at_sign_in: AtSignIn, program: &Path) -> Result<(), Unavailable> {
        remove(keys.approved, &keys.name)?;
        match at_sign_in {
            AtSignIn::Opens => {
                let command = format!("\"{}\" {AUTOSTART_ARGUMENT}", program.display());
                write(keys.run, &keys.name, &command)
            }
            AtSignIn::Not => remove(keys.run, &keys.name),
        }
    }

    fn write(key: &str, name: &str, text: &str) -> Result<(), Unavailable> {
        let (key, name, text) = (wide(key), wide(name), wide(text));
        let mut opened: HKEY = std::ptr::null_mut();
        // SAFETY: a NUL-terminated name alive across the call and a valid out pointer.
        let made = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &raw mut opened,
                std::ptr::null_mut(),
            )
        };
        if made != ERROR_SUCCESS {
            return Err(Unavailable);
        }
        let bytes = u32::try_from(text.len() * 2).map_err(|_| Unavailable)?;
        // SAFETY: the key opened above; `text` is UTF-16 with its NUL, `bytes` long.
        let set = unsafe { RegSetValueExW(opened, name.as_ptr(), 0, REG_SZ, text.as_ptr().cast(), bytes) };
        // SAFETY: the key opened above, closed once.
        unsafe { RegCloseKey(opened) };
        if set == ERROR_SUCCESS { Ok(()) } else { Err(Unavailable) }
    }

    fn remove(key: &str, name: &str) -> Result<(), Unavailable> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: NUL-terminated names alive across the call.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            _ => Err(Unavailable),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::System::Registry::{REG_BINARY, RegDeleteTreeW};

        /// Keys of this test's own, so a test that fails never leaves an entry Windows would start at sign-in.
        fn test() -> Keys {
            Keys {
                run: r"Software\Chatkord-test-startup\Run",
                approved: r"Software\Chatkord-test-startup\Approved",
                name: user().name,
            }
        }

        /// Task Manager's switch, as it writes it.
        fn switched(on: bool) {
            let keys = test();
            let (key, name) = (wide(keys.approved), wide(&keys.name));
            let mut opened: HKEY = std::ptr::null_mut();
            // SAFETY: as in `write`.
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    std::ptr::null(),
                    &raw mut opened,
                    std::ptr::null_mut(),
                )
            };
            let mark: [u8; 12] = [if on { 2 } else { 3 }, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            // SAFETY: the key opened above and a buffer of the size stated.
            unsafe { RegSetValueExW(opened, name.as_ptr(), 0, REG_BINARY, mark.as_ptr(), 12) };
            // SAFETY: as above, closed once.
            unsafe { RegCloseKey(opened) };
        }

        #[test]
        fn the_run_value_is_the_setting_written_and_removed_and_task_managers_switch_turns_it_off() {
            let program = Path::new(r"C:\Program Files\Chat kord\Chatkord.exe");
            let keys = test();
            assert_eq!(keys.name, "Chatkord", "named as the setup names the product");
            assert_eq!(at_sign_in(&keys), AtSignIn::Not, "none there");
            set_at_sign_in(&keys, AtSignIn::Opens, program).unwrap();
            assert_eq!(at_sign_in(&keys), AtSignIn::Opens);
            switched(false);
            assert_eq!(at_sign_in(&keys), AtSignIn::Not, "switched off in Task Manager, the value left");
            switched(true);
            assert_eq!(at_sign_in(&keys), AtSignIn::Opens, "switched on again");
            switched(false);
            set_at_sign_in(&keys, AtSignIn::Opens, program).unwrap();
            assert_eq!(at_sign_in(&keys), AtSignIn::Opens, "turned on here, Task Manager's mark goes");
            set_at_sign_in(&keys, AtSignIn::Not, program).unwrap();
            assert_eq!(at_sign_in(&keys), AtSignIn::Not);
            set_at_sign_in(&keys, AtSignIn::Not, program).expect("none there is removed already");
            let tree = wide(r"Software\Chatkord-test-startup");
            // SAFETY: a NUL-terminated name of this test's own key.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, tree.as_ptr()) };
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::state::{kept_text_in, raw_kept_in, read_kept_in, write_kept_in};

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-startup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn the_autostart_entry_is_the_setting_written_and_removed_and_a_hidden_one_is_off() {
        let config = scratch("entry");
        let program = Program::Executable(PathBuf::from("/opt/chat kord/chatkord"));
        assert_eq!(at_sign_in(&config), AtSignIn::Not, "none there");
        set_at_sign_in(&config, AtSignIn::Opens, &program).unwrap();
        assert_eq!(at_sign_in(&config), AtSignIn::Opens);
        let entry = std::fs::read_to_string(entry_path(&config)).unwrap();
        assert!(entry.contains("Exec=\"/opt/chat kord/chatkord\" --autostart\n"), "{entry}");
        assert!(entry.contains("Name=Chatkord\n") && entry.contains("Icon=chatkord\n"), "the package's keys: {entry}");
        std::fs::write(entry_path(&config), entry + "Hidden=true\n").unwrap();
        assert_eq!(at_sign_in(&config), AtSignIn::Not, "turned off in the desktop's own settings");
        set_at_sign_in(&config, AtSignIn::Not, &program).unwrap();
        assert!(!entry_path(&config).exists());
        set_at_sign_in(&config, AtSignIn::Not, &program).expect("none there is removed already");
    }

    #[test]
    fn an_appimage_starts_again_from_its_own_file_not_its_mount() {
        let exe = || Ok(PathBuf::from("/tmp/.mount_ChatkoAbc123/opt/chatkord/chatkord"));
        let image = Some(std::ffi::OsString::from("/home/x/Applications/Chatkord.AppImage"));
        assert_eq!(
            program(image, exe()),
            Some(Program::Image(PathBuf::from("/home/x/Applications/Chatkord.AppImage")))
        );
        assert_eq!(
            program(None, exe()),
            exe().ok().map(Program::Executable),
            "a program not in an AppImage is its executable"
        );
        let relative = Some(std::ffi::OsString::from("Chatkord.AppImage"));
        assert_eq!(
            program(relative, exe()),
            exe().ok().map(Program::Executable),
            "a path that is not whole is not one to start from"
        );
    }

    #[test]
    fn a_command_with_quotes_or_a_dollar_is_escaped_as_the_entry_rule_asks() {
        let entry = autostart_entry(&Program::Executable(PathBuf::from("/home/x/$\"odd\"/chatkord")));
        assert!(entry.contains(r#"Exec="/home/x/\$\"odd\"/chatkord" --autostart"#), "{entry}");
    }

    #[test]
    fn the_entry_names_its_file_as_a_string_for_the_launcher_to_test() {
        let entry = entry(Path::new("/home/x/a\\b/chatkord"), None);
        assert!(entry.contains("TryExec=/home/x/a\\\\b/chatkord\n"), "{entry}");
        let image = autostart_entry(&Program::Image(PathBuf::from("/home/x/C.AppImage")));
        assert!(image.lines().any(|line| line == MARK) && image.contains("TryExec=/home/x/C.AppImage\n"), "{image}");
        let exe = autostart_entry(&Program::Executable(PathBuf::from("/opt/chatkord")));
        assert!(!exe.lines().any(|line| line == MARK), "{exe}");
    }

    #[test]
    fn a_marked_autostart_entry_follows_its_file_and_no_other_is_touched() {
        let config = scratch("follow");
        let (old, new) = (Path::new("/home/x/Old.AppImage"), Path::new("/home/x/Applications/New.AppImage"));
        follow(&config, new).unwrap();
        assert!(!entry_path(&config).exists(), "none there stays none");
        set_at_sign_in(&config, AtSignIn::Opens, &Program::Image(old.to_path_buf())).unwrap();
        follow(&config, new).unwrap();
        let moved = std::fs::read_to_string(entry_path(&config)).unwrap();
        assert!(moved.contains("Exec=\"/home/x/Applications/New.AppImage\" --autostart\n"), "{moved}");
        assert!(moved.contains("TryExec=/home/x/Applications/New.AppImage\n"), "{moved}");
        let unmarked = autostart_entry(&Program::Executable(PathBuf::from("/opt/chatkord")));
        let hidden = autostart_entry(&Program::Image(old.to_path_buf())) + "Hidden=true\n";
        for kept in [unmarked, hidden] {
            std::fs::write(entry_path(&config), &kept).unwrap();
            follow(&config, new).unwrap();
            assert_eq!(std::fs::read_to_string(entry_path(&config)).unwrap(), kept);
        }
    }

    #[test]
    fn an_autostart_entry_that_follows_its_file_is_a_new_file_its_folder_shows() {
        use std::os::unix::fs::MetadataExt;
        let config = scratch("renamed");
        set_at_sign_in(&config, AtSignIn::Opens, &Program::Image(PathBuf::from("/home/x/Old.AppImage"))).unwrap();
        let was = std::fs::metadata(entry_path(&config)).unwrap().ino();
        follow(&config, Path::new("/home/x/Applications/New.AppImage")).unwrap();
        assert_ne!(std::fs::metadata(entry_path(&config)).unwrap().ino(), was, "rewritten in place");
    }

    #[test]
    fn the_choices_read_back_as_written_and_the_defaults_are_written_as_nothing() {
        let folder = scratch("kept");
        write_kept_in(&folder, &Kept::default()).unwrap();
        assert_eq!(kept_text_in::<Kept>(&folder), "{}");
        let kept = Kept { starts_in: StartsIn::Tray, close_to: CloseTo::Quit };
        write_kept_in(&folder, &kept).unwrap();
        assert_eq!(read_kept_in::<Kept>(&folder), kept);
        raw_kept_in::<Kept>(&folder, r#"{"starts_in":"moon","close_to":7}"#).unwrap();
        assert_eq!(read_kept_in::<Kept>(&folder), Kept::default(), "any other word is the default");
    }
}
