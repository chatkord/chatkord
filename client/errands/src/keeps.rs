//! What the window and its places keep for the next run, written one after another in the order asked, so two
//! changes to one setting end on the later, and a change is never written over by one asked before it.

use choices::{Audio, State, Volume};
use screens::Keep;
use terms::{CategoryId, MemberId, ServerIdentity, ServerUrl};
use tokio::sync::mpsc;

/// One thing kept.
pub(crate) enum Keeping {
    /// A choice of this device, one of the window's.
    Choice(Keep),
    /// How the next call starts.
    NextCall(State),
    /// Which server is open.
    Open(ServerUrl),
    /// How loud a member's sound plays at a server.
    Volume { server: ServerUrl, user_id: MemberId, audio: Audio, volume: Volume },
    /// Which categories of a server's channels are folded.
    Folded { server: ServerUrl, folded: std::collections::BTreeSet<CategoryId> },
    /// What a server says it is, kept beside its address.
    Server { server: ServerUrl, identity: ServerIdentity },
}

/// Writes what is asked, each once the one before it is done.
pub(crate) async fn keeps(mut asks: mpsc::UnboundedReceiver<Keeping>, platform: platform::Platform) {
    ordered(&mut asks, |kept| write(&platform, kept)).await;
}

/// Hands each ask to `write` in the order they came, the next only once the one before is done.
async fn ordered<F, Fut>(asks: &mut mpsc::UnboundedReceiver<Keeping>, mut write: F)
where
    F: FnMut(Keeping) -> Fut,
    Fut: Future<Output = ()>,
{
    while let Some(kept) = asks.recv().await {
        write(kept).await;
    }
}

async fn write(platform: &platform::Platform, kept: Keeping) {
    let _ = match kept {
        Keeping::Choice(keep) => match keep {
            Keep::Microphone(name) => platform::voice::use_microphone(platform, name).await,
            Keep::Speaker(name) => platform::voice::use_speaker(platform, name).await,
            Keep::Share(quality) => platform::share::keep(platform, quality).await,
            Keep::Look(look) => platform::look::keep(platform, look).await,
            Keep::EventSounds(sounds) => platform::voice::keep_event_sounds(platform, sounds).await,
            Keep::MessageSounds(sounds) => platform::voice::keep_message_sounds(platform, sounds).await,
            Keep::Popups(popups) => platform::voice::keep_popups(platform, popups).await,
            Keep::Processing(processing) => platform::voice::keep_processing(platform, processing).await,
            Keep::Startup(_) => Ok(()),
            Keep::Camera(settings) => platform::camera::keep(platform, settings).await,
            Keep::Language(language) => platform::language::keep(platform, language).await,
            Keep::AutoUpdate(auto_update) => platform::auto_update::keep(platform, auto_update).await,
            Keep::Emoji(picks) => platform::emoji::keep(platform, picks).await,
        },
        Keeping::NextCall(state) => platform::voice::keep_next_call(platform, state.mic_muted, state.deafened).await,
        Keeping::Open(url) => platform::state::remember_open(platform, url).await,
        Keeping::Volume { server, user_id, audio, volume } => {
            platform::volume::keep(platform, server, user_id, audio, volume).await
        }
        Keeping::Folded { server, folded } => platform::folds::keep(platform, server, folded).await,
        Keeping::Server { server, identity } => platform::state::remember_server(platform, server, identity).await,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn open(host: &str) -> Keeping {
        Keeping::Open(ServerUrl::parse(&format!("wss://{host}/v1")).expect("a url"))
    }

    fn host(kept: &Keeping) -> String {
        match kept {
            Keeping::Open(url) => url.as_str().to_owned(),
            _ => String::new(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn what_is_kept_is_written_in_the_order_it_was_asked_whatever_each_write_takes() {
        let (ask, mut asks) = mpsc::unbounded_channel();
        for place in ["a", "b", "c"] {
            ask.send(open(&format!("{place}.example.invalid"))).expect("open");
        }
        drop(ask);
        let written = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&written);
        // The first write takes longest: kept apart, the later two would have finished before it.
        let mut left = [300_u64, 200, 100].into_iter();
        ordered(&mut asks, |kept| {
            let (seen, took) = (Arc::clone(&seen), left.next().unwrap_or(0));
            async move {
                tokio::time::sleep(Duration::from_millis(took)).await;
                seen.lock().expect("a lock").push(host(&kept));
            }
        })
        .await;
        let written = written.lock().expect("a lock").clone();
        assert_eq!(written.len(), 3);
        assert!(
            written[0].contains("a.example") && written[1].contains("b.example") && written[2].contains("c.example")
        );
    }
}
