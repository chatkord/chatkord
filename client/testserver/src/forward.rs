//! The relay's forwarding, run in its own process (`chatkord-test-relay`): between the clients and a server's media
//! port, so a test can break a call's media alone, the WebSocket working on while every datagram is dropped. It
//! forwards as a NAT does, one upstream socket per client address, and the server, being ICE-lite, answers whichever
//! address its checks came from.

use crate::impair::{Impaired, Impairing, Losing, Loss, Lost, Schedule, Verdict, Way};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::task::JoinSet;

/// The largest datagram carried: past any media packet on a loopback MTU.
const DATAGRAM: usize = 65_536;

/// Datagrams carried between the clients and the server until dropped; [`Forwarder::cut`] drops them both ways.
pub struct Forwarder {
    downstream: SocketAddr,
    ways: Arc<Ways>,
    _tasks: JoinSet<()>,
}

/// What the relay does to what it carries, read by every client's path.
#[derive(Default)]
struct Ways {
    cut: AtomicBool,
    impairing: Mutex<Option<Impairing>>,
    losing: Mutex<Option<Losing>>,
}

impl Ways {
    fn verdict(&self, way: Way, datagram: &[u8]) -> Verdict {
        if self.cut.load(Ordering::Relaxed) {
            return Verdict::Drop;
        }
        let mut losing = self.losing.lock().unwrap_or_else(PoisonError::into_inner);
        if losing.as_mut().is_some_and(|losing| losing.verdict(way, datagram) == Verdict::Drop) {
            return Verdict::Drop;
        }
        drop(losing);
        if way == Way::Up {
            return Verdict::Pass;
        }
        let mut impairing = self.impairing.lock().unwrap_or_else(PoisonError::into_inner);
        impairing.as_mut().map_or(Verdict::Pass, |impairing| impairing.verdict(datagram, Instant::now()))
    }
}

impl Forwarder {
    /// A relay the clients reach at `ip`, on a port the system picks, that carries their datagrams to the same port at
    /// `upstream`: a server told that port listens there and announces the relay.
    pub async fn new(ip: IpAddr, upstream: IpAddr) -> Self {
        let downstream = Arc::new(UdpSocket::bind((ip, 0)).await.expect("the relay's port"));
        let address = downstream.local_addr().expect("its address");
        let upstream = SocketAddr::new(upstream, address.port());
        let ways = Arc::new(Ways::default());
        let mut tasks = JoinSet::new();
        tasks.spawn(carry(downstream, upstream, Arc::clone(&ways)));
        Self { downstream: address, ways, _tasks: tasks }
    }

    /// Where the clients send: the address the server announces.
    pub fn address(&self) -> SocketAddr {
        self.downstream
    }

    /// Drops every datagram, both ways, until [`Forwarder::mend`].
    pub fn cut(&self) {
        self.ways.cut.store(true, Ordering::Relaxed);
    }

    /// Carries datagrams again.
    pub fn mend(&self) {
        self.ways.cut.store(false, Ordering::Relaxed);
    }

    /// Impairs the server's video on its way down from now on, as `schedule` says.
    pub fn impair(&self, schedule: Schedule) {
        *self.ways.impairing.lock().unwrap_or_else(PoisonError::into_inner) = Some(Impairing::new(schedule));
    }

    /// Loses RTP by chance from now on, as `loss` says, beside any schedule.
    pub fn lose(&self, loss: Loss) {
        *self.ways.losing.lock().unwrap_or_else(PoisonError::into_inner) = Some(Losing::new(loss));
    }

    /// What the loss has done so far; nothing without one.
    pub fn lost(&self) -> Lost {
        let losing = self.ways.losing.lock().unwrap_or_else(PoisonError::into_inner);
        losing.as_ref().map(Losing::done).unwrap_or_default()
    }

