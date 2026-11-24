//! The devices a person chose for calls, read back through the crate that keeps them. Each test runs in a
//! process of its own (nextest).

use crate::common;

use cli::{Command, run};
use common::Scripted;
use link::Trust;
use platform::{Platform, voice};

use tier::skipped;

fn keep(name: &str) -> Platform {
    let folder = std::env::temp_dir().join(format!("chatkord-devices-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    Platform::default().config_in(folder)
}

async fn command(platform: &Platform, command: Command, person: &mut Scripted) -> Result<(), String> {
    run(command, person, platform, &Trust::system()).await
}

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_device_this_machine_does_not_have_is_refused_and_not_kept() {
        let platform = keep("missing");
        let mut person = Scripted::default();
        let refused = command(&platform, Command::UseMicrophone(Some("No such microphone".into())), &mut person).await;
        match refused {
            Err(sentence)
                if sentence
                    == words::Words::english()
                        .say(&words::failure::of(&call::failure::devices(&webrtc::DevicesRefused::Unavailable))) =>
            {
                return skipped("sound cannot start on this machine");
            }
            other => assert_eq!(
                other,
                Err(words::Words::english()
                    .say(&words::failure::of(&call::failure::devices(&webrtc::DevicesRefused::Missing))))
            ),
        }
        assert_eq!(voice::choice(&platform).await.microphone, None);
    }

    #[tokio::test]
    async fn a_listed_device_is_kept_marked_in_the_list_and_the_default_takes_it_back() {
        let platform = keep("listed");
        let mut person = Scripted::default();
        if let Err(sentence) = command(&platform, Command::Devices, &mut person).await {
            return skipped(&sentence);
        }
        let Some(at) = person.said.iter().position(|line| line == "Microphones:") else { panic!("no heading") };
        let listed: Vec<&String> = person.said[at + 1..].iter().take_while(|line| line.starts_with("  ")).collect();
        let Some(first) = listed.iter().find(|line| !line.trim_start().starts_with("Default")) else {
            return skipped("no microphone on this machine besides the default");
        };
        let shown = first.trim().to_string();
        let id = shown.rsplit_once(" [").and_then(|(_, id)| id.strip_suffix(']')).expect("listed with its id");

        command(&platform, Command::UseMicrophone(Some(id.to_string())), &mut person).await.expect("kept");
        assert_eq!(voice::choice(&platform).await.microphone.map(|kept| kept.id), Some(id.to_string()));
        person.said.clear();
        command(&platform, Command::Devices, &mut person).await.expect("listed");
        assert!(person.heard(&format!("{shown} (chosen)")), "the list marks it: {:?}", person.said);

        command(&platform, Command::UseMicrophone(None), &mut person).await.expect("kept");
        assert_eq!(voice::choice(&platform).await.microphone, None, "the default takes it back");
    }

    #[tokio::test]
    async fn the_system_default_is_listed_as_default_chosen_by_no_name_and_never_kept_by_the_modules_own() {
        let platform = keep("default");
        let mut person = Scripted::default();
        if let Err(sentence) = command(&platform, Command::Devices, &mut person).await {
            return skipped(&sentence);
        }
        let Some(at) = person.said.iter().position(|line| line == "Microphones:") else { panic!("no heading") };
        let Some(first) = person.said.get(at + 1).map(|line| line.trim().to_string()) else {
            return skipped("no microphone on this machine");
        };
        if !cfg!(target_os = "linux") {
            return skipped("only Linux's sound module lists the default first");
        }
        let Some(followed) = first.strip_prefix("Default (").and_then(|rest| rest.split(')').next()) else {
            panic!("the first microphone is the default and is said so: {first}");
        };
        assert!(first.ends_with("(chosen)"), "no name kept means the default is the chosen one: {first}");

        // The module calls the entry "default: <device>", which stops being right when the system's default changes.
        let raw = format!("default: {followed}");
        let refused = command(&platform, Command::UseMicrophone(Some(raw)), &mut person).await;
        assert_eq!(
            refused,
            Err(words::Words::english()
                .say(&words::failure::of(&call::failure::devices(&webrtc::DevicesRefused::Missing))))
        );
        assert_eq!(voice::choice(&platform).await.microphone, None, "nothing was kept");
    }
}
