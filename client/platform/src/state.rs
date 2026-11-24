//! What a run leaves for the next: each server this device was added to, in order, with its name, colour, picture,
//! account and the key trusted for it, and which was open. Never a token, password or invite. A program that names no
//! folder remembers nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::kept::{Kept, Section};
use crate::{Platform, Unavailable, blocking};
use proto::files::FileId;
use proto::pin::KeyPin;
use proto::rules::Colour;
use terms::{ServerIdentity, ServerUrl};

/// One server this device was added to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The address that greeted when it was added.
    pub server: ServerUrl,
    /// The name it greeted with, if any.
    pub name: Option<String>,
    /// The colour of its picture it last said, if any.
    pub colour: Option<Colour>,
    /// Its picture's file, kept by `cache` once fetched.
    pub picture: Option<FileId>,
    /// The account signed in at it: the name its token is filed under.
    pub identity: Option<String>,
    /// The key a person trusted for it, for a certificate no authority vouches for; none while the system's
    /// verifier vouches for it.
    pub pin: Option<KeyPin>,
}

/// `<config>/chatkord/`, where [`crate::kept::FILE`] is.
pub(crate) const FOLDER: &str = "chatkord";

/// The rows the servers' section holds, as text. A row whose address does not parse is dropped on reading.
#[derive(Default, Deserialize, Serialize)]
struct File {
    servers: Vec<Stored>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open: Option<String>,
}

impl crate::kept::Section for File {
    const NAME: &'static str = "servers";
    const FORMAT: u32 = 1;
}

/// What the file says, read.
#[derive(Default)]
struct Remembered {
    rows: Vec<Row>,
    open: Option<ServerUrl>,
}

#[derive(Deserialize, Serialize)]
struct Stored {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// `#RRGGBB`; one of another shape is no colour, and leaves the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colour: Option<String>,
    /// A file id; one of another shape is no picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<String>,
    /// A pin of another shape is no pin: the server is asked about again.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    pin: Option<KeyPin>,
}

/// The system's configuration directory: `XDG_CONFIG_HOME`, else `$HOME/.config` on Linux; `APPDATA`, else
/// `LOCALAPPDATA` on Windows. Only a path stated from the root counts.
#[cfg(target_os = "linux")]
pub(crate) fn directory() -> Option<PathBuf> {
    chosen(from_env("XDG_CONFIG_HOME"), from_env("HOME").map(|home| home.join(".config")))
}

