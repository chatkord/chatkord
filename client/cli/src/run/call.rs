//! Being in a voice channel from the terminal: speaking, listening, sharing a screen, and watching the screens shared.

use super::{Servers, Terminal, engine, joining, sign_in};
use crate::command::WatchTo;
use crate::record;
use call::room::{self, Command as Call, Join, Report, Source};
use call::screen;
use errands::signin;
use link::{Events, Link};
use platform::voice;
use screens::{CameraSent, RoomEvent, Route, Shared, Track};
use terms::Address;
use tokio::sync::mpsc;
use words::{Phrase, Sentence};

pub(super) async fn speak(
    address: Address,
    identity: String,
    channel_id: terms::ChannelId,
    tone: bool,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, mut events, ack, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let source = if tone { Source::Tone } else { Source::Microphone };
    let join = joining(channel_id, signin::me(&ack), source);
    in_call(&link, &mut events, join, None, terminal, servers.platform).await
}

pub(super) async fn listen(
    address: Address,
    identity: String,
    channel_id: terms::ChannelId,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, mut events, ack, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let join = joining(channel_id, signin::me(&ack), Source::Nothing);
    in_call(&link, &mut events, join, None, terminal, servers.platform).await
}

pub(super) async fn share(
    want: share::Want,
    sound: Vec<String>,
    address: Address,
    identity: String,
    channel_id: terms::ChannelId,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let sound = playing(&sound).await?;
    let (link, mut events, ack, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let picked = share::choose(want, share::Cursor::Embedded)
        .await
        .map_err(|refused| crate::failed(&::call::failure::picked(refused)))?;
    let me = signin::me(&ack);
    let join = joining(channel_id, me, Source::Nothing);
    in_call(&link, &mut events, join, Some((picked, sound)), terminal, servers.platform).await
}

pub(super) async fn watch(
    address: Address,
    identity: String,
    channel_id: terms::ChannelId,
    to: WatchTo,
    terminal: &mut dyn Terminal,
    servers: &Servers<'_>,
) -> Result<(), String> {
    let (link, events, ack, _, _) = sign_in(&address, &identity, terminal, servers).await?;
    let engine = engine().await?;
    match to {
        WatchTo::Reports => record::watch(&link, &engine, events, channel_id, signin::me(&ack), terminal).await,
        WatchTo::Stream(file) => record::watch_stream(&link, &engine, events, channel_id, file, terminal).await,
    }
}

/// The share's sound: the applications playing now that `names` name, each by its name or its program. One that plays
/// nothing is refused.
#[cfg(any(target_os = "linux", windows))]
async fn playing(names: &[String]) -> Result<room::SoundAsked, String> {
    if names.is_empty() {
        return named(Vec::new());
    }
    let unavailable = || crate::failed(&::call::failure::sound(::sound::share::SoundError::Unavailable));
    let apps = listed().await.ok_or_else(unavailable)?;
    let found = names
        .iter()
        .map(|name| {
            apps.iter()
                .find(|app| app.answers_to(name))
                .cloned()
                .ok_or_else(|| format!("No application called {name} is playing."))
        })
        .collect::<Result<_, _>>()?;
    named(found)
}

/// The applications playing now; none when the sound server would not say.
async fn listed() -> Option<Vec<::sound::share::App>> {
    ::sound::Sound::shared().apps().await.ok()
}

/// What the applications found are asked as: on Linux all of them, none being no sound.
#[cfg(target_os = "linux")]
fn named(found: Vec<::sound::share::App>) -> Result<room::SoundAsked, String> {
    Ok(room::SoundAsked::Apps(found))
}

/// On Windows one, heard as its process tree; none is what was picked has.
#[cfg(windows)]
fn named(found: Vec<::sound::share::App>) -> Result<room::SoundAsked, String> {
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (None, _) => Ok(room::SoundAsked::Follow),
        (Some(one), None) => Ok(room::SoundAsked::App(one)),
        (Some(_), Some(_)) => Err("Windows shares one application's sound at a time.".to_string()),
    }
}

/// Elsewhere the share's sound follows what is picked: no application is named for it.
#[cfg(not(any(target_os = "linux", windows)))]
async fn playing(names: &[String]) -> Result<room::SoundAsked, String> {
    match names {
        [] => Ok(room::SoundAsked::Follow),
        _ => Err("A share's sound is not available on this system yet.".to_string()),
    }
}

