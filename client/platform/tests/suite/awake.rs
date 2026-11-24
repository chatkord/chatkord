//! The hold on the desktop's idle and suspend, taken and released through the Inhibit portal, as the desktop reports
//! what is inhibited: GNOME's session manager, or Plasma's power management, which names each hold by its reason.

use platform::awake::Hold;
use screens::Awake;
use std::time::Duration;
use tier::skipped;

/// The reason the hold is taken with here, by which Plasma's list names it.
const REASON: &str = "In a call";

/// The desktop that says what is inhibited.
#[derive(Clone, Copy)]
enum Desktop {
    /// GNOME's session manager, which says whether anything inhibits.
    Gnome,
    /// Plasma's power management, which lists each hold with its reason.
    Plasma,
}

/// What the session manager reports inhibited in `flag`: 4 is suspending, 8 is idling.
async fn inhibited(session: &zbus::Connection, flag: u32) -> zbus::Result<bool> {
    let manager =
        zbus::Proxy::new(session, "org.gnome.SessionManager", "/org/gnome/SessionManager", "org.gnome.SessionManager")
            .await?;
    manager.call("IsInhibited", &(flag,)).await
}

/// What Plasma holds for [`REASON`], as (suspend, idle): each hold is its kinds (`sleep:idle` for both), its app,
/// its reason, its mode and a cookie.
async fn ours(session: &zbus::Connection) -> zbus::Result<(bool, bool)> {
    let agent = zbus::Proxy::new(
        session,
        "org.kde.Solid.PowerManagement",
        "/org/kde/Solid/PowerManagement/PolicyAgent",
        "org.kde.Solid.PowerManagement.PolicyAgent",
    )
    .await?;
    let active: Vec<(String, String, String, String, u32)> = agent.get_property("ActiveInhibitions").await?;
    let held = |kind: &str| {
        active.iter().any(|(kinds, _, reason, _, _)| reason == REASON && kinds.split(':').any(|each| each == kind))
    };
    Ok((held("sleep"), held("idle")))
}

/// What the desktop reports held, as (suspend, idle).
async fn seen(session: &zbus::Connection, desktop: Desktop) -> zbus::Result<(bool, bool)> {
    match desktop {
        Desktop::Gnome => Ok((inhibited(session, 4).await?, inhibited(session, 8).await?)),
        Desktop::Plasma => ours(session).await,
    }
}

/// Waits until the desktop reports (suspend, idle) as `want`, or says what it reported last.
async fn becomes(session: &zbus::Connection, desktop: Desktop, want: (bool, bool)) -> Result<(), (bool, bool)> {
    let mut seen_last = (false, false);
    for _ in 0..100 {
        seen_last = seen(session, desktop).await.unwrap_or_default();
        if seen_last == want {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(seen_last)
}

mod lab {
    use super::*;

    #[tokio::test]
    async fn a_display_hold_stops_idle_and_suspend_a_system_hold_only_suspend_and_free_releases_both() {
        let Ok(session) = zbus::Connection::session().await else { return skipped("no session bus") };
        let desktop = if ours(&session).await.is_ok() {
            Desktop::Plasma
        } else if inhibited(&session, 4).await.is_ok() {
            Desktop::Gnome
        } else {
            return skipped("no session manager to ask what is inhibited");
        };
        if becomes(&session, desktop, (false, false)).await.is_err() {
            return skipped("something else holds the desktop awake");
        }
        let (hold, task) = Hold::new();
        let task = tokio::spawn(task);
        hold.keep(Awake::Display, REASON);
        assert_eq!(becomes(&session, desktop, (true, true)).await, Ok(()));
        hold.keep(Awake::System, REASON);
        assert_eq!(becomes(&session, desktop, (true, false)).await, Ok(()));
        hold.keep(Awake::Free, REASON);
        assert_eq!(becomes(&session, desktop, (false, false)).await, Ok(()));
        hold.keep(Awake::Display, REASON);
        assert_eq!(becomes(&session, desktop, (true, true)).await, Ok(()));
        drop(hold);
        assert!(task.await.unwrap().is_ok());
        assert_eq!(becomes(&session, desktop, (false, false)).await, Ok(()), "a dropped hold keeps nothing");
    }
}
