//! Which channel this build is, and every name it goes by. Chosen once at build time (`CHATKORD_CHANNEL`: `stable`,
//! the default, or `dev`) from `channels/<channel>.env`, which the packaging scripts source too: a dev build is an
//! app of its own beside the stable one, its data, sign-ins, install and update feed apart.

#![forbid(unsafe_code)]

/// A build's channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// The releases from `main`.
    Stable,
    /// The dev releases from `develop`, installed beside the stable app.
    Dev,
}

/// The names a build goes by, all from its channel's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Its channel.
    pub channel: Channel,
    /// The id of the app: its app id, desktop entry and icon name, its folders, its keyring service. Lowercase
    /// ASCII letters, digits and `-`.
    pub id: &'static str,
    /// The name a person sees: the window's title, the installed app's name. Letters, digits and spaces.
    pub product: &'static str,
    /// The stem of its package files and of its Windows executable: letters, digits and `-`, no space.
    pub file: &'static str,
    /// Windows' application user model id, which groups its windows, shortcut and notifications.
    pub app_user_model_id: &'static str,
    /// The file beside `client/update/Cargo.toml` holding the public key its updates are signed by.
    pub update_key: &'static str,
}

/// This build's names.
pub const THIS: Identity = include!(concat!(env!("OUT_DIR"), "/identity.rs"));

#[cfg(test)]
mod names;

#[cfg(test)]
mod tests {
    use super::names::{NAMES, read};

    fn channel(file: &str) -> std::collections::BTreeMap<&'static str, String> {
        read(&std::fs::read_to_string(format!("{}/channels/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }

    #[test]
    fn the_two_channels_share_no_name_so_their_installs_never_meet() {
        let (stable, dev) = (channel("stable.env"), channel("dev.env"));
        for name in NAMES {
            assert_ne!(stable[name], dev[name], "{name} is the same in both channels");
        }
    }

    #[test]
    fn a_file_missing_repeating_or_inventing_a_name_is_refused() {
        let whole = std::fs::read_to_string(format!("{}/channels/dev.env", env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(read(&whole).is_ok());
        let missing: String =
            whole.lines().filter(|line| !line.starts_with("CHANNEL_FEED=")).map(|l| format!("{l}\n")).collect();
        assert!(read(&missing).unwrap_err().contains("CHANNEL_FEED"));
        assert!(read(&format!("{whole}CHANNEL_ID=again\n")).unwrap_err().contains("twice"));
        assert!(read(&format!("{whole}CHANNEL_COLOUR=amber\n")).unwrap_err().contains("unknown"));
        assert!(read(&whole.replace("'Chatkord Dev'", "'Chatkord $HOME'")).is_err());
        assert!(
            read(&whole.replace("CHANNEL_ID=chatkord-dev", "CHANNEL_ID=Chatkord Dev"))
                .unwrap_err()
                .contains("character")
        );
    }

    #[test]
    fn this_build_is_the_channel_it_was_given() {
        let expected = match std::env::var("CHATKORD_CHANNEL").as_deref() {
            Ok("dev") => super::Channel::Dev,
            _ => super::Channel::Stable,
        };
        assert_eq!(super::THIS.channel, expected);
    }
}
