//! A Windows toast: shown for this program's explicit AppUserModelID, which the Start Menu shortcut carries too.
//! No apartment is entered: `windows` keeps the process in the multithreaded one from the first call that finds none,
//! where one entered on a pool thread would end with that thread and take the click handlers with it.

use super::{APP_USER_MODEL_ID, Click, Popup, Shown, heard};
use crate::Unavailable;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastDismissalReason, ToastDismissedEventArgs, ToastNotification, ToastNotificationManager, ToastTemplateType,
};
use windows::core::{HSTRING, IInspectable};

/// The group every toast of ours is filed under, so one is found by its tag within it.
const GROUP: &str = channel::THIS.id;

/// The toasts that may still be clicked: a toast activates only through the object that showed it.
fn live() -> MutexGuard<'static, HashMap<String, ToastNotification>> {
    static LIVE: OnceLock<Mutex<HashMap<String, ToastNotification>>> = OnceLock::new();
    LIVE.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn show(popup: &Popup) -> Result<Shown, Unavailable> {
    made(popup).map_err(|_| Unavailable)?;
    Ok(Shown { id: popup.id().to_owned() })
}

/// The toast, text set through the document so it is plain whatever it holds, and the notifier showing it.
fn made(popup: &Popup) -> windows::core::Result<()> {
    let xml: XmlDocument = ToastNotificationManager::GetTemplateContent(ToastTemplateType::ToastText02)?;
    let texts = xml.GetElementsByTagName(&HSTRING::from("text"))?;
    texts.Item(0)?.SetInnerText(&HSTRING::from(popup.title()))?;
    texts.Item(1)?.SetInnerText(&HSTRING::from(popup.body()))?;
    // The window plays its own message sound, under its own switch, as peer apps do; the system's would be a second.
    let audio = xml.CreateElement(&HSTRING::from("audio"))?;
    audio.SetAttribute(&HSTRING::from("silent"), &HSTRING::from("true"))?;
    xml.DocumentElement()?.AppendChild(&audio)?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.SetTag(&HSTRING::from(popup.id()))?;
    toast.SetGroup(&HSTRING::from(GROUP))?;
    let clicked = popup.id().to_owned();
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
        // No one listening is a click no one wanted.
        let _ = heard().send(Click { id: clicked.clone(), activation: None });
        Ok(())
    }))?;
    let closed = popup.id().to_owned();
    toast.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(move |_, args| {
        // A banner that timed out stays in the action center, where it can still be clicked.
        let timed_out = args.as_ref().and_then(|args| args.Reason().ok()) == Some(ToastDismissalReason::TimedOut);
        if !timed_out {
            live().remove(&closed);
        }
        Ok(())
    }))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))?.Show(&toast)?;
    live().insert(popup.id().to_owned(), toast);
    Ok(())
}

pub(super) fn withdraw(id: &str) -> Result<(), Unavailable> {
    live().remove(id);
    ToastNotificationManager::History()
        .and_then(|history| {
            history.RemoveGroupedTagWithId(&HSTRING::from(id), &HSTRING::from(GROUP), &HSTRING::from(APP_USER_MODEL_ID))
        })
        .map_err(|_| Unavailable)
}