#[cfg(target_os = "windows")]
pub(crate) fn directory() -> Option<PathBuf> {
    chosen(from_env("APPDATA"), from_env("LOCALAPPDATA"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn directory() -> Option<PathBuf> {
    None
}

fn chosen(stated: Option<PathBuf>, other: Option<PathBuf>) -> Option<PathBuf> {
    stated.or(other)
}

fn from_root(stated: &str) -> Option<PathBuf> {
    Path::new(stated).is_absolute().then(|| Path::new(stated).to_path_buf())
}

pub(crate) fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var(name).ok().and_then(|stated| from_root(&stated))
}

/// This program's folder taken away, with all a run left in it; none there is done already.
pub(crate) fn remove(platform: &Platform) -> Result<(), Unavailable> {
    platform.folder().map_or(Ok(()), |here| crate::removed(&here))
}

/// Takes away the partial files a dead run left beside the kept file.
pub(crate) fn sweep(platform: &Platform, running: impl Fn(u32) -> bool, started: std::time::SystemTime) -> usize {
    platform.folder().map_or(0, |here| crate::partial::sweep(&here, crate::partial::SUFFIX, running, started))
}

/// Each account remembered: its server and the name its token is filed under. A file that is there but cannot be
/// read is a refusal, so what takes the accounts away never takes an unread file for none.
pub(crate) fn accounts(platform: &Platform) -> Result<Vec<(ServerUrl, String)>, Unavailable> {
    let Some(here) = platform.folder() else { return Ok(Vec::new()) };
    let rows = Remembered::from(Kept::in_folder(here).held::<File>()?).rows;
    Ok(rows.into_iter().filter_map(|row| Some((row.server, row.identity?))).collect())
}

/// The servers remembered, in the order they were added; none when nothing was named, written or readable.
pub async fn servers(platform: &Platform) -> Vec<Row> {
    read(platform, |remembered| remembered.rows).await
}

/// The server that was open when the window last chose one, if it is still remembered.
pub async fn opened(platform: &Platform) -> Option<ServerUrl> {
    read(platform, |remembered| remembered.open).await
}

/// A server that greeted, or said it changed, with the name, colour and picture it gave, keeping the account
/// remembered at it; a new one goes last. A picture it replaced leaves the cache.
pub async fn remember_server(platform: &Platform, server: ServerUrl, said: ServerIdentity) -> Result<(), Unavailable> {
    if let Some(old) = change(platform, move |remembered| remembered.greeted(server, said)).await? {
        crate::cache::forget(platform, old).await;
    }
    Ok(())
}

/// The account signed in at a server, keeping the name it greeted with; a new one goes last.
pub async fn remember_account(platform: &Platform, server: ServerUrl, identity: String) -> Result<(), Unavailable> {
    change(platform, move |remembered| remembered.signed_in(server, identity)).await
}

/// The server the window shows, so the next run opens on it.
pub async fn remember_open(platform: &Platform, server: ServerUrl) -> Result<(), Unavailable> {
    change(platform, move |remembered| remembered.open = Some(server)).await
}

/// No account is signed in at a server any more: its row stays.
pub async fn signed_out(platform: &Platform, server: ServerUrl) -> Result<(), Unavailable> {
    change(platform, move |remembered| remembered.signed_out(&server)).await
}

/// A server taken off this device: its row with the key trusted for it, and the open mark if it was the open one.
pub async fn forget_server(platform: &Platform, server: ServerUrl) -> Result<(), Unavailable> {
    change(platform, move |remembered| remembered.forgot(&server)).await
}

/// The key a person trusted for a server, none when none is kept or nothing was named.
pub async fn trusted_key(platform: &Platform, server: ServerUrl) -> Option<KeyPin> {
    read(platform, move |remembered| {
        remembered.rows.into_iter().find(|row| row.server == server).and_then(|row| row.pin)
    })
    .await
}

/// The key a person trusts for a server, in place of any kept before; a server not remembered is remembered with it.
pub async fn trust_key(platform: &Platform, server: ServerUrl, pin: KeyPin) -> Result<(), Unavailable> {
    change(platform, move |remembered| row_for(&mut remembered.rows, server).pin = Some(pin)).await
}

/// No key is trusted for a server any more: the system's verifier vouches for it now.
pub async fn drop_key(platform: &Platform, server: ServerUrl) -> Result<(), Unavailable> {
    change(platform, move |remembered| remembered.dropped_key(&server)).await
}

impl Remembered {
    /// The picture the greeting replaced, if any.
    fn greeted(&mut self, server: ServerUrl, said: ServerIdentity) -> Option<FileId> {
        let row = row_for(&mut self.rows, server);
        let replaced = row.picture.filter(|was| Some(*was) != said.picture);
        (row.name, row.colour, row.picture) = (said.name, said.colour, said.picture);
        replaced
    }

    fn signed_in(&mut self, server: ServerUrl, identity: String) {
        row_for(&mut self.rows, server).identity = Some(identity);
    }

    fn signed_out(&mut self, server: &ServerUrl) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.server == *server) {
            row.identity = None;
        }
    }

    fn dropped_key(&mut self, server: &ServerUrl) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.server == *server) {
            row.pin = None;
        }
    }

    fn forgot(&mut self, server: &ServerUrl) {
        self.rows.retain(|row| row.server != *server);
        self.open = self.open.take().filter(|open| open != server);
    }
}

fn row_for(rows: &mut Vec<Row>, server: ServerUrl) -> &mut Row {
    let at = match rows.iter().position(|row| row.server == server) {
        Some(at) => at,
        None => {
            rows.push(Row { server, name: None, colour: None, picture: None, identity: None, pin: None });
            rows.len() - 1
        }
    };
    &mut rows[at]
}

