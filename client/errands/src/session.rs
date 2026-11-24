//! A signed-in connection, and one kept: its events told as they come, or lent whole for a while (a call).

use link::{Events, Link};
use proto::chatkord_v1::LoginAck;
use std::future::Future;
use terms::Heard;
use tokio::sync::{mpsc, oneshot};

/// A connection that is signed in.
pub struct Session {
    /// The connection, for asks.
    pub link: Link,
    /// What the server tells it unasked.
    pub events: Events,
    /// The server's answer to the sign-in.
    pub ack: LoginAck,
}

/// A signed-in connection kept: its link for asks, and a listener that tells its events or lends them whole for a
/// while (a call); it ends with the last clone, or with the connection after telling its end.
#[derive(Clone)]
pub struct Kept {
    /// The connection, for asks.
    pub link: Link,
    /// The server's answer to the sign-in.
    pub ack: LoginAck,
    lend: mpsc::Sender<oneshot::Sender<Lent>>,
}

/// A kept connection's events, lent: sent `back` when done, after which they are told again. Dropping `back` ends the
/// listener, and the end of the connection is then the borrower's to tell.
pub struct Lent {
    /// The events, the borrower's alone until given back.
    pub events: Events,
    /// Where they are given back.
    pub back: oneshot::Sender<Events>,
}

impl Kept {
    /// Keeps `session`, telling `told` everything its events say, in order, until it is dropped or ends.
    pub fn listen<F, Told>(session: Session, told: F) -> Kept
    where
        F: FnMut(Heard) -> Told + Send + 'static,
        Told: Future<Output = ()> + Send + 'static,
    {
        let (lend, asked) = mpsc::channel(1);
        crate::errand::spawn_here(report::word!("tell a connection"), listen(session.events, asked, told));
        Kept { link: session.link, ack: session.ack, lend }
    }

    /// The events, lent; none once the listener has ended.
    pub async fn lend(&self) -> Option<Lent> {
        let (given, lent) = oneshot::channel();
        self.lend.send(given).await.ok()?;
        lent.await.ok()
    }
}

async fn listen<F, Told>(mut events: Events, mut asked: mpsc::Receiver<oneshot::Sender<Lent>>, mut told: F)
where
    F: FnMut(Heard) -> Told,
    Told: Future<Output = ()>,
{
    loop {
        tokio::select! {
            // A dropped `Kept` is seen before anything more is told.
            biased;
            lend = asked.recv() => {
                let Some(given) = lend else { return };
                let (back, returned) = oneshot::channel();
                if let Err(unwanted) = given.send(Lent { events, back }) {
                    events = unwanted.events;
                    continue;
                }
                match returned.await {
                    Ok(given_back) => events = given_back,
                    Err(_) => return,
                }
            }
            heard = events.next() => match heard {
                Some(Heard::Ended(ended)) => return told(Heard::Ended(ended)).await,
                Some(heard) => told(heard).await,
                None => return,
            },
        }
    }
}
