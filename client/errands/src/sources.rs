//! A screen chosen to share from a call: through the desktop's picker, or on Windows our own, whose sources are listed
//! with a still of each, again while it is open.

use crate::host::Host;
use crate::mailbox::Mailbox;
use choices::ShareQuality;
use screens::{PlaceId, PlaceMessage, SourceKey, WindowMessage};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Opens the desktop's picker and, for a screen picked, begins sending it from the call as `quality` says; a refusal is
/// a notice of the call, and a picker closed without a screen says nothing.
pub(crate) async fn choose_screen(
    id: PlaceId,
    commands: mpsc::Sender<call::room::Command>,
    quality: ShareQuality,
    mailbox: Mailbox,
) {
    let tell = |message: PlaceMessage| WindowMessage::At(id, message);
    match share::choose(share::Want::Either, share::Cursor::Embedded).await {
        Ok(selection) => {
            mailbox.tell(tell(PlaceMessage::Call(screens::CallMessage::Starting))).await;
            let pick = call::screen::Pick::Portal(selection);
            // A call that ended while the picker was open gives the permission back at once.
            // What was picked decides the sound, as the panel's switches allow.
            let sound = call::room::SoundAsked::Follow;
            if let Err(gone) = commands.send(call::room::Command::Share(pick, quality, sound)).await
                && let call::room::Command::Share(call::screen::Pick::Portal(selection), ..) = gone.0
            {
                selection.close().await;
            }
        }
        Err(share::Refused::Cancelled) => {}
        Err(refused) => {
            let said = words::failure::of(&call::failure::picked(refused));
            mailbox.tell(tell(PlaceMessage::Call(screens::CallMessage::Notice(screens::Subject::Picker, said)))).await
        }
    }
}

/// How often the picker's list and stills are made again while it is open: the canvas's "every few seconds".
#[cfg(windows)]
const REFRESH: std::time::Duration = std::time::Duration::from_secs(3);

/// The size a still fits inside: a card's picture on a dense screen.
#[cfg(windows)]
const STILL: (u32, u32) = (640, 360);

/// Lists the sources for the picker of `id` and tells the window, then again every [`REFRESH`], until aborted.
#[cfg(windows)]
pub(crate) fn watch(
    runtime: &tokio::runtime::Runtime,
    mailbox: Mailbox,
    id: PlaceId,
    host: Arc<impl Host>,
) -> tokio::task::AbortHandle {
    use crate::host::Still;
    use screens::Painting;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    crate::errand::spawn(runtime, report::word!("list the sources"), async move {
        // One device for every still of this picker, on the adapter that paints the window (the one a share is
        // on), made on the blocking pool as every device is.
        let adapter = host.painting().device().map(media::D3d12Device::adapter);
        let gpu = crate::errand::blocking(report::word!("open the picker device"), move || {
            media::Gpu::open_on(adapter, media::Validation::Off)
        })
        .await
        .ok()
        .flatten();
        let gpu = Arc::new(gpu);
        loop {
            let round = Arc::clone(&gpu);
            let listed = crate::errand::blocking(report::word!("list sources"), move || {
                let mut made = Vec::new();
                let mut keep = |width: u32, height: u32, rgba: Vec<u8>| {
                    let painting = Painting(NEXT.fetch_add(1, Ordering::Relaxed));
                    made.push(Still { painting, width, height, rgba });
                    painting
                };
                let sources: Vec<screens::Source> = share::candidates()
                    .into_iter()
                    .map(|candidate| {
                        let still = round.as_ref().as_ref().and_then(|gpu| {
                            let item = share::select(candidate.id, share::Cursor::Hidden).ok()?.item;
                            media::still(&item, gpu, STILL).ok()
                        });
                        screens::Source {
                            key: key(candidate.id),
                            kind: match candidate.kind {
                                share::Kind::Window => screens::SourceKind::Application,
                                share::Kind::Monitor | share::Kind::Virtual => screens::SourceKind::Screen,
                            },
                            title: candidate.title,
                            size: candidate.size,
                            icon: candidate.icon.map(|icon| keep(icon.side, icon.side, icon.rgba)),
                            still: still.map(|still| keep(still.width, still.height, still.rgba)),
                        }
                    })
                    .collect();
                (sources, made)
            })
            .await;
            let Ok((sources, made)) = listed else { return };
            host.hold_stills(made);
            mailbox.tell(WindowMessage::At(id, PlaceMessage::Call(screens::CallMessage::Sources(sources)))).await;
            tokio::time::sleep(REFRESH).await;
        }
    })
    .abort_handle()
}

/// No system but Windows has our own picker: nothing is listed.
#[cfg(not(windows))]
pub(crate) fn watch(
    runtime: &tokio::runtime::Runtime,
    _: Mailbox,
    _: PlaceId,
    _: Arc<impl Host>,
) -> tokio::task::AbortHandle {
    crate::errand::spawn(runtime, report::word!("watch nothing"), async {}).abort_handle()
}

/// Shares the source `key` names from the call `commands` runs, sent as `quality` says; a source gone since it was
/// listed is a notice of the call.
pub(crate) async fn share(
    id: PlaceId,
    key: SourceKey,
    quality: ShareQuality,
    commands: mpsc::Sender<call::room::Command>,
    mailbox: Mailbox,
) {
    let tell = |message: PlaceMessage| WindowMessage::At(id, message);
    match selected(&key).await {
        Ok(selection) => {
            mailbox.tell(tell(PlaceMessage::Call(screens::CallMessage::Starting))).await;
            let pick = call::screen::Pick::Portal(selection);
            let _ = commands.send(call::room::Command::Share(pick, quality, call::room::SoundAsked::Follow)).await;
        }
        Err(refused) => {
            let said = words::failure::of(&call::failure::picked(refused));
            mailbox.tell(tell(PlaceMessage::Call(screens::CallMessage::Notice(screens::Subject::Picker, said)))).await
        }
    }
}

#[cfg(windows)]
async fn selected(key: &SourceKey) -> Result<share::Selection, share::Refused> {
    let id = id_of(key).ok_or(share::Refused::Failed)?;
    crate::errand::blocking(report::word!("select a source"), move || share::select(id, share::Cursor::Embedded))
        .await
        .unwrap_or(Err(share::Refused::Failed))
}

#[cfg(not(windows))]
async fn selected(_: &SourceKey) -> Result<share::Selection, share::Refused> {
    Err(share::Refused::Unavailable)
}

/// The picker's name for a source: its kind and Windows' handle, which only this host reads back.
#[cfg(windows)]
fn key(id: share::Id) -> SourceKey {
    SourceKey(match id {
        share::Id::Monitor(handle) => format!("monitor:{handle}"),
        share::Id::Window(handle) => format!("window:{handle}"),
    })
}

#[cfg(windows)]
fn id_of(key: &SourceKey) -> Option<share::Id> {
    let (kind, handle) = key.0.split_once(':')?;
    let handle = handle.parse().ok()?;
    match kind {
        "monitor" => Some(share::Id::Monitor(handle)),
        "window" => Some(share::Id::Window(handle)),
        _ => None,
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_source_s_key_names_it_again() {
        for id in [share::Id::Monitor(65537), share::Id::Window(1312)] {
            assert_eq!(id_of(&key(id)), Some(id));
        }
        assert_eq!(id_of(&SourceKey("printer:3".into())), None);
    }
}
