//! Who owns the window a share picked, on KDE. The portal names the window's app only in its stream's name
//! (`kwin-screencast-<app id>`), and only KWin knows which processes show windows of that app, so a script asks it.
//! Anywhere else, or when anything fails, nobody is named: never a guess.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use zbus::blocking::Connection;

use crate::pick::Owner;

/// What KWin calls a window's stream; the rest is the window's app id.
const STREAM_PREFIX: &str = "kwin-screencast-";
/// The longest app id taken: a D-Bus name's bound, which a desktop file's id follows.
const APP_ID_MAX: usize = 255;
/// How long KWin is given to answer; a share starts after it either way.
const ANSWER: Duration = Duration::from_secs(1);
/// Where the script calls back, on this process's own connection.
const PATH: &str = "/org/chatkord/Owner";

/// Who owns the window whose stream is named `media_name`; blocks for up to a second on KWin's answer.
pub fn owner(media_name: &str) -> Option<Owner> {
    let app_id = app_id(media_name)?;
    let pids = ask_kwin(app_id).ok().filter(|pids| !pids.is_empty())?;
    Some(Owner { app_id: app_id.to_owned(), pids, name: shown_name(app_id) })
}

/// The app id in a KWin stream's name, when it is one and holds only what an app id may: anything else never reaches
/// the script.
fn app_id(media_name: &str) -> Option<&str> {
    let id = media_name.strip_prefix(STREAM_PREFIX)?;
    let allowed = |byte: u8| byte.is_ascii_alphanumeric() || b"._-".contains(&byte);
    ((1..=APP_ID_MAX).contains(&id.len()) && id.bytes().all(allowed)).then_some(id)
}

/// Why KWin gave no list.
#[derive(Debug)]
pub enum NoAnswer {
    /// No session bus, or KWin is not on it.
    Bus,
    /// The script could not be written or loaded.
    Script,
    /// KWin did not call back in time.
    Late,
}

impl From<zbus::Error> for NoAnswer {
    fn from(_: zbus::Error) -> Self {
        NoAnswer::Bus
    }
}

/// The receiving end of the script's one call.
struct Told(mpsc::Sender<String>);

#[zbus::interface(name = "org.chatkord.Owner")]
impl Told {
    fn pids(&self, pids: String) {
        // The asker gave up waiting: nobody is left to tell.
        let _ = self.0.send(pids);
    }
}

/// The processes showing windows of `app_id`, as KWin lists them; an empty list is an answer too.
#[doc(hidden)]
pub fn ask_kwin(asked: &str) -> Result<Vec<u32>, NoAnswer> {
    static ASKED: AtomicU32 = AtomicU32::new(0);
    let stream = format!("{STREAM_PREFIX}{asked}");
    let app_id = app_id(&stream).ok_or(NoAnswer::Script)?;
    let (tell, told) = mpsc::channel();
    let connection = zbus::blocking::connection::Builder::session()?.serve_at(PATH, Told(tell))?.build()?;
    let service = connection.unique_name().ok_or(NoAnswer::Bus)?.to_string();
    let plugin = format!("chatkord-owner-{}-{}", std::process::id(), ASKED.fetch_add(1, Ordering::Relaxed));
    let file = Script::write(&plugin, &script(app_id, &service)).ok_or(NoAnswer::Script)?;
    let answer =
        run(&connection, &file.0, &plugin).and_then(|()| told.recv_timeout(ANSWER).map_err(|_| NoAnswer::Late));
    // Unloaded whatever came of it, so nothing of ours stays in KWin.
    let _ = connection.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "unloadScript",
        &plugin,
    );
    Ok(answer?.split(',').filter_map(|pid| pid.trim().parse().ok()).collect())
}

