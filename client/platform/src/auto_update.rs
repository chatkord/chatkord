//! Whether Chatkord downloads its own updates, kept for the next run: Settings › About Chatkord's switch.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::AutoUpdate;
use serde::{Deserialize, Serialize};

const FILE: &str = "updates";

/// The word kept for a switch turned off; anything else, or nothing, is on.
const OFF: &str = "off";

/// The file: `automatic` absent while the switch is on, as it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Held {
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    automatic: Option<String>,
}

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

impl Held {
    fn auto_update(&self) -> AutoUpdate {
        match self.automatic.as_deref() {
            Some(OFF) => AutoUpdate::Off,
            Some(_) | None => AutoUpdate::On,
        }
    }

    fn of(auto_update: AutoUpdate) -> Held {
        Held {
            automatic: match auto_update {
                AutoUpdate::On => None,
                AutoUpdate::Off => Some(OFF.to_string()),
            },
        }
    }
}

/// Whether Chatkord downloads its own updates; on where nothing was kept.
pub async fn auto_update(platform: &Platform) -> AutoUpdate {
    read_kept::<Held>(platform).await.auto_update()
}

/// Keeps `auto_update` for the next run.
pub async fn keep(platform: &Platform, auto_update: AutoUpdate) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| *held = Held::of(auto_update)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{kept_text_in, raw_kept_in, read_kept_in, write_kept_in};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-auto-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn the_switch_reads_back_as_kept_and_on_writes_nothing() {
        let folder = scratch("round");
        write_kept_in(&folder, &Held::of(AutoUpdate::Off)).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).auto_update(), AutoUpdate::Off);
        write_kept_in(&folder, &Held::of(AutoUpdate::On)).unwrap();
        assert_eq!(kept_text_in::<Held>(&folder), "{}");
        assert_eq!(read_kept_in::<Held>(&folder).auto_update(), AutoUpdate::On);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_word_this_build_does_not_know_is_on_as_the_switch_starts() {
        let folder = scratch("odd");
        for settings in [r#"{"automatic":"sometimes"}"#, r#"{"automatic":7}"#, "{}"] {
            raw_kept_in::<Held>(&folder, settings).unwrap();
            assert_eq!(read_kept_in::<Held>(&folder).auto_update(), AutoUpdate::On, "{settings}");
        }
        raw_kept_in::<Held>(&folder, r#"{"automatic":"off"}"#).unwrap();
        assert_eq!(read_kept_in::<Held>(&folder).auto_update(), AutoUpdate::Off);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
