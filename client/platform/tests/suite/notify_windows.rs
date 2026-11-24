//! A toast shown, replaced and withdrawn, read back from the history Windows keeps for the program's id.

use platform::notify::{self, APP_USER_MODEL_ID, Popup};
use std::time::Duration;
use tier::skipped;
use windows::UI::Notifications::ToastNotificationManager;
use windows::core::HSTRING;

const ADA: &str = "ada";

/// The text lines of each toast Windows holds under `id` for the program.
fn held(id: &str) -> Vec<Vec<String>> {
    let history = ToastNotificationManager::History().unwrap();
    let toasts = history.GetHistoryWithId(&HSTRING::from(APP_USER_MODEL_ID)).unwrap();
    toasts
        .into_iter()
        .filter(|toast| toast.Tag().unwrap() == id)
        .map(|toast| {
            let texts = toast.Content().unwrap().GetElementsByTagName(&HSTRING::from("text")).unwrap();
            (0..texts.Length().unwrap()).map(|at| texts.Item(at).unwrap().InnerText().unwrap().to_string()).collect()
        })
        .collect()
}

/// What Windows holds under `id` once it is `wanted`, or what it held after two seconds: the history follows a call.
async fn settled(id: &str, wanted: &[&[&str]]) -> Vec<Vec<String>> {
    for _ in 0..40 {
        if held(id) == wanted {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    held(id)
}

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_popup_is_shown_replaced_and_withdrawn() {
        let markup = "hello <b>there</b> & welcome";
        let first = Popup::new(ADA, "Ada", markup).unwrap();
        let Ok(shown) = notify::show(first).await else { return skipped("Windows shows no toast in this session") };
        assert_eq!(shown.id, ADA);
        assert_eq!(settled(ADA, &[&["Ada", markup]]).await, [["Ada", markup]], "the text is shown as it was given");
        let again = Popup::new(ADA, "Ada", "hello again").unwrap();
        assert_eq!(notify::show(again).await.unwrap().id, ADA);
        assert_eq!(settled(ADA, &[&["Ada", "hello again"]]).await, [["Ada", "hello again"]], "one toast, the newer");
        assert!(notify::withdraw(ADA).await.is_ok(), "the popup is taken away");
        assert!(settled(ADA, &[]).await.is_empty(), "Windows holds it no longer");
        assert!(notify::withdraw(ADA).await.is_ok(), "a popup already gone is taken");
    }

    #[tokio::test]
    async fn the_clicks_are_heard_and_none_is_told_that_no_one_made() {
        let Ok(mut clicks) = notify::clicks().await else { return skipped("Windows shows no toast in this session") };
        let quiet = tokio::time::timeout(Duration::from_millis(300), clicks.next()).await;
        assert!(quiet.is_err(), "no click is told that no one made");
    }
}
