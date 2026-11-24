//! GNOME's idle monitor, for the session whose compositor does not offer `ext-idle-notify-v1`.

use crate::Unavailable;
use std::time::Duration;
use zbus::Connection;

#[zbus::proxy(
    interface = "org.gnome.Mutter.IdleMonitor",
    default_service = "org.gnome.Mutter.IdleMonitor",
    default_path = "/org/gnome/Mutter/IdleMonitor/Core"
)]
trait IdleMonitor {
    /// Milliseconds since the last input.
    fn get_idletime(&self) -> zbus::Result<u64>;
}

/// The shell's monitor of the core seat.
pub(super) struct Monitor {
    proxy: IdleMonitorProxy<'static>,
}

impl Monitor {
    /// The monitor, or `Unavailable` where no shell serves one.
    pub(super) async fn open() -> Result<Self, Unavailable> {
        let bus = Connection::session().await.map_err(|_| Unavailable)?;
        let proxy = IdleMonitorProxy::new(&bus).await.map_err(|_| Unavailable)?;
        Ok(Self { proxy })
    }

    pub(super) async fn idle_for(&self) -> Option<Duration> {
        self.proxy.get_idletime().await.ok().map(Duration::from_millis)
    }
}
