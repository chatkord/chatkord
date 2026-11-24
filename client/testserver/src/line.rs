//! A line between one client and a server's socket on this machine, which a test cuts and mends: that client's
//! connection drops and its reconnects are refused until mended, while every other client of the server keeps its own.

use crate::Server;
use std::net::{Ipv4Addr, SocketAddr, TcpListener as Bound};
use std::thread::JoinHandle;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// What the line does with a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Carries each one to the server.
    Carrying,
    /// Closes each one, new and open.
    Cut,
    /// Ends, its thread with it.
    Ended,
}

/// A TCP pass-through on loopback to a server's socket, carried on a thread of its own; ended when dropped.
pub struct Line {
    address: SocketAddr,
    state: watch::Sender<State>,
    thread: Option<JoinHandle<()>>,
}

impl Line {
    /// A line to `server`'s socket, carrying until cut.
    pub fn to(server: &Server) -> Self {
        Self::toward(SocketAddr::from((Ipv4Addr::LOCALHOST, server.port)))
    }

    /// A line to whatever listens at `far`.
    fn toward(far: SocketAddr) -> Self {
        let bound = Bound::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
        bound.set_nonblocking(true).expect("a socket the runtime can poll");
        let address = bound.local_addr().expect("its address");
        let (state, watched) = watch::channel(State::Carrying);
        let thread = std::thread::Builder::new()
            .name("line".into())
            .spawn(move || carry(bound, far, watched))
            .expect("the line's thread");
        Self { address, state, thread: Some(thread) }
    }

    /// The server's socket through this line, unencrypted, as [`Server::url`] says it.
    pub fn url(&self) -> String {
        format!("ws://{}/v1", self.address)
    }

    /// Closes every connection the line carries, and each one made until [`Line::mend`] as soon as it is accepted:
    /// a network that went away under its client.
    pub fn cut(&self) {
        self.state.send_replace(State::Cut);
    }

    /// Carries each new connection again.
    pub fn mend(&self) {
        self.state.send_replace(State::Carrying);
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        self.state.send_replace(State::Ended);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Accepts on `bound` until the line ends, each connection carried to `far` while the line carries and closed at once
/// while it is cut.
fn carry(bound: Bound, far: SocketAddr, state: watch::Receiver<State>) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_io().build().expect("the line's runtime");
    runtime.block_on(async move {
        let listener = TcpListener::from_std(bound).expect("a listener on the runtime");
        let mut ending = state.clone();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    if let Ok((near, _)) = accepted
                        && *state.borrow() == State::Carrying
                    {
                        tokio::spawn(pair(near, far, state.clone()));
                    }
                }
                _ = ending.wait_for(|now| *now == State::Ended) => break,
            }
        }
    });
}

/// Carries `near` to `far` both ways until either closes or the line stops carrying; both close as it returns.
async fn pair(mut near: TcpStream, far: SocketAddr, mut state: watch::Receiver<State>) {
    if let Ok(mut far) = TcpStream::connect(far).await {
        // Loopback with Nagle's delay would hold a small frame back for the peer's delayed acknowledgement.
        let _ = (near.set_nodelay(true), far.set_nodelay(true));
        tokio::select! {
            _ = copy_bidirectional(&mut near, &mut far) => {}
            _ = state.wait_for(|now| *now != State::Carrying) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Line;
    use std::io::{ErrorKind, Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(2);

    /// A server on loopback that sends back what each connection sends it.
    fn echo() -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bound");
        let address = listener.local_addr().expect("its address");
        std::thread::spawn(move || {
            for mut written in listener.incoming().map_while(Result::ok) {
                std::thread::spawn(move || {
                    let mut read = written.try_clone().expect("a second handle");
                    let _ = std::io::copy(&mut read, &mut written);
                });
            }
        });
        address
    }

    fn through(line: &Line) -> TcpStream {
        let stream = TcpStream::connect(line.address).expect("connected");
        stream.set_read_timeout(Some(WAIT)).expect("a timeout");
        stream
    }

    fn round_trip(stream: &mut TcpStream, said: &[u8]) -> Vec<u8> {
        stream.write_all(said).expect("written");
        let mut heard = vec![0; said.len()];
        stream.read_exact(&mut heard).expect("echoed");
        heard
    }

    /// Whether the far end closed `stream` within the wait.
    fn closed(stream: &mut TcpStream) -> bool {
        match stream.read(&mut [0; 16]) {
            Ok(read) => read == 0,
            Err(error) => !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    }

    #[test]
    fn a_line_carries_closes_every_connection_once_cut_refuses_new_ones_and_carries_again_once_mended() {
        let line = Line::toward(echo());
        assert!(line.url().starts_with("ws://127.0.0.1:"));
        let mut open = through(&line);
        assert_eq!(round_trip(&mut open, b"one"), b"one");
        line.cut();
        assert!(closed(&mut open), "a connection open as the line is cut is closed");
        assert!(closed(&mut through(&line)), "a connection made while cut is closed");
        line.mend();
        assert_eq!(round_trip(&mut through(&line), b"two"), b"two");
    }
}
