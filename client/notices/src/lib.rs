//! The third-party notices a package carries beside the program (`notices.json`, which scripts/package/notices.sh
//! writes): read whole, and refused whole when the file is over [`MOST_BYTES`], is not that shape, or holds a value
//! over its bound. The caller reads the file; this reads its bytes.
#![forbid(unsafe_code)]

use serde::Deserialize;
use std::fmt;

/// The most bytes a notices file is read to; the client package's is under 1 MB.
pub const MOST_BYTES: usize = 8 * 1024 * 1024;

/// The most groups a notices file holds.
pub const MOST_GROUPS: usize = 512;

/// A field of a group, each with a bound of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The project's name, or a licence's id.
    Name,
    /// What it is for in Chatkord.
    What,
    /// The licence's id, or the ids it is under.
    Licence,
    /// Where it was built from.
    Source,
    /// The licence's whole text.
    Text,
}

impl Field {
    /// The most bytes the field holds; none holds nothing.
    pub const fn most(self) -> usize {
        match self {
            Field::Name | Field::Licence => 128,
            Field::What => 256,
            Field::Source => 512,
            Field::Text => 4 * 1024 * 1024,
        }
    }
}

/// The most bytes a package's version, a tag or a commit holds.
pub const MOST_BUILT: usize = 64;

/// The package itself, as it was built: what About Chatkord states before what it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    version: String,
    from: Origin,
}

impl Built {
    /// The package's version: a release's, or the pre-release one a build off a tag is given.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where its source is.
    pub fn from(&self) -> &Origin {
        &self.from
    }
}

/// Where a package's source is: a release's tag, or the commit a build off one was made from.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Origin {
    /// A release's tag, `v` and its version.
    Tag(String),
    /// A commit, by its abbreviated hash.
    Commit(String),
}

/// Why a notices file was refused.
#[derive(Debug)]
pub enum Refused {
    /// It is over [`MOST_BYTES`].
    TooLarge,
    /// It is not the JSON scripts/package/notices.sh writes.
    Shape(serde_json::Error),
    /// It holds more than [`MOST_GROUPS`] groups.
    TooMany,
    /// The package's version, tag or commit is empty or over [`MOST_BUILT`].
    Built,
    /// A group's field is empty or over its bound; `group` counts from 0.
    Field {
        /// Which group, in the file's order.
        group: usize,
        /// Which of its fields.
        field: Field,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::TooLarge => write!(f, "the notices file is over {MOST_BYTES} bytes"),
            Refused::Shape(why) => write!(f, "the notices file is not notices: {why}"),
            Refused::TooMany => write!(f, "the notices file holds more than {MOST_GROUPS} groups"),
            Refused::Built => write!(f, "the notices file's build is empty or over {MOST_BUILT} bytes"),
            Refused::Field { group, field } => {
                write!(f, "group {group} of the notices file has an empty {field:?} or one over {} bytes", field.most())
            }
        }
    }
}

impl std::error::Error for Refused {}

/// One carried project, or the Rust packages under one licence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    name: String,
    what: String,
    licence: String,
    source: Option<String>,
    text: String,
}

impl Group {
    /// Its name: "FFmpeg", or a licence's id for the Rust packages under it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What it is for in Chatkord, or which Rust packages it covers.
    pub fn what(&self) -> &str {
        &self.what
    }

    /// Its licence's id, or the ids it is under.
    pub fn licence(&self) -> &str {
        &self.licence
    }

    /// Where it was built from, for a project the package builds itself.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// The licence's whole text, a paragraph to a line.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The package's notices, in the order they are shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notices {
    built: Option<Built>,
    groups: Vec<Group>,
}

impl Notices {
    /// The package itself, where the file names it.
    pub fn built(&self) -> Option<&Built> {
        self.built.as_ref()
    }