async fn read<T: Default + Send + 'static>(
    platform: &Platform,
    part: impl FnOnce(Remembered) -> T + Send + 'static,
) -> T {
    let Some(here) = platform.config().map(Path::to_path_buf) else { return T::default() };
    blocking(move || Ok(part(read_in(&here)))).await.unwrap_or_default()
}

/// Reads the file, edits what it held and writes it back whole; what the edit returned.
async fn change<T: Send + 'static>(
    platform: &Platform,
    edit: impl FnOnce(&mut Remembered) -> T + Send + 'static,
) -> Result<T, Unavailable> {
    let Some(here) = platform.config().map(Path::to_path_buf) else { return Err(Unavailable) };
    blocking(move || change_in(&here, edit)).await
}

fn change_in<T>(folder: &Path, edit: impl FnOnce(&mut Remembered) -> T) -> Result<T, Unavailable> {
    change_kept_in(folder, |file: &mut File| {
        let mut remembered = Remembered::from(std::mem::take(file));
        let made = edit(&mut remembered);
        *file = File::from(&remembered);
        made
    })
}

/// The kept file in this program's folder inside `folder`.
fn kept_in(folder: &Path) -> Kept {
    Kept::in_folder(folder.join(FOLDER))
}

/// A section of the kept file, read; its defaults when it is missing or unreadable, and when no folder was named.
pub(crate) async fn read_kept<S: Section + Send + 'static>(platform: &Platform) -> S {
    let Some(here) = platform.config().map(Path::to_path_buf) else { return S::default() };
    blocking(move || Ok(kept_in(&here).read::<S>())).await.unwrap_or_default()
}

/// Reads a section of the kept file, edits it and writes the file back, off the sockets' tasks.
pub(crate) async fn change_kept<S, M>(
    platform: &Platform,
    edit: impl FnOnce(&mut S) -> M + Send + 'static,
) -> Result<M, Unavailable>
where
    S: Section + Send + 'static,
    M: Send + 'static,
{
    let Some(here) = platform.config().map(Path::to_path_buf) else { return Err(Unavailable) };
    blocking(move || change_kept_in(&here, edit)).await
}

/// The same as [`read_kept`], blocking: for what the window reads before any runtime runs.
pub(crate) fn read_kept_now<S: Section>(platform: &Platform) -> S {
    platform.config().map(|here| kept_in(here).read::<S>()).unwrap_or_default()
}

/// The same as [`change_kept`], blocking: for what the window keeps as it closes, when no runtime may be left.
pub(crate) fn change_kept_now<S: Section, M>(
    platform: &Platform,
    edit: impl FnOnce(&mut S) -> M,
) -> Result<M, Unavailable> {
    let here = platform.config().ok_or(Unavailable)?;
    change_kept_in(here, edit)
}

fn change_kept_in<S: Section, M>(folder: &Path, edit: impl FnOnce(&mut S) -> M) -> Result<M, Unavailable> {
    kept_in(folder).change(edit)
}

#[cfg(test)]
pub(crate) fn read_kept_in<S: Section>(folder: &Path) -> S {
    kept_in(folder).read::<S>()
}

#[cfg(test)]
pub(crate) fn write_kept_in<S: Section + Clone>(folder: &Path, held: &S) -> Result<(), Unavailable> {
    let held = held.clone();
    change_kept_in(folder, move |section: &mut S| *section = held)
}

/// Writes `S`'s section with `settings` in its format, as a person or another build would, the file holding nothing
/// else.
#[cfg(test)]
pub(crate) fn raw_kept_in<S: Section>(folder: &Path, settings: impl AsRef<str>) -> std::io::Result<()> {
    let file = kept_in(folder).file();
    std::fs::create_dir_all(file.parent().unwrap_or(folder))?;
    let (name, format, settings) = (S::NAME, S::FORMAT, settings.as_ref());
    std::fs::write(file, format!("{{\"{name}\":{{\"format\":{format},\"settings\":{settings}}}}}"))
}

/// `text` as the file writes it: compact, each object's keys in order.
#[cfg(test)]
pub(crate) fn json_sorted(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text).map(|value| value.to_string()).unwrap_or_default()
}

