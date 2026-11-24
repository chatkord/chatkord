//! `ext-idle-notify-v1` on a connection of its own: the compositor tells when the seat has had no input for
//! [`IDLE_AFTER`] and when input resumes. Version 2's input-only notification is used where offered, so a video that
//! inhibits the screen's idling does not hold the person active.

use super::{IDLE_AFTER, State};
use crate::Unavailable;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::os::fd::OwnedFd;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use wayland_client::backend::WaylandError;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{self, ExtIdleNotificationV1};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

/// What the compositor has told and not yet been read.
#[derive(Default)]
struct Told {
    transitions: VecDeque<State>,
}

/// The transitions, read from the compositor's socket as it becomes readable.
pub(super) struct Feed {
    queue: EventQueue<Told>,
    told: Told,
    socket: AsyncFd<OwnedFd>,
    // Both live as long as the feed: dropping the notification ends it.
    _notification: ExtIdleNotificationV1,
    _notifier: ExtIdleNotifierV1,
}

impl Feed {
    /// Connects, binds the notifier and asks to be told; a compositor without the protocol is `Unavailable`.
    pub(super) async fn open() -> Result<Self, Unavailable> {
        let (connection, queue, notifier, notification) = crate::blocking(|| {
            let connection = Connection::connect_to_env().map_err(|_| Unavailable)?;
            let (globals, queue) = registry_queue_init::<Told>(&connection).map_err(|_| Unavailable)?;
            let qh = queue.handle();
            let seat: WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|_| Unavailable)?;
            let notifier: ExtIdleNotifierV1 = globals.bind(&qh, 1..=2, ()).map_err(|_| Unavailable)?;
            let millis = u32::try_from(IDLE_AFTER.as_millis()).map_err(|_| Unavailable)?;
            let notification = if notifier.version() >= 2 {
                notifier.get_input_idle_notification(millis, &seat, &qh, ())
            } else {
                notifier.get_idle_notification(millis, &seat, &qh, ())
            };
            connection.flush().map_err(|_| Unavailable)?;
            Ok((connection, queue, notifier, notification))
        })
        .await?;
        let backend = connection.backend();
        let socket = backend.poll_fd().try_clone_to_owned().map_err(|_| Unavailable)?;
        // SAFETY: `socket` is an `OwnedFd`, a valid open descriptor of our own duplicate that the `AsyncFd` now owns
        // and keeps open until it drops.
        let socket = unsafe { AsyncFd::register_with_interest(socket, Interest::READABLE) }.map_err(|_| Unavailable)?;
        Ok(Self { queue, told: Told::default(), socket, _notification: notification, _notifier: notifier })
    }

    /// The next transition; `None` once the compositor's connection has ended.
    pub(super) async fn next(&mut self) -> Option<State> {
        loop {
            if let Some(state) = self.told.transitions.pop_front() {
                return Some(state);
            }
            self.queue.dispatch_pending(&mut self.told).ok()?;
            if !self.told.transitions.is_empty() {
                continue;
            }
            self.queue.flush().ok()?;
            let mut ready = self.socket.readable().await.ok()?;
            if let Some(guard) = self.queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(gone)) if gone.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => return None,
                }
            }
            ready.clear_ready();
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Told {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Told {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for Told {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for Told {
    fn event(
        told: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => told.transitions.push_back(State::Idle),
            ext_idle_notification_v1::Event::Resumed => told.transitions.push_back(State::Active),
            _ => {}
        }
    }
}