    /// Every group, in the order they are shown.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// The notices `file` holds, or why the whole file is refused.
    pub fn parse(file: &[u8]) -> Result<Notices, Refused> {
        if file.len() > MOST_BYTES {
            return Err(Refused::TooLarge);
        }
        let file: File = serde_json::from_slice(file).map_err(Refused::Shape)?;
        if file.groups.len() > MOST_GROUPS {
            return Err(Refused::TooMany);
        }
        let built = file.built.map(WrittenBuilt::checked).transpose()?;
        let groups =
            file.groups.into_iter().enumerate().map(|(at, group)| group.checked(at)).collect::<Result<_, _>>()?;
        Ok(Notices { built, groups })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    built: Option<WrittenBuilt>,
    groups: Vec<Written>,
}

/// The package as the file has it, before its bounds are checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenBuilt {
    version: String,
    from: Origin,
}

impl WrittenBuilt {
    fn checked(self) -> Result<Built, Refused> {
        let fits = |value: &str| !value.is_empty() && value.len() <= MOST_BUILT;
        let (Origin::Tag(from) | Origin::Commit(from)) = &self.from;
        if fits(&self.version) && fits(from) {
            Ok(Built { version: self.version, from: self.from })
        } else {
            Err(Refused::Built)
        }
    }
}

/// A group as the file has it, before its bounds are checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    name: String,
    what: String,
    licence: String,
    #[serde(default)]
    source: Option<String>,
    text: String,
}

