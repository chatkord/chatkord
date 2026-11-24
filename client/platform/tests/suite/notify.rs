//! A popup shown and withdrawn through the desktop's notification service on the session bus.

use platform::notify::{self, Popup};
use tier::skipped;

const ADA: &str = "ada";

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_popup_is_shown_replaced_and_withdrawn() {
        let first = Popup::new(ADA, "Ada", "hello <b>there</b> & welcome").unwrap();
        let Ok(shown) = notify::show(first).await else { return skipped("no notification service on the session bus") };
        assert_eq!(shown.id, ADA);
        let again = Popup::new(ADA, "Ada", "hello again").unwrap();
        assert_eq!(notify::show(again).await.unwrap().id, ADA);
        assert!(notify::withdraw(ADA).await.is_ok(), "the popup is taken away");
        assert!(notify::withdraw(ADA).await.is_ok(), "a popup already gone is taken");
    }

    #[tokio::test]
    async fn the_clicks_are_heard_from_the_service() {
        let Ok(mut clicks) = notify::clicks().await else {
            return skipped("no notification service on the session bus");
        };
        let quiet = tokio::time::timeout(std::time::Duration::from_millis(300), clicks.next()).await;
        assert!(quiet.is_err(), "no click is told that no one made");
    }
}