/// `S`'s settings as written, compact.
#[cfg(test)]
pub(crate) fn kept_text_in<S: Section>(folder: &Path) -> String {
    let whole: serde_json::Value =
        serde_json::from_slice(&std::fs::read(kept_in(folder).file()).unwrap_or_default()).unwrap_or_default();
    whole.get(S::NAME).and_then(|section| section.get("settings")).map(ToString::to_string).unwrap_or_default()
}

fn read_in(folder: &Path) -> Remembered {
    Remembered::from(kept_in(folder).read::<File>())
}

/// An empty name or identity is none, a row whose address does not parse is no row, and an open mark for a server not
/// in the rows is no mark.
impl From<File> for Remembered {
    fn from(file: File) -> Self {
        let given = |text: Option<String>| text.filter(|one| !one.is_empty());
        let rows: Vec<Row> = file
            .servers
            .into_iter()
            .filter_map(|stored| {
                Some(Row {
                    server: ServerUrl::parse(&stored.url).ok()?,
                    name: given(stored.name),
                    colour: stored.colour.and_then(|colour| Colour::parse(&colour).ok()),
                    picture: stored.picture.and_then(|picture| FileId::parse(&picture).ok()),
                    identity: given(stored.identity),
                    pin: stored.pin,
                })
            })
            .collect();
        let open = file
            .open
            .and_then(|url| ServerUrl::parse(&url).ok())
            .filter(|open| rows.iter().any(|row| row.server == *open));
        Remembered { rows, open }
    }
}

impl From<&Remembered> for File {
    fn from(remembered: &Remembered) -> Self {
        File {
            servers: remembered
                .rows
                .iter()
                .map(|row| Stored {
                    url: row.server.as_str().to_string(),
                    name: row.name.clone(),
                    colour: row.colour.map(|colour| colour.to_string()),
                    picture: row.picture.map(|picture| picture.to_string()),
                    identity: row.identity.clone(),
                    pin: row.pin,
                })
                .collect(),
            open: remembered.open.as_ref().map(|open| open.as_str().to_string()),
        }
    }
}

