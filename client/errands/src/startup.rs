//! How the system starts Chatkord, kept and read one ask after another, so a read never answers from before a keep.

use crate::mailbox::Mailbox;
use screens::WindowMessage;
use tokio::sync::mpsc;

/// What is asked of how the system starts Chatkord.
pub(crate) enum StartupAsk {
    Keep(choices::Startup),
    Read,
}

/// Keeps and reads the system's start in the order asked, each read's answer told to the window.
pub(crate) async fn startup_asks(
    mut asks: mpsc::UnboundedReceiver<StartupAsk>,
    platform: platform::Platform,
    mailbox: Mailbox,
) {
    while let Some(ask) = asks.recv().await {
        match ask {
            StartupAsk::Keep(startup) => {
                let _ = platform::startup::keep(&platform, startup).await;
            }
            StartupAsk::Read => {
                if let Some(startup) = platform::startup::startup(&platform).await {
                    mailbox.tell(WindowMessage::StartupRead(startup)).await;
                }
            }
        }
    }
}