impl Written {
    fn checked(self, group: usize) -> Result<Group, Refused> {
        let bounded = |value: String, field: Field| {
            if value.is_empty() || value.len() > field.most() {
                Err(Refused::Field { group, field })
            } else {
                Ok(value)
            }
        };
        Ok(Group {
            name: bounded(self.name, Field::Name)?,
            what: bounded(self.what, Field::What)?,
            licence: bounded(self.licence, Field::Licence)?,
            source: self.source.map(|source| bounded(source, Field::Source)).transpose()?,
            text: bounded(self.text, Field::Text)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FFMPEG: &str = r#"{"name": "FFmpeg", "what": "Decodes and encodes video", "licence": "GPL-2.0-or-later",
        "source": "Built from commit 37515cfd.", "text": "GNU GENERAL PUBLIC LICENSE"}"#;
    const MIT: &str = r#"{"name": "MIT", "what": "1 Rust package: serde", "licence": "MIT", "text": "MIT License"}"#;

    fn file(groups: &[String]) -> Vec<u8> {
        format!(r#"{{"groups": [{}]}}"#, groups.join(",")).into_bytes()
    }

    fn with(field: &str, value: &str) -> String {
        let mut group: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(FFMPEG).unwrap_or_else(|_| serde_json::Map::new());
        group.insert(field.into(), value.into());
        serde_json::Value::Object(group).to_string()
    }

    #[test]
    fn a_written_file_reads_in_its_order_and_a_source_is_had_only_where_written() {
        let read = Notices::parse(&file(&[FFMPEG.into(), MIT.into()])).expect("read");
        let [ffmpeg, mit] = read.groups() else { panic!("two groups: {read:?}") };
        assert_eq!(
            (ffmpeg.name(), ffmpeg.what(), ffmpeg.licence(), ffmpeg.source(), ffmpeg.text()),
            (
                "FFmpeg",
                "Decodes and encodes video",
                "GPL-2.0-or-later",
                Some("Built from commit 37515cfd."),
                "GNU GENERAL PUBLIC LICENSE"
            )
        );
        assert_eq!((mit.name(), mit.source()), ("MIT", None));
    }

    #[test]
    fn a_package_names_its_version_and_its_tag_or_commit_and_a_file_without_one_names_none() {
        let tagged = br#"{"built": {"version": "0.9.0", "from": {"tag": "v0.9.0"}}, "groups": []}"#;
        let read = Notices::parse(tagged).expect("read");
        let built = read.built().expect("built");
        assert_eq!((built.version(), built.from()), ("0.9.0", &Origin::Tag("v0.9.0".into())));
        let off = br#"{"built": {"version": "0.9.1-dev.20261114", "from": {"commit": "1a2b3c4d"}}, "groups": []}"#;
        assert_eq!(
            Notices::parse(off).expect("read").built().map(Built::from),
            Some(&Origin::Commit("1a2b3c4d".into()))
        );
        assert_eq!(Notices::parse(&file(&[MIT.into()])).expect("read").built(), None);
    }

    #[test]
    fn a_build_over_its_bound_or_empty_refuses_the_file() {
        let at = |version: &str, commit: &str| {
            format!(r#"{{"built": {{"version": "{version}", "from": {{"commit": "{commit}"}}}}, "groups": []}}"#)
        };
        let full = "x".repeat(MOST_BUILT);
        assert!(Notices::parse(at(&full, &full).as_bytes()).is_ok(), "both at their bound");
        let over = "x".repeat(MOST_BUILT + 1);
        for (version, commit) in [(over.as_str(), "1a2b"), ("0.9.0", over.as_str()), ("", "1a2b"), ("0.9.0", "")] {
            assert!(
                matches!(Notices::parse(at(version, commit).as_bytes()), Err(Refused::Built)),
                "{version} {commit}"
            );
        }
        let neither = br#"{"built": {"version": "0.9.0", "from": {"branch": "main"}}, "groups": []}"#;
        assert!(matches!(Notices::parse(neither), Err(Refused::Shape(_))));
    }

    #[test]
    fn a_file_over_its_bound_is_refused_unread() {
        let padded = [file(&[MIT.into()]), vec![b' '; MOST_BYTES]].concat();
        assert!(matches!(Notices::parse(&padded), Err(Refused::TooLarge)));
        let at_the_bound = [file(&[MIT.into()]), vec![b' '; MOST_BYTES - file(&[MIT.into()]).len()]].concat();
        assert!(Notices::parse(&at_the_bound).is_ok(), "the bound itself is read");
    }

    #[test]
    fn more_groups_than_the_bound_are_refused_whole() {
        assert!(Notices::parse(&file(&vec![MIT.to_string(); MOST_GROUPS])).is_ok());
        assert!(matches!(Notices::parse(&file(&vec![MIT.to_string(); MOST_GROUPS + 1])), Err(Refused::TooMany)));
    }

    #[test]
    fn each_field_over_its_bound_or_empty_refuses_the_file_and_names_the_group() {
        for (name, field) in [
            ("name", Field::Name),
            ("what", Field::What),
            ("licence", Field::Licence),
            ("source", Field::Source),
            ("text", Field::Text),
        ] {
            let full = "x".repeat(field.most());
            assert!(Notices::parse(&file(&[MIT.into(), with(name, &full)])).is_ok(), "{name} at its bound");
            for wrong in ["x".repeat(field.most() + 1), String::new()] {
                let refused = Notices::parse(&file(&[MIT.into(), with(name, &wrong)]));
                assert!(
                    matches!(refused, Err(Refused::Field { group: 1, field: found }) if found == field),
                    "{name} of {} bytes: {refused:?}",
                    wrong.len()
                );
            }
        }
    }

    #[test]
    fn what_is_not_the_written_shape_is_refused() {
        for wrong in [
            &b"not json"[..],
            br#"{"groups": [{"name": "MIT", "what": "w", "licence": "MIT"}]}"#,
            br#"{"groups": [{"name": "MIT", "what": "w", "licence": "MIT", "text": "t", "extra": 1}]}"#,
            br#"{"groups": [], "more": []}"#,
            br#"{"groups": [{"name": 1, "what": "w", "licence": "MIT", "text": "t"}]}"#,
            br#"[]"#,
        ] {
            assert!(matches!(Notices::parse(wrong), Err(Refused::Shape(_))), "{}", String::from_utf8_lossy(wrong));
        }
    }
}