#[cfg(test)]
fn write_in(folder: &Path, remembered: &Remembered) -> Result<(), Unavailable> {
    let file = File::from(remembered);
    change_kept_in(folder, move |section: &mut File| *section = file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    fn at(host: &str) -> ServerUrl {
        ServerUrl::parse(&format!("wss://{host}/v1")).unwrap()
    }

    /// A folder of the test's own, emptied first: never the folder a person's client uses.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chatkord-state-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// The servers' section written as `body`, as another build or a person would.
    fn leave(dir: &Path, body: &str) {
        raw_kept_in::<File>(dir, body).unwrap();
    }

    fn row(host: &str, name: Option<&str>, identity: Option<&str>) -> Row {
        Row {
            server: at(host),
            name: name.map(str::to_string),
            colour: None,
            picture: None,
            identity: identity.map(str::to_string),
            pin: None,
        }
    }

    fn pin(byte: u8) -> KeyPin {
        KeyPin::from_digest([byte; KeyPin::LEN])
    }

    #[test]
    fn a_trusted_key_is_kept_with_its_row_replaced_dropped_and_forgotten_with_it() {
        let dir = scratch("pin");
        change_in(&dir, |r| r.greeted(at("lan.example.invalid"), ServerIdentity::default())).unwrap();
        change_in(&dir, |r| row_for(&mut r.rows, at("lan.example.invalid")).pin = Some(pin(1))).unwrap();
        change_in(&dir, |r| r.signed_in(at("lan.example.invalid"), ADA.into())).unwrap();
        assert_eq!(read_in(&dir).rows[0].pin, Some(pin(1)), "a sign-in keeps it");
        assert!(kept_text_in::<File>(&dir).contains(&format!("\"pin\":\"{}\"", "01".repeat(KeyPin::LEN))));
        change_in(&dir, |r| row_for(&mut r.rows, at("lan.example.invalid")).pin = Some(pin(2))).unwrap();
        assert_eq!(read_in(&dir).rows[0].pin, Some(pin(2)), "a new key replaces it");
        change_in(&dir, |r| r.dropped_key(&at("lan.example.invalid"))).unwrap();
        assert_eq!(read_in(&dir).rows, [row("lan.example.invalid", None, Some(ADA))], "dropped, the row stays");
        change_in(&dir, |r| row_for(&mut r.rows, at("lan.example.invalid")).pin = Some(pin(3))).unwrap();
        change_in(&dir, |r| r.forgot(&at("lan.example.invalid"))).unwrap();
        change_in(&dir, |r| r.greeted(at("lan.example.invalid"), ServerIdentity::default())).unwrap();
        assert_eq!(read_in(&dir).rows[0].pin, None, "a server added again is asked about again");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_kept_pin_of_another_shape_is_no_pin_and_the_row_stays() {
        let dir = scratch("pin-shape");
        leave(&dir, r#"{"servers":[{"url":"wss://lan.example.invalid/v1","identity":"ada","pin":"B41C 9E07"}]}"#);
        assert_eq!(read_in(&dir).rows, [row("lan.example.invalid", None, Some(ADA))]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rows_read_back_as_written_and_hold_no_secret() {
        let dir = scratch("rows");
        let rows =
            [row("chat.example.invalid", Some("Night Owls"), Some(ADA)), row("books.example.invalid", None, None)];
        write_in(&dir, &Remembered { rows: rows.to_vec(), open: None }).unwrap();
        assert_eq!(read_in(&dir).rows, rows);
        assert_eq!(
            kept_text_in::<File>(&dir),
            json_sorted(
                "{\"servers\":[{\"url\":\"wss://chat.example.invalid/v1\",\"name\":\"Night Owls\",\"identity\":\"ada\"},\
             {\"url\":\"wss://books.example.invalid/v1\"}]}"
            ),
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_of_another_shape_or_a_row_that_is_no_address_is_nothing() {
        let dir = scratch("shapes");
        assert!(read_in(&dir).rows.is_empty(), "a first run");
        leave(&dir, "{\"servers\":\"wss://example.invalid/v1\"}");
        assert!(read_in(&dir).rows.is_empty(), "a list of another shape");
        leave(&dir, "{\"servers\":[{\"url\":\"nonsense\"},{\"url\":\"wss://example.invalid/v1\",\"identity\":\"\"}]}");
        assert_eq!(read_in(&dir).rows, [row("example.invalid", None, None)], "an empty name is no name");
        leave(&dir, "{\"servers\":[{\"url\":\"wss://example.invalid/v1\",\"name\":\"S\",\"colour\":\"plaid\"}]}");
        assert_eq!(read_in(&dir).rows, [row("example.invalid", Some("S"), None)], "a colour of another shape is none");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_colour_is_kept_in_its_one_spelling_beside_the_name() {
        let dir = scratch("colour");
        let blue = Colour::parse("#3d7fd6").ok();
        let rows = [Row { colour: blue, ..row("chat.example.invalid", Some("Night Owls"), None) }];
        write_in(&dir, &Remembered { rows: rows.to_vec(), open: None }).unwrap();
        assert_eq!(read_in(&dir).rows, rows);
        assert!(kept_text_in::<File>(&dir).contains("\"colour\":\"#3D7FD6\""));
        let plaid = r#"{"servers":[{"url":"wss://chat.example.invalid/v1","name":"Night Owls","colour":"plaid"}]}"#;
        leave(&dir, plaid);
        assert_eq!(read_in(&dir).rows, [row("chat.example.invalid", Some("Night Owls"), None)], "only the colour goes");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_picture_is_kept_by_its_id_and_one_of_another_shape_is_none() {
        let dir = scratch("picture");
        let id = FileId::parse("0fa1000000000000000000000000ff10").ok();
        let rows = [Row { picture: id, ..row("chat.example.invalid", Some("Night Owls"), None) }];
        write_in(&dir, &Remembered { rows: rows.to_vec(), open: None }).unwrap();
        assert_eq!(read_in(&dir).rows, rows);
        assert!(kept_text_in::<File>(&dir).contains("\"picture\":\"0fa1000000000000000000000000ff10\""));
        leave(&dir, r#"{"servers":[{"url":"wss://chat.example.invalid/v1","picture":"../../etc/passwd"}]}"#);
        assert_eq!(read_in(&dir).rows, [row("chat.example.invalid", None, None)], "only the picture goes");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn greeting_a_server_again_keeps_its_account_and_its_place() {
        // Servers stay in the order they were added: a greeting or a sign-in moves nothing.
        let dir = scratch("greet-keeps");
        let named = |name: &str| ServerIdentity { name: Some(name.into()), colour: None, picture: None };
        change_in(&dir, |r| r.greeted(at("books.example.invalid"), named("Book club"))).unwrap();
        change_in(&dir, |r| r.signed_in(at("chat.example.invalid"), ADA.into())).unwrap();
        change_in(&dir, |r| r.greeted(at("chat.example.invalid"), named("Night Owls"))).unwrap();
        change_in(&dir, |r| r.signed_in(at("chat.example.invalid"), ADA.into())).unwrap();
        assert_eq!(
            read_in(&dir).rows,
            [
                row("books.example.invalid", Some("Book club"), None),
                row("chat.example.invalid", Some("Night Owls"), Some(ADA)),
            ],
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_open_server_is_kept_until_it_is_forgotten_and_a_sign_out_keeps_the_row() {
        let dir = scratch("open-forget");
        change_in(&dir, |r| r.signed_in(at("chat.example.invalid"), ADA.into())).unwrap();
        change_in(&dir, |r| r.signed_in(at("books.example.invalid"), ADA.into())).unwrap();
        assert_eq!(read_in(&dir).open, None);
        change_in(&dir, |r| r.open = Some(at("books.example.invalid"))).unwrap();
        assert_eq!(read_in(&dir).open, Some(at("books.example.invalid")));
        change_in(&dir, |r| r.signed_out(&at("chat.example.invalid"))).unwrap();
        assert_eq!(
            read_in(&dir).rows[0],
            row("chat.example.invalid", None, None),
            "the row stays, without its account"
        );
        change_in(&dir, |r| r.forgot(&at("books.example.invalid"))).unwrap();
        assert_eq!(read_in(&dir).rows, [row("chat.example.invalid", None, None)]);
        assert_eq!(read_in(&dir).open, None, "a forgotten server is not opened");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn servers_remembered_at_once_from_many_threads_are_all_kept() {
        // Servers reconnecting together each remember themselves.
        let dir = scratch("together");
        let hosts: Vec<String> = (0..32).map(|n| format!("s{n}.example.invalid")).collect();
        std::thread::scope(|scope| {
            for host in &hosts {
                let dir = &dir;
                scope.spawn(move || {
                    change_in(dir, |remembered| {
                        row_for(&mut remembered.rows, at(host));
                    })
                    .unwrap()
                });
            }
        });
        let mut kept: Vec<String> = read_in(&dir).rows.iter().map(|row| row.server.as_str().to_string()).collect();
        kept.sort();
        let mut wanted: Vec<String> = hosts.iter().map(|host| at(host).as_str().to_string()).collect();
        wanted.sort();
        assert_eq!(kept, wanted);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_does_not_parse_is_set_aside_not_written_over() {
        let dir = scratch("unreadable");
        let half = "{\"servers\":[{\"url\":\"wss://chat.exam";
        let file = kept_in(&dir).file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, half).unwrap();
        assert!(read_in(&dir).rows.is_empty(), "shown as nothing");
        change_in(&dir, |remembered| {
            row_for(&mut remembered.rows, at("books.example.invalid"));
        })
        .unwrap();
        assert_eq!(read_in(&dir).rows, [row("books.example.invalid", None, None)]);
        let aside = file.with_file_name(format!("{}.unreadable", crate::kept::FILE));
        assert_eq!(std::fs::read_to_string(aside).unwrap(), half, "nothing is lost");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_a_folder_stated_from_the_root_is_used() {
        assert_eq!(chosen(from_root("chatkord"), from_root("/run/user/chatkord")), from_root("/run/user/chatkord"));
        assert_eq!(chosen(from_root("/srv/chatkord"), from_root("/run/user/chatkord")), from_root("/srv/chatkord"));
        assert_eq!(chosen(from_root("./chatkord"), from_root("chatkord")), None);
    }
}
