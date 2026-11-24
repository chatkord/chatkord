//! How loud this device plays each member's sounds, kept for the next run under the server's address, since a
//! member's id is that server's own. Only a volume below full is written: a member heard as sent is not in the file.

use crate::state::{change_kept, read_kept};
use crate::{Platform, Unavailable};
use choices::{Audio, Loudness, Volume, Volumes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use terms::{MemberId, ServerUrl};

const FILE: &str = "volumes";

/// The section: each server's address, then each member's id there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
struct Held(BTreeMap<String, BTreeMap<String, Member>>);

impl crate::kept::Section for Held {
    const NAME: &'static str = FILE;
    const FORMAT: u32 = 1;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
struct Member {
    /// Their microphone's volume in whole percent; none for full.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    voice: Option<u8>,
    /// Their share's sound's.
    #[serde(default, deserialize_with = "crate::kept::lenient", skip_serializing_if = "Option::is_none")]
    share: Option<u8>,
}

impl Member {
    /// A percent over full, or of another shape, is full.
    fn loudness(&self) -> Loudness {
        let volume = |percent: Option<u8>| percent.and_then(|percent| Volume::new(percent.into())).unwrap_or_default();
        Loudness { voice: volume(self.voice), share: volume(self.share) }
    }
}

/// Each server's volumes, by its address; a member kept under an empty id is no one, and is passed over.
fn volumes(held: Held) -> BTreeMap<String, Volumes> {
    held.0
        .into_iter()
        .map(|(server, members)| {
            let kept = members
                .into_iter()
                .filter_map(|(user_id, member)| Some((MemberId::parse(&user_id)?, member.loudness())))
                .collect();
            (server, kept)
        })
        .collect()
}

/// `held` with `user_id`'s `audio` at `volume` on `server`; a member or a server left at full is taken out.
fn keep_in(held: &mut Held, server: &str, user_id: &MemberId, audio: Audio, volume: Volume) {
    let members = held.0.entry(server.to_string()).or_default();
    let member = members.entry(user_id.as_str().to_owned()).or_default();
    let percent = (volume != Volume::FULL).then_some(volume.percent());
    match audio {
        Audio::Voice => member.voice = percent,
        Audio::Share => member.share = percent,
    }
    if member.loudness() == Loudness::default() {
        members.remove(user_id.as_str());
    }
    if members.is_empty() {
        held.0.remove(server);
    }
}

/// Every server's kept volumes, by its address; none where nothing was kept or the file cannot be read.
pub async fn kept(platform: &Platform) -> BTreeMap<String, Volumes> {
    volumes(read_kept::<Held>(platform).await)
}

/// Keeps how loud `user_id`'s `audio` plays at `server`.
pub async fn keep(
    platform: &Platform,
    server: ServerUrl,
    user_id: MemberId,
    audio: Audio,
    volume: Volume,
) -> Result<(), Unavailable> {
    change_kept::<Held, _>(platform, move |held| keep_in(held, server.as_str(), &user_id, audio, volume)).await
}

/// Forgets every volume kept at `server`, a server this device no longer keeps.
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
    const BO: &str = "bo";

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("chatkord-volume-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn bo() -> MemberId {
        MemberId::parse(BO).unwrap()
    }

    fn percent(percent: u16) -> Volume {
        Volume::new(percent).unwrap()
    }

    #[test]
    fn a_volume_kept_reads_back_at_its_server_alone_and_a_full_one_leaves_the_file() {
        let folder = scratch("kept");
        let mut held = Held::default();
        keep_in(&mut held, OWLS, &bo(), Audio::Voice, percent(40));
        keep_in(&mut held, OWLS, &bo(), Audio::Share, percent(0));
        write_kept_in(&folder, &held).unwrap();
        let text = kept_text_in::<Held>(&folder);
        assert_eq!(text, json_sorted(r#"{"wss://chat.example.invalid/v1":{"bo":{"voice":40,"share":0}}}"#));
        let read = volumes(read_kept_in(&folder));
        assert_eq!(read[OWLS].of(&bo(), Audio::Voice), percent(40));
        assert_eq!(read[OWLS].of(&bo(), Audio::Share), percent(0));
        assert!(!read.contains_key(BOOKS), "the same id at another server is another person");

        keep_in(&mut held, OWLS, &bo(), Audio::Voice, Volume::FULL);
        assert_eq!(held.0[OWLS][BO], Member { voice: None, share: Some(0) }, "a full volume is not written");
        keep_in(&mut held, OWLS, &bo(), Audio::Share, Volume::FULL);
        assert_eq!(held, Held::default(), "nor is a member, or a server, heard as sent");
    }

    #[test]
    fn a_percent_over_full_or_of_another_shape_is_full_and_leaves_the_rest() {
        let folder = scratch("odd");
        std::fs::create_dir_all(folder.join("chatkord")).unwrap();
        let odd =
            r#"{"wss://chat.example.invalid/v1":{"bo":{"voice":101,"share":30},"cy":{"voice":"loud","share":-1}}}"#;
        raw_kept_in::<Held>(&folder, odd).unwrap();
        let read = volumes(read_kept_in(&folder));
        assert_eq!(read[OWLS].of(&bo(), Audio::Voice), Volume::FULL);
        assert_eq!(read[OWLS].of(&bo(), Audio::Share), percent(30));
        assert_eq!(read[OWLS].iter().count(), 1, "a member with nothing below full is no one kept");
    }
}