    /// What the schedule has done so far; nothing without one.
    pub fn impaired(&self) -> Impaired {
        let impairing = self.ways.impairing.lock().unwrap_or_else(PoisonError::into_inner);
        impairing.as_ref().map(Impairing::done).unwrap_or_default()
    }
}

/// Carries each client's datagrams up through a socket of its own, and the answers back down, until the relay drops.
async fn carry(downstream: Arc<UdpSocket>, upstream: SocketAddr, ways: Arc<Ways>) {
    let mut clients: HashMap<SocketAddr, Arc<UdpSocket>> = HashMap::new();
    let mut answers = JoinSet::new();
    let mut datagram = vec![0; DATAGRAM];
    while let Ok((length, client)) = downstream.recv_from(&mut datagram).await {
        let up = match clients.get(&client) {
            Some(up) => Arc::clone(up),
            None => {
                let Ok(up) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await else { continue };
                if up.connect(upstream).await.is_err() {
                    continue;
                }
                let up = Arc::new(up);
                answers.spawn(answer(Arc::clone(&up), Arc::clone(&downstream), client, Arc::clone(&ways)));
                clients.insert(client, Arc::clone(&up));
                up
            }
        };
        if ways.verdict(Way::Up, &datagram[..length]) == Verdict::Pass {
            let _ = up.send(&datagram[..length]).await;
        }
    }
}

/// Carries the server's answers to one client back down, each as the relay's ways say.
async fn answer(up: Arc<UdpSocket>, downstream: Arc<UdpSocket>, client: SocketAddr, ways: Arc<Ways>) {
    let mut datagram = vec![0; DATAGRAM];
    let mut held = JoinSet::new();
    while let Ok(length) = up.recv(&mut datagram).await {
        let packet = &datagram[..length];
        match ways.verdict(Way::Down, packet) {
            Verdict::Pass => {
                let _ = downstream.send_to(packet, client).await;
            }
            Verdict::Drop => {}
            Verdict::Hold(until) => {
                let (downstream, packet) = (Arc::clone(&downstream), packet.to_vec());
                held.spawn(async move {
                    tokio::time::sleep_until(until.into()).await;
                    let _ = downstream.send_to(&packet, client).await;
                });
            }
        }
        while held.try_join_next().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    const WAIT: Duration = Duration::from_secs(2);

    /// Another loopback address, where the server stands in these tests.
    const SERVER: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

    /// A server at `SERVER` on `port` that sends back what it is sent, from the address it was sent to.
    async fn echo(port: u16) {
        let socket = UdpSocket::bind((SERVER, port)).await.expect("bound");
        tokio::spawn(async move {
            let mut datagram = vec![0; DATAGRAM];
            while let Ok((length, from)) = socket.recv_from(&mut datagram).await {
                let _ = socket.send_to(&datagram[..length], from).await;
            }
        });
    }

    async fn round_trip(client: &UdpSocket, relay: &Forwarder, said: &[u8]) -> Option<Vec<u8>> {
        client.send_to(said, relay.address()).await.expect("sent");
        let mut heard = vec![0; DATAGRAM];
        let (length, from) = timeout(WAIT, client.recv_from(&mut heard)).await.ok()?.ok()?;
        assert_eq!(from, relay.address(), "the answer comes from where the client sent");
        Some(heard[..length].to_vec())
    }

    #[tokio::test]
    async fn the_relay_carries_both_ways_drops_everything_while_cut_and_carries_again_once_mended() {
        let relay = Forwarder::new(IpAddr::V4(Ipv4Addr::LOCALHOST), SERVER).await;
        echo(relay.address().port()).await;
        let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bound");
        assert_eq!(round_trip(&client, &relay, b"one").await.as_deref(), Some(&b"one"[..]));
        relay.cut();
        assert_eq!(round_trip(&client, &relay, b"two").await, None, "nothing crosses a cut relay");
        relay.mend();
        assert_eq!(round_trip(&client, &relay, b"three").await.as_deref(), Some(&b"three"[..]));
    }
}
