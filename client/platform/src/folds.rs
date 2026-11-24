//! Which categories of a server's channel list are folded on this device, kept for the next run under the server's
//! address, since a category's id is that server's own. Only a folded category is written: an open one is not in the
//! file.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use terms::{CategoryId, ServerUrl};

const FILE: &str = "folds";

/// The section: each server's address, then the ids of its folded categories.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
struct Held(BTreeMap<String, BTreeSet<String>>);

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

/// Each server's folded categories, by its address; a category kept under an empty id is none, and is passed over.
fn folds(held: Held) -> BTreeMap<String, BTreeSet<CategoryId>> {
    held.0
        .into_iter()
        .map(|(server, folded)| (server, folded.iter().filter_map(|id| CategoryId::parse(id)).collect::<BTreeSet<_>>()))
        .filter(|(_, folded)| !folded.is_empty())
        .collect()
}

/// `held` with `server`'s folded categories `folded`; a server with none folded is taken out.
fn keep_in(held: &mut Held, server: &str, folded: &BTreeSet<CategoryId>) {
    if folded.is_empty() {
        held.0.remove(server);
    } else {
        held.0.insert(server.to_string(), folded.iter().map(|id| id.as_str().to_owned()).collect());
    }
}

/// Every server's folded categories, by its address; none where nothing was kept or the file cannot be read.
pub async fn kept(platform: &Platform) -> BTreeMap<String, BTreeSet<CategoryId>> {
    folds(read_kept::<Held>(platform).await)
}

/// Keeps `folded` as the categories folded at `server`, in place of those kept before.
pub async fn keep(platform: &Platform, server: ServerUrl, folded: BTreeSet<CategoryId>) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| keep_in(held, server.as_str(), &folded)).await
}

/// Forgets every fold kept at `server`, a server this device no longer keeps.
pub async fn forget(platform: &Platform, server: ServerUrl) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| {
        held.0.remove(server.as_str());
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{json_sorted, kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    const OWLS: &str = "wss://chat.example.invalid/v1";
    const BOOKS: &str = "wss://books.example.invalid/v1";

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-folds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn ids(ids: &[&str]) -> BTreeSet<CategoryId> {
        ids.iter().filter_map(|id| CategoryId::parse(id)).collect()
    }

    #[test]
    fn folds_kept_read_back_at_their_server_alone_and_none_folded_leaves_the_file() {
        let folder = scratch("kept");
        let mut held = Held::default();
        keep_in(&mut held, OWLS, &ids(&["projects", "hangouts"]));
        write_kept_in(&folder, &held).unwrap();
        let text = kept_text_in::<Held>(&folder);
        assert_eq!(text, json_sorted(r#"{"wss://chat.example.invalid/v1":["hangouts","projects"]}"#));
        let read = folds(read_kept_in(&folder));
        assert_eq!(read[OWLS], ids(&["hangouts", "projects"]));
        assert!(!read.contains_key(BOOKS), "the same id at another server is another category");

        keep_in(&mut held, OWLS, &ids(&["projects"]));
        assert_eq!(folds(held.clone())[OWLS], ids(&["projects"]), "a keep replaces what the server kept");
        keep_in(&mut held, OWLS, &BTreeSet::new());
        assert_eq!(held, Held::default(), "a server with none folded is not written");
    }

    #[test]
    fn an_empty_id_or_another_shape_is_passed_over_and_leaves_the_rest() {
        let folder = scratch("odd");
        std::fs::create_dir_all(folder.join("chatkord")).unwrap();
        let odd = r#"{"wss://chat.example.invalid/v1":["","games"],"wss://books.example.invalid/v1":[""]}"#;
        raw_kept_in::<Held>(&folder, odd).unwrap();
        let read = folds(read_kept_in(&folder));
        assert_eq!(read[OWLS], ids(&["games"]));
        assert!(!read.contains_key(BOOKS), "a server left with nothing folded is none");
        raw_kept_in::<Held>(&folder, r#"{"wss://chat.example.invalid/v1":"games"}"#).unwrap();
        assert!(folds(read_kept_in(&folder)).is_empty(), "a section that does not read is its default");
    }
}
