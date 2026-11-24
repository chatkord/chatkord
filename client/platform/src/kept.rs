//! What this device keeps between runs: one file, `chatkord.json`, of sections, one for each concern that keeps
//! something, each written with its format beside its settings. Each section is read on its own, as far as its fields
//! read whatever format it names; one that does not read is that concern's defaults alone, set aside beside the file
//! for a look and never written over; a file that is not a JSON object is set aside whole. Edits take turns under one
//! lock, from any thread or process, and a reader never sees half a file.

use crate::Unavailable;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The file's name in this program's folder.
pub const FILE: &str = "chatkord.json";

/// One concern's part of the file.
pub(crate) trait Section: Serialize + DeserializeOwned + Default {
    /// Its key in the file, which no other section uses.
    const NAME: &'static str;
    /// The format this build writes: raised when what a kept field means or how it is written changes, never for a
    /// field added with its default.
    const FORMAT: u32;
}

/// A section as the file holds it: its settings beside the format a build wrote them in, which a section must name.
#[derive(Deserialize)]
struct Held {
    #[serde(rename = "format")]
    _format: u32,
    settings: Value,
}

/// A section as this build writes it.
#[derive(Serialize)]
struct Writing<'a, S> {
    format: u32,
    settings: &'a S,
}

/// `S` from its part of the file, as far as its fields read (a newer build's format too); none when it is not a section
/// with a format.
fn section<S: Section>(raw: &Value) -> Option<S> {
    let Held { settings, .. } = Held::deserialize(raw).ok()?;
    S::deserialize(settings).ok()
}

/// A field read as none when its value has another shape, so one bad value is its own default alone:
/// `#[serde(default, deserialize_with = "crate::kept::lenient")]`.
pub(crate) fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(from: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(from)?).ok())
}

/// The file in one folder: the program's own, which the caller names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Kept {
    folder: PathBuf,
}

impl Kept {
    /// The file in `folder`, which is made when something is first kept.
    pub(crate) fn in_folder(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// `S` as kept; its defaults when the file, the section or a readable one is missing. Never writes.
    pub(crate) fn read<S: Section>(&self) -> S {
        self.held().unwrap_or_default()
    }

    /// `S` as [`Kept::read`] gives it, but refused when the file is there and cannot be read at all, so what acts on
    /// its absence never takes an unread file for an empty one.
    pub(crate) fn held<S: Section>(&self) -> Result<S, Unavailable> {
        let bytes = match std::fs::read(self.file()) {
            Ok(bytes) => bytes,
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(S::default()),
            Err(_) => return Err(Unavailable),
        };
        let sections = serde_json::from_slice::<Map<String, Value>>(&bytes).unwrap_or_default();
        Ok(sections.get(S::NAME).and_then(section).unwrap_or_default())
    }

    /// Reads `S`, edits it and writes the file back whole with every other section as it was.
    pub(crate) fn change<S: Section, M>(&self, edit: impl FnOnce(&mut S) -> M) -> Result<M, Unavailable> {
        std::fs::create_dir_all(&self.folder).map_err(|_| Unavailable)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(beside(&self.file(), "lock"))
            .map_err(|_| Unavailable)?;
        lock.lock().map_err(|_| Unavailable)?;
        let mut sections = self.sections()?;
        let mut section = match sections.remove(S::NAME) {
            None => S::default(),
            Some(raw) => match section(&raw) {
                Some(read) => read,
                None => {
                    let aside = beside(&self.file(), &format!("{}.unreadable", S::NAME));
                    write_whole(&aside, &raw)?;
                    S::default()
                }
            },
        };
        let made = edit(&mut section);
        let writing = Writing { format: S::FORMAT, settings: &section };
        sections.insert(S::NAME.to_string(), serde_json::to_value(writing).map_err(|_| Unavailable)?);
        write_whole(&self.file(), &sections)?;
        Ok(made)
    }

    /// Where the file is, for a test that writes it as a person or another build would.
    pub(crate) fn file(&self) -> PathBuf {
        self.folder.join(FILE)
    }

    /// Every section as written, raw; a file that is not a JSON object is set aside whole and none is.
    fn sections(&self) -> Result<Map<String, Value>, Unavailable> {
        let file = self.file();
        match std::fs::read(&file) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(sections) => Ok(sections),
                Err(_) => {
                    std::fs::rename(&file, beside(&file, "unreadable")).map_err(|_| Unavailable)?;
                    Ok(Map::new())
                }
            },
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(_) => Err(Unavailable),
        }
    }
}

