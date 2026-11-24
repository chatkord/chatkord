//! A popup in the desktop's notification area and the click on it: the freedesktop notification server on Linux, a
//! Windows toast on Windows. What a popup says and when one is shown is decided by the caller; the text it is given is
//! shown as plain text and is bounded where the popup is made.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use crate::Unavailable;
use crate::activation::Activation;
use std::fmt;
use std::sync::OnceLock;
use tokio::sync::{broadcast, mpsc};

/// The longest id a popup takes, in characters; Windows keeps a toast's tag to 64.
pub const ID_MAX: usize = 64;
/// The longest title a popup takes, in characters.
pub const TITLE_MAX: usize = 80;
/// The longest body a popup takes, in characters: a notification area shows a few lines.
pub const BODY_MAX: usize = 240;

/// The name Windows files this program's toasts and taskbar button under, which the process takes as it starts
/// ([`crate::launcher::keep`]); the installer's Start Menu shortcut carries the same one (`AppUserModelID` in
/// `scripts/package/windows.iss`), which gives a toast the program's name and icon.
#[cfg(windows)]
pub const APP_USER_MODEL_ID: &str = channel::THIS.app_user_model_id;

/// Why a popup was not made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Refused {
    /// The id is empty or over [`ID_MAX`] characters.
    Id,
    /// The title is over [`TITLE_MAX`] characters.
    Title,
    /// The body is over [`BODY_MAX`] characters.
    Body,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Id => write!(f, "a popup's id is empty or over {ID_MAX} characters"),
            Refused::Title => write!(f, "a popup's title is over {TITLE_MAX} characters"),
            Refused::Body => write!(f, "a popup's body is over {BODY_MAX} characters"),
        }
    }
}

impl std::error::Error for Refused {}

/// One popup. A popup shown under an id that is still up replaces it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Popup {
    id: String,
    title: String,
    body: String,
}

impl Popup {
    /// A popup the caller names by `id`, which a click on it and [`withdraw`] give back. Text over its bound is
    /// refused, never cut.
    pub fn new(id: impl Into<String>, title: impl Into<String>, body: impl Into<String>) -> Result<Self, Refused> {
        let (id, title, body) = (id.into(), title.into(), body.into());
        if id.is_empty() || id.chars().count() > ID_MAX {
            return Err(Refused::Id);
        }
        if title.chars().count() > TITLE_MAX {
            return Err(Refused::Title);
        }
        if body.chars().count() > BODY_MAX {
            return Err(Refused::Body);
        }
        Ok(Self { id, title, body })
    }

    /// The id the caller named it by.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The title, plain text.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The body, plain text.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// A popup the desktop took, shown or queued by it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Shown {
    /// The id of the popup, as it was named.
    pub id: String,
}

/// A click on a popup of ours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Click {
    /// The id of the popup clicked.
    pub id: String,
    /// The token to take the focus with, where the desktop gives one.
    pub activation: Option<Activation>,
}

/// The clicks on this process's popups, from the time it was made.
pub struct Clicks {
    heard: broadcast::Receiver<Click>,
}

impl Clicks {
    /// The next click, or `None` once the desktop's notification service has gone for good.
    pub async fn next(&mut self) -> Option<Click> {
        loop {
            match self.heard.recv().await {
                Ok(click) => return Some(click),
                // A slow reader lost the oldest clicks; the ones after them are still wanted.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Sends what `wrap` makes of each click to `to`, until `to` is closed.
    pub fn forward<T: Send + 'static>(mut self, to: mpsc::Sender<T>, wrap: impl Fn(Click) -> T + Send + 'static) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    click = self.next() => {
                        let Some(click) = click else { break };
                        if to.send(wrap(click)).await.is_err() {
                            break;
                        }
                    }
                    () = to.closed() => break,
                }
            }
        });
    }
}

/// Where the system's click handlers speak; one for the process.
fn heard() -> &'static broadcast::Sender<Click> {
    static HEARD: OnceLock<broadcast::Sender<Click>> = OnceLock::new();
    HEARD.get_or_init(|| broadcast::channel(16).0)
}

/// Shows `popup`. A desktop with no notification service, or a system with none here yet, is [`Unavailable`].
pub async fn show(popup: Popup) -> Result<Shown, Unavailable> {
    #[cfg(target_os = "linux")]
    {
        linux::show(popup).await
    }
    #[cfg(windows)]
    {
        crate::blocking(move || windows::show(&popup)).await
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = popup;
        Err(Unavailable)
    }
}

/// Takes the popup `id` away, from the screen or from the notification area; one that is gone already is taken.
pub async fn withdraw(id: &str) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        linux::withdraw(id).await
    }
    #[cfg(windows)]
    {
        let id = id.to_owned();
        crate::blocking(move || windows::withdraw(&id)).await
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = id;
        Err(Unavailable)
    }
}

/// Starts hearing the clicks on popups: those made after this returns are told.
pub async fn clicks() -> Result<Clicks, Unavailable> {
    #[cfg(target_os = "linux")]
    linux::listen().await?;
    #[cfg(any(target_os = "linux", windows))]
    {
        Ok(Clicks { heard: heard().subscribe() })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "ada";

    #[test]
    fn a_popup_within_its_bounds_is_made() {
        let popup = Popup::new(ADA, "t".repeat(TITLE_MAX), "b".repeat(BODY_MAX)).unwrap();
        assert_eq!(popup.id(), ADA);
        assert_eq!(popup.title().chars().count(), TITLE_MAX);
        assert_eq!(popup.body().chars().count(), BODY_MAX);
    }

    #[test]
    fn text_over_a_bound_is_refused_not_cut() {
        assert_eq!(Popup::new("", "t", "b"), Err(Refused::Id));
        assert_eq!(Popup::new("i".repeat(ID_MAX + 1), "t", "b"), Err(Refused::Id));
        assert_eq!(Popup::new(ADA, "t".repeat(TITLE_MAX + 1), "b"), Err(Refused::Title));
        assert_eq!(Popup::new(ADA, "t", "b".repeat(BODY_MAX + 1)), Err(Refused::Body));
    }

    #[test]
    fn bounds_count_characters_not_bytes() {
        assert!(Popup::new(ADA, "t", "é".repeat(BODY_MAX)).is_ok());
    }

    #[test]
    fn an_activation_token_is_never_printed() {
        let click = Click { id: ADA.into(), activation: Activation::new("secret-token".into()).ok() };
        assert!(!format!("{click:?}").contains("secret-token"));
    }

    #[tokio::test]
    async fn a_forwarded_click_reaches_the_channel() {
        let (told, mut got) = mpsc::channel(1);
        let (sender, heard) = broadcast::channel(4);
        Clicks { heard }.forward(told, |click| click.id);
        sender.send(Click { id: ADA.into(), activation: None }).unwrap();
        assert_eq!(got.recv().await.as_deref(), Some(ADA));
    }
}
