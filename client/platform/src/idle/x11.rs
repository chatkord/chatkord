//! The X server's screen-saver extension: milliseconds since the last input of any client.

use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::screensaver::ConnectionExt;
use x11rb::rust_connection::RustConnection;

/// The time since the last input, or `None` where the server cannot be reached or has no such extension.
pub(super) fn idle_for() -> Option<Duration> {
    let (connection, screen) = RustConnection::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let info = connection.screensaver_query_info(root).ok()?.reply().ok()?;
    Some(Duration::from_millis(u64::from(info.ms_since_user_input)))
}