/// Loads the script at `file` as `plugin` and runs it.
fn run(connection: &Connection, file: &Path, plugin: &str) -> Result<(), NoAnswer> {
    let path = file.to_str().ok_or(NoAnswer::Script)?;
    let loaded = connection.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "loadScript",
        &(path, plugin),
    )?;
    let id: i32 = loaded.body().deserialize()?;
    // KWin answers a script it could not load with a negative id.
    let id = u32::try_from(id).map_err(|_| NoAnswer::Script)?;
    let script = format!("/Scripting/Script{id}");
    connection.call_method(Some("org.kde.KWin"), script.as_str(), Some("org.kde.kwin.Script"), "run", &())?;
    Ok(())
}

/// The script: the processes of every window of `app_id`, called back to `service` as a comma-separated list. Both are
/// checked plain words, so nothing in them is read as code.
fn script(app_id: &str, service: &str) -> String {
    format!(
        "const pids = workspace.windowList().filter(w => w.desktopFileName === \"{app_id}\" || w.resourceClass === \
         \"{app_id}\").map(w => w.pid);\ncallDBus(\"{service}\", \"{PATH}\", \"org.chatkord.Owner\", \"Pids\", \
         pids.join(\",\"));\n"
    )
}

/// The script's file, readable by this user alone, removed when dropped.
struct Script(PathBuf);

impl Script {
    fn write(plugin: &str, text: &str) -> Option<Self> {
        let folder = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
        let path = folder.join(format!("{plugin}.js"));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).ok()?;
        let written = Script(path);
        file.write_all(text.as_bytes()).ok()?;
        Some(written)
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The `Name=` of `<app_id>.desktop` in the first data folder that has it, else the id itself.
fn shown_name(app_id: &str) -> String {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let shared = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    home.into_iter()
        .chain(shared.split(':').filter(|dir| !dir.is_empty()).map(PathBuf::from))
        .find_map(|dir| std::fs::read_to_string(dir.join("applications").join(format!("{app_id}.desktop"))).ok())
        .and_then(|entry| desktop_name(&entry))
        .unwrap_or_else(|| app_id.to_owned())
}

/// The plain `Name=` of a desktop file's main section.
fn desktop_name(entry: &str) -> Option<String> {
    let mut in_main = false;
    for line in entry.lines().map(str::trim) {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
        } else if in_main && let Some(name) = line.strip_prefix("Name=") {
            return Some(name.trim().to_owned()).filter(|name| !name.is_empty());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_kwin_stream_with_a_plain_app_id_names_an_app() {
        assert_eq!(app_id("kwin-screencast-brave-browser"), Some("brave-browser"));
        assert_eq!(app_id("kwin-screencast-org.kde.konsole"), Some("org.kde.konsole"));
        for refused in [
            "kwin-screencast-",
            "kwin-screencast-a\"b",
            "kwin-screencast-a b",
            "kwin-screencast-a\\b",
            "kwin-screencast-a);x(",
            "gnome-shell-screencast",
            "xdpw-stream",
        ] {
            assert_eq!(app_id(refused), None, "{refused}");
        }
        assert_eq!(app_id(&format!("kwin-screencast-{}", "a".repeat(APP_ID_MAX + 1))), None);
    }

    #[test]
    fn the_script_asks_for_the_apps_windows_and_calls_back_with_their_processes() {
        let text = script("brave-browser", ":1.42");
        assert!(text.contains("w.desktopFileName === \"brave-browser\" || w.resourceClass === \"brave-browser\""));
        assert!(text.contains("callDBus(\":1.42\", \"/org/chatkord/Owner\", \"org.chatkord.Owner\", \"Pids\""));
    }

    #[test]
    fn a_desktop_files_name_is_its_main_sections_plain_name() {
        let entry = "[Desktop Entry]\nGenericName=Web Browser\nName[de]=Brave\nName=Brave Web Browser\n\n\
                     [Desktop Action new-window]\nName=New Window\n";
        assert_eq!(desktop_name(entry), Some("Brave Web Browser".to_owned()));
        assert_eq!(desktop_name("[Desktop Action x]\nName=Other\n"), None);
    }
}
