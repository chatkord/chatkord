//! What this device keeps and has, asked of no server: its sound devices, how calls start, and its screens.

use super::Terminal;
use call::screen;
use platform::voice;
use webrtc::DevicesRefused;

const NOWHERE_TO_KEEP: &str = "This device has nowhere to keep that.";

#[derive(Clone, Copy)]
pub(super) enum Device {
    Microphone,
    Speaker,
}

/// Says the microphones and speakers this device has, each with the id it is chosen by, marking the ones chosen.
pub(super) async fn devices(terminal: &mut dyn Terminal, platform: &platform::Platform) -> Result<(), String> {
    let chosen = voice::choice(platform).await;
    let (inputs, outputs) = ::call::devices::listed().await.map_err(|failed| crate::failed(&failed))?;
    for (heading, found, kept) in [("Microphones:", inputs, chosen.microphone), ("Speakers:", outputs, chosen.speaker)]
    {
        terminal.say(heading);
        for device in found {
            let mark = if kept.as_ref().map(|kept| kept.id.as_str()) == device.asked_as() { " (chosen)" } else { "" };
            let shown = match device.asked_as() {
                None => format!("Default ({})", device.label()),
                Some(id) => format!("{} [{id}]", device.name),
            };
            terminal.say(&format!("  {shown}{mark}"));
        }
    }
    Ok(())
}

/// Keeps the device of the id `asked` (or the default for none) for calls, if this device has it.
pub(super) async fn use_device(
    asked: Option<String>,
    which: Device,
    terminal: &mut dyn Terminal,
    platform: &platform::Platform,
) -> Result<(), String> {
    let picked = match &asked {
        None => None,
        Some(wanted) => {
            let (inputs, outputs) = ::call::devices::listed().await.map_err(|failed| crate::failed(&failed))?;
            let found = match which {
                Device::Microphone => inputs,
                Device::Speaker => outputs,
            };
            let device = found
                .into_iter()
                .find(|device| device.asked_as() == Some(wanted.as_str()))
                .ok_or_else(|| crate::failed(&::call::failure::devices(&DevicesRefused::Missing)))?;
            Some(choices::Picked { id: wanted.clone(), label: device.label().to_string() })
        }
    };
    let kept = match which {
        Device::Microphone => voice::use_microphone(platform, picked).await,
        Device::Speaker => voice::use_speaker(platform, picked).await,
    };
    kept.map_err(|_| NOWHERE_TO_KEEP.to_string())?;
    terminal.say(if asked.is_some() { "Calls will use that device." } else { "Calls will use the default device." });
    Ok(())
}

/// Picks a screen and says what it is.
pub(super) async fn pick(want: share::Want, terminal: &mut dyn Terminal) -> Result<(), String> {
    let selection = share::choose(want, share::Cursor::Embedded)
        .await
        .map_err(|refused| crate::failed(&::call::failure::picked(refused)))?;
    let source = selection.source;
    let kind = match source.kind {
        Some(share::Kind::Monitor) => "monitor",
        Some(share::Kind::Window) => "window",
        Some(share::Kind::Virtual) => "virtual",
        None => "something",
    };
    let size = source.size.map(|(width, height)| format!(", {width}x{height}")).unwrap_or_default();
    #[cfg(target_os = "linux")]
    terminal.say(&format!("Picked a {kind}{size}, as PipeWire node {}.", source.node_id));
    #[cfg(not(target_os = "linux"))]
    terminal.say(&format!("Picked a {kind}{size}."));
    selection.close().await;
    Ok(())
}

/// Picks a screen and encodes a few seconds of it on the GPU, saying which encoder took it and what it cost.
#[cfg(target_os = "linux")]
pub(super) async fn capture(want: share::Want, terminal: &mut dyn Terminal) -> Result<(), String> {
    let selection = share::choose(want, share::Cursor::Embedded)
        .await
        .map_err(|refused| crate::failed(&::call::failure::picked(refused)))?;
    let node = selection.source.node_id;
    let remote = selection
        .remote
        .try_clone()
        .map_err(|_| crate::failed(&::call::failure::capture(capture::CaptureError::Connect)))?;
    let encoded = tokio::task::spawn_blocking(move || screen::encode(remote, node)).await;
    selection.close().await;
    let report = encoded
        .map_err(|_| crate::failed(&::call::failure::capture(capture::CaptureError::Stream)))?
        .map_err(|why| crate::failed(&::call::failure::screen(&why)))?;
    terminal.say(&report.line());
    Ok(())
}

/// The same, with the screen picked in our own picker.
#[cfg(windows)]
pub(super) async fn capture(want: share::Want, terminal: &mut dyn Terminal) -> Result<(), String> {
    let selection = share::choose(want, share::Cursor::Embedded)
        .await
        .map_err(|refused| crate::failed(&::call::failure::picked(refused)))?;
    let (item, refresh) = (selection.item.clone(), selection.refresh);
    let encoded = tokio::task::spawn_blocking(move || screen::encode(&item, refresh)).await;
    selection.close().await;
    let report = encoded
        .map_err(|_| crate::failed(&::call::failure::capture(capture::CaptureError::Stream)))?
        .map_err(|why| crate::failed(&::call::failure::screen(&why)))?;
    terminal.say(&report.line());
    Ok(())
}

/// A system with no capture has nothing to encode.
#[cfg(not(any(target_os = "linux", windows)))]
pub(super) async fn capture(_: share::Want, _: &mut dyn Terminal) -> Result<(), String> {
    Err(crate::failed(&::call::failure::picked(share::Refused::Unavailable)))
}
