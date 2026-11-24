//! Unity's `LauncherEntry` on the session bus. A dock that reads it (Ubuntu's, Plasma's task manager) listens for its
//! `Update` signal from any sender, files the count under the sender's name and drops it when that name leaves the
//! bus; it owns `com.canonical.Unity` while it runs and asks nothing when it starts, so the entry tells the count again
//! whenever that name gains an owner. `Query` answers what libunity's entry does: the properties shown now.

use crate::Unavailable;
use futures_util::StreamExt;
use std::collections::HashMap;
use tokio::sync::watch;
use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;

/// The name a dock that reads launcher entries owns while it runs.
const DOCK: &str = "com.canonical.Unity";
/// Where the entry is served.
const PATH: &str = "/com/chatkord/LauncherEntry";

/// The entry, which answers from the count last told.
struct Entry {
    counts: watch::Receiver<Option<u32>>,
}

#[zbus::interface(name = "com.canonical.Unity.LauncherEntry")]
impl Entry {
    /// The properties shown now.
    fn query(&self) -> HashMap<&'static str, Value<'static>> {
        properties(*self.counts.borrow())
    }

    /// The properties of the entry `app_uri` names changed.
    #[zbus(signal)]
    async fn update(
        emitter: &SignalEmitter<'_>,
        app_uri: &str,
        properties: HashMap<&'static str, Value<'static>>,
    ) -> zbus::Result<()>;
}

/// The entry served on a connection of its own, telling each count `counts` holds and telling it again to each dock
/// that comes onto the bus, until the sender is dropped; the connection goes with it, and the docks drop the count.
pub(super) async fn serve(mut counts: watch::Receiver<Option<u32>>) -> Result<(), Unavailable> {
    let bus = Connection::session().await.map_err(|_| Unavailable)?;
    let names = zbus::fdo::DBusProxy::new(&bus).await.map_err(|_| Unavailable)?;
    let docks = names.receive_name_owner_changed_with_args(&[(0, DOCK)]).await.map_err(|_| Unavailable)?;
    let mut docks = std::pin::pin!(docks);
    bus.object_server().at(PATH, Entry { counts: counts.clone() }).await.map_err(|_| Unavailable)?;
    let emitter = SignalEmitter::new(&bus, PATH).map_err(|_| Unavailable)?;
    let uri = app_uri();
    loop {
        let shown = *counts.borrow_and_update();
        Entry::update(&emitter, &uri, properties(shown)).await.map_err(|_| Unavailable)?;
        loop {
            tokio::select! {
                changed = counts.changed() => match changed {
                    Ok(()) => break,
                    Err(_) => return Ok(()),
                },
                dock = docks.next() => match dock {
                    Some(dock) if dock.args().is_ok_and(|args| args.new_owner().is_some()) => break,
                    Some(_) => {}
                    None => return Err(Unavailable),
                },
            }
        }
    }
}

/// The URI the dock knows this program's entry by.
pub(super) fn app_uri() -> String {
    format!("application://{}.desktop", super::DESKTOP_ENTRY)
}

/// The entry's properties for `count`: the number, and whether one is shown.
pub(super) fn properties(count: Option<u32>) -> HashMap<&'static str, Value<'static>> {
    HashMap::from([
        ("count", Value::from(i64::from(count.unwrap_or_default()))),
        ("count-visible", Value::from(count.is_some())),
    ])
}
