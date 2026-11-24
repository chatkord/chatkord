//! The freedesktop notification server over the session bus: `Notify` with the desktop entry and a default action,
//! `CloseNotification`, and the server's signals for a click.

use super::{Click, Popup, Shown, heard};
use crate::Unavailable;
use crate::activation::Activation;
use crate::launcher::DESKTOP_ENTRY;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::OnceCell;
use zbus::message::Type;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

/// The name the desktop shows a popup under.
const APP_NAME: &str = "Chatkord";
/// The action a click on the popup's body invokes.
const DEFAULT_ACTION: &str = "default";
/// What the default action is called, for a server that lists actions.
const DEFAULT_LABEL: &str = "Open";
/// `im.received`: a chat message, which a server may keep in its history or silence in a mode.
const CATEGORY: &str = "im.received";
const INTERFACE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[expect(clippy::too_many_arguments, reason = "the method's own signature on the bus")]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;
}

/// The connection, and which of our popups the server shows.
struct Service {
    proxy: NotificationsProxy<'static>,
    /// Whether the server reads the body as markup, so it must be escaped to be plain.
    markup: bool,
    /// The server's id for each popup of ours that may still be up.
    up: Mutex<HashMap<String, u32>>,
}

impl Service {
    fn up(&self) -> std::sync::MutexGuard<'_, HashMap<String, u32>> {
        self.up.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn service() -> Result<&'static Arc<Service>, Unavailable> {
    static SERVICE: OnceCell<Arc<Service>> = OnceCell::const_new();
    SERVICE
        .get_or_try_init(|| async {
            let bus = Connection::session().await.map_err(|_| Unavailable)?;
            let proxy = NotificationsProxy::new(&bus).await.map_err(|_| Unavailable)?;
            let capabilities = proxy.get_capabilities().await.map_err(|_| Unavailable)?;
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(INTERFACE)
                .and_then(|rule| rule.path(PATH))
                .map_err(|_| Unavailable)?
                .build();
            let signals = MessageStream::for_match_rule(rule, &bus, None).await.map_err(|_| Unavailable)?;
            let service = Arc::new(Service {
                proxy,
                markup: capabilities.iter().any(|capability| capability == "body-markup"),
                up: Mutex::new(HashMap::new()),
            });
            tokio::spawn(hear(Arc::clone(&service), bus, signals));
            Ok(service)
        })
        .await
}

/// Makes the service, so the clicks on popups made from now on are heard.
pub(super) async fn listen() -> Result<(), Unavailable> {
    service().await.map(|_| ())
}

pub(super) async fn show(popup: Popup) -> Result<Shown, Unavailable> {
    let service = service().await?;
    let body = if service.markup { escaped(popup.body()) } else { popup.body().to_owned() };
    // Plasma 6 updates a popup it has put in its history in place and shows nothing: the older is closed, and the
    // newer shown as new. One the person closed meanwhile is gone already.
    let older = service.up().remove(popup.id());
    if let Some(older) = older {
        let _ = service.proxy.close_notification(older).await;
    }
    // The window plays its own message sound, under its own switch, as peer apps do; the server's would be a second.
    let hints = HashMap::from([
        ("desktop-entry", Value::from(DESKTOP_ENTRY)),
        ("category", Value::from(CATEGORY)),
        ("suppress-sound", Value::from(true)),
    ]);
    let id = service
        .proxy
        .notify(APP_NAME, 0, DESKTOP_ENTRY, popup.title(), &body, &[DEFAULT_ACTION, DEFAULT_LABEL], hints, -1)
        .await
        .map_err(|_| Unavailable)?;
    service.up().insert(popup.id().to_owned(), id);
    Ok(Shown { id: popup.id().to_owned() })
}

pub(super) async fn withdraw(id: &str) -> Result<(), Unavailable> {
    let service = service().await?;
    let Some(server_id) = service.up().remove(id) else { return Ok(()) };
    match service.proxy.close_notification(server_id).await {
        Ok(()) => Ok(()),
        // A popup the person closed meanwhile is gone, which is what was asked.
        Err(zbus::Error::MethodError(..)) => Ok(()),
        Err(_) => Err(Unavailable),
    }
}

/// `text` with the characters a markup body reads as markup written as themselves.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// Tells each default-action click on a popup of ours, with the token the server sent just before it.
async fn hear(service: Arc<Service>, bus: Connection, mut signals: MessageStream) {
    let Ok(names) = zbus::fdo::DBusProxy::new(&bus).await else { return };
    let Ok(server) = zbus::names::BusName::try_from(INTERFACE) else { return };
    let mut token: Option<(u32, Activation)> = None;
    while let Some(Ok(message)) = signals.next().await {
        let header = message.header();
        // Only the server that owns the name speaks for it; any other client of the bus could emit the same signal.
        let owner = names.get_name_owner(server.clone()).await;
        let from_server = match (owner, header.sender()) {
            (Ok(owner), Some(sender)) => owner.as_str() == sender.as_str(),
            _ => false,
        };
        if !from_server {
            continue;
        }
        match header.member().map(|member| member.as_str()) {
            Some("ActivationToken") => {
                if let Ok((id, given)) = message.body().deserialize::<(u32, String)>() {
                    token = Activation::new(given).ok().map(|given| (id, given));
                }
            }
            Some("ActionInvoked") => {
                let Ok((id, action)) = message.body().deserialize::<(u32, String)>() else { continue };
                let given = token.take().filter(|(told, _)| *told == id).map(|(_, given)| given);
                let popup =
                    service.up().iter().find(|(_, server_id)| **server_id == id).map(|(popup, _)| popup.clone());
                if let (Some(popup), true) = (popup, action == DEFAULT_ACTION) {
                    // No one listening is a click no one wanted.
                    let _ = heard().send(Click { id: popup, activation: given });
                }
            }
            Some("NotificationClosed") => {
                if let Ok((id, _reason)) = message.body().deserialize::<(u32, u32)>() {
                    service.up().retain(|_, server_id| *server_id != id);
                }
            }
            _ => {}
        }
    }
}