/// Written beside the file and renamed over it, so a reader never sees half of one.
fn write_whole<T: Serialize>(path: &Path, held: &T) -> Result<(), Unavailable> {
    let bytes = serde_json::to_vec_pretty(held).map_err(|_| Unavailable)?;
    let partial = beside(path, &format!("{}{}", std::process::id(), crate::partial::SUFFIX));
    let written = std::fs::File::create(&partial)
        .and_then(|mut file| std::io::Write::write_all(&mut file, &bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&partial, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written.map_err(|_| Unavailable)
}

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Look {
        zoom: u32,
    }

    impl Section for Look {
        const NAME: &'static str = "look";
        const FORMAT: u32 = 1;
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Fields(BTreeMap<String, u32>);

    impl Section for Fields {
        const NAME: &'static str = "fields";
        const FORMAT: u32 = 1;
    }

    /// A folder of the test's own, emptied first: never the folder a person's client uses.
    fn scratch(name: &str) -> Kept {
        let dir = std::env::temp_dir().join(format!("chatkord-kept-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Kept::in_folder(dir)
    }

    fn written(kept: &Kept, whole: &Value) {
        std::fs::create_dir_all(&kept.folder).expect("folder");
        std::fs::write(kept.file(), whole.to_string()).expect("written");
    }

    fn on_disk(kept: &Kept) -> Value {
        serde_json::from_slice(&std::fs::read(kept.file()).expect("written")).expect("JSON")
    }

    fn aside(kept: &Kept, suffix: &str) -> Option<Value> {
        let text = std::fs::read_to_string(beside(&kept.file(), suffix)).ok()?;
        Some(serde_json::from_str(&text).expect("JSON"))
    }

    #[test]
    fn a_section_never_kept_reads_as_its_defaults_and_reading_writes_nothing() {
        let kept = scratch("empty");
        assert_eq!(kept.read::<Look>(), Look::default());
        assert!(!kept.file().exists());
    }

    #[test]
    fn a_section_is_written_with_its_format_beside_its_settings() {
        let kept = scratch("format");
        kept.change(|look: &mut Look| look.zoom = 125).expect("kept");
        assert_eq!(on_disk(&kept), json!({"look": {"format": 1, "settings": {"zoom": 125}}}));
        assert_eq!(kept.read::<Look>(), Look { zoom: 125 });
    }

    #[test]
    fn an_edit_of_one_section_keeps_the_others() {
        let kept = scratch("others");
        let window = json!({"format": 1, "settings": {"x": 3}});
        written(&kept, &json!({"window": window, "look": {"format": 1, "settings": {"zoom": 90}}}));
        kept.change(|look: &mut Look| look.zoom = 125).expect("kept");
        assert_eq!(on_disk(&kept), json!({"window": window, "look": {"format": 1, "settings": {"zoom": 125}}}));
    }

    #[test]
    fn a_newer_format_is_read_as_far_as_it_reads_and_written_back_in_this_one() {
        let kept = scratch("newer");
        written(&kept, &json!({"look": {"format": 2, "settings": {"zoom": 80, "grain": "fine"}}}));
        assert_eq!(kept.read::<Look>(), Look { zoom: 80 });
        kept.change(|look: &mut Look| look.zoom = 90).expect("kept");
        assert_eq!(on_disk(&kept), json!({"look": {"format": 1, "settings": {"zoom": 90}}}));
    }

    #[test]
    fn a_section_naming_no_format_is_set_aside_once_and_starts_from_its_defaults() {
        let kept = scratch("no-format");
        written(&kept, &json!({"look": {"zoom": 90}}));
        assert_eq!(kept.read::<Look>(), Look::default(), "a section with no format is none");
        kept.change(|look: &mut Look| look.zoom = 110).expect("kept");
        assert_eq!(aside(&kept, "look.unreadable"), Some(json!({"zoom": 90})));
        std::fs::remove_file(beside(&kept.file(), "look.unreadable")).expect("removed");
        kept.change(|look: &mut Look| look.zoom = 120).expect("kept");
        assert_eq!(aside(&kept, "look.unreadable"), None, "set aside once");
        assert_eq!(kept.read::<Look>(), Look { zoom: 120 });
    }

    #[test]
    fn a_bad_section_resets_alone_and_is_set_aside() {
        let kept = scratch("bad-section");
        let window = json!({"format": 1, "settings": {"x": 3}});
        let bad = json!({"format": 1, "settings": {"zoom": "huge"}});
        written(&kept, &json!({"window": window, "look": bad}));
        assert_eq!(kept.read::<Look>(), Look::default(), "a section that does not read is its defaults");
        kept.change(|look: &mut Look| look.zoom = 110).expect("kept");
        assert_eq!(on_disk(&kept), json!({"window": window, "look": {"format": 1, "settings": {"zoom": 110}}}));
        assert_eq!(aside(&kept, "look.unreadable"), Some(bad));
    }

    #[test]
    fn a_file_that_is_not_a_json_object_is_set_aside_whole() {
        let kept = scratch("bad-file");
        std::fs::create_dir_all(&kept.folder).expect("folder");
        let half = r#"{"look": {"format": 1, "settings": {"zoom": 9"#;
        std::fs::write(kept.file(), half).expect("written");
        kept.change(|look: &mut Look| look.zoom = 100).expect("kept");
        assert_eq!(std::fs::read_to_string(beside(&kept.file(), "unreadable")).expect("set aside"), half);
        assert_eq!(on_disk(&kept), json!({"look": {"format": 1, "settings": {"zoom": 100}}}));
    }

    #[test]
    fn edits_from_many_threads_take_turns() {
        let kept = scratch("turns");
        std::thread::scope(|scope| {
            for n in 0..32u32 {
                let kept = &kept;
                scope.spawn(move || {
                    kept.change(|fields: &mut Fields| fields.0.insert(format!("f{n}"), n)).expect("kept")
                });
            }
        });
        assert_eq!(kept.read::<Fields>().0.len(), 32);
    }
}