/// Joins a voice channel as `join` says and is in it until told to stop: hearing everyone there, sending its source,
/// sending the screen `picked` with its sound when there is one, and saying the loudness of each person heard every
/// second.
async fn in_call(
    link: &Link,
    events: &mut Events,
    join: Join,
    picked: Option<(share::Selection, room::SoundAsked)>,
    terminal: &mut dyn Terminal,
    platform: &platform::Platform,
) -> Result<(), String> {
    let source = join.source;
    let stop = terminal.stop();
    let (commands, queued) = mpsc::channel(8);
    let sharing = picked.is_some();
    let leaving = room::Leaving::default();
    let ended = leaving.clone();
    let report = |report: Report| match report {
        Report::Said(line) => terminal.say(&line),
        Report::Left(Ok(())) => terminal.say("Left the voice channel."),
        Report::Left(Err(said)) | Report::Notice(_, said) => terminal.say(&crate::english(&said)),
        Report::Room(RoomEvent::Joined { .. }) => terminal.say(match (source, sharing) {
            (_, true) => "Joined. Press Ctrl-C to stop sharing.",
            (Source::Nothing, false) => "Listening. Press Ctrl-C to stop.",
            (Source::Tone, false) => "Speaking a test tone. Press Ctrl-C to stop.",
            (Source::Microphone, false) => "Speaking. Press Ctrl-C to stop.",
        }),
        Report::Share(Shared::Started { width, height, encoder }) => {
            let by = match encoder {
                Route::Hardware { via } => via,
                Route::UntestedHardware { via } => format!("{via}, on an encoder nobody has tested with Chatkord"),
                Route::Software => "the processor".to_string(),
            };
            terminal.say(&format!("Sharing the screen, {width}x{height}, encoded through {by}."));
        }
        // A share that stopped ends this command: it is all it was for.
        Report::Share(Shared::Failed(failed)) => {
            terminal.say(&crate::failed(&failed));
            ended.leave();
        }
        Report::Share(Shared::Denied) => {
            terminal.say(&crate::english(&Sentence::Phrase(Phrase::ShareDenied)));
            ended.leave();
        }
        Report::Share(Shared::Sound(said)) => terminal.say(&said.line(&words::Words::english())),
        Report::Share(Shared::HandedOver(route)) => terminal.say(&crate::english(&route.handed_over())),
        Report::Camera(CameraSent::Started { width, height, encoder }) => {
            let encoded = screens::encoder_row(&encoder, &words::Words::english());
            terminal.say(&format!("Sending the camera, {width}x{height}, encoded: {encoded}."));
        }
        Report::Camera(CameraSent::Failed(failed)) => terminal.say(&crate::failed(&failed)),
        Report::Camera(CameraSent::Denied) => {
            terminal.say(&crate::english(&Sentence::Phrase(Phrase::CameraNotAllowed)))
        }
        Report::Camera(CameraSent::Stopped) => terminal.say("The camera is off."),
        Report::Room(RoomEvent::Levels(levels)) => {
            for (user_id, level) in levels {
                terminal.say(&format!("{}: level {level}", user_id.as_str()));
            }
        }
        Report::Room(RoomEvent::Stopped { user_id, track: Track::Microphone }) => {
            terminal.say(&format!("{} stopped speaking.", user_id.as_str()));
        }
        Report::Room(
            RoomEvent::MemberJoined { .. }
            | RoomEvent::MemberLeft { .. }
            | RoomEvent::Renamed { .. }
            | RoomEvent::Recoloured { .. }
            | RoomEvent::State { .. }
            | RoomEvent::Held { .. }
            | RoomEvent::Started { .. }
            | RoomEvent::Stopped { .. }
            | RoomEvent::Lanes { .. }
            | RoomEvent::Taking { .. }
            | RoomEvent::Ended(_),
        )
        | Report::Heard(_)
        | Report::Behind
        | Report::Answered(_)
        | Report::Connection(_)
        | Report::Sound(_)
        | Report::Share(Shared::Looked(_) | Shared::Stopped | Shared::Unshown)
        | Report::Camera(CameraSent::Looked(_))
        | Report::OwnWatched { .. }
        | Report::Watch(_) => {}
    };
    let chosen = voice::choice(platform).await;
    let join = Join { microphone: chosen.microphone, speaker: chosen.speaker, processing: chosen.processing, ..join };
    if let Some((selection, sound)) = picked {
        // As the window would start it: from the defaults kept in Settings.
        let quality = platform::share::quality(platform).await;
        let _ = commands.try_send(Call::Share(screen::Pick::Portal(selection), quality, sound));
    }
    // A device that comes or goes is the call's to settle; a machine that cannot say so is not told.
    if let Ok(changes) = ::sound::Sound::shared().watch().await {
        changes.forward(commands.clone(), || Call::DevicesChanged);
    }
    let engine = engine().await?;
    // `speak` speaks: the window's account bar and its volumes set how its own calls start, not this command's.
    let (_wants, wanted) = tokio::sync::watch::channel(room::Wants::default());
    let running = room::run(link, &engine, events, join, (wanted, queued), leaving.clone(), report);
    tokio::pin!(running);
    tokio::pin!(stop);
    tokio::select! {
        done = &mut running => return done.map_err(|stopped| crate::english(&stopped.sentence())),
        () = &mut stop => {}
    }
    leaving.leave();
    running.await.map_err(|stopped| crate::english(&stopped.sentence()))
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn windows_shares_one_named_application_and_refuses_a_second() {
        use super::{named, room::SoundAsked};
        use ::sound::share::App;
        let (game, player) = (App { name: "game".into(), root: 700 }, App { name: "player".into(), root: 800 });
        assert_eq!(named(Vec::new()), Ok(SoundAsked::Follow), "none named is what was picked has");
        assert_eq!(named(vec![game.clone()]), Ok(SoundAsked::App(game.clone())));
        assert_eq!(named(vec![game, player]), Err("Windows shares one application's sound at a time.".to_string()));
    }
}
