//! A relay between the clients and a server's media port, so a test can break a call's media alone: the WebSocket
//! keeps working while every datagram is dropped, lost or held. The relay is a process of its own in a container of
//! the server's test image on this machine's network, as the server is: there the relay and the server take one port
//! at two addresses, which the server's announced address needs and Windows' mirrored network forbids between the
//! machine and a container.

use crate::control::{Command, Reply};
use crate::impair::{Impaired, Loss, Lost, Schedule};
use crate::{END, WAIT, image, runtime};
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Instant;

/// The relay's process, driven from the test's.
pub struct Relay {
    name: String,
    child: Child,
    /// Closed to end the relay, and `--rm` then removes its container; a test binary that dies closes it too.
    talk: Mutex<Option<Talk>>,
    address: SocketAddr,
}

/// What a test writes the relay and what it hears back, one line each.
struct Talk {
    stdin: ChildStdin,
    replies: mpsc::Receiver<Reply>,
}

impl Relay {
    /// A relay the clients reach at `ip` on a port it picks, carrying to that port on the container's loopback, in a
    /// container called `name`. Blocking.
    pub(crate) fn start(name: String, ip: IpAddr) -> Self {
        let mut child = runtime(&["run", "--rm", "--interactive", "--network", "host", "--label", "chatkord-test"])
            .args(["--name", &name, "--entrypoint", "chatkord-test-relay", &image(), &ip.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the container runtime started");
        let said = child.stdout.take().expect("its output");
        let (tell, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(said).lines().map_while(Result::ok) {
                let reply = serde_json::from_str(&line).unwrap_or_else(|_| panic!("the relay said {line:?}"));
                if tell.send(reply).is_err() {
                    return;
                }
            }
        });
        let stdin = child.stdin.take().expect("its input");
        let Ok(Reply::Listening(port)) = replies.recv_timeout(WAIT) else { panic!("the relay {name} did not listen") };
        let talk = Mutex::new(Some(Talk { stdin, replies }));
        Self { name, child, talk, address: SocketAddr::new(ip, port) }
    }

    /// Where the clients send: the address the server announces.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Tells the relay `command` and waits until it took effect; what it said.
    fn ask(&self, command: Command) -> Reply {
        let mut talk = self.talk.lock().unwrap_or_else(PoisonError::into_inner);
        let talk = talk.as_mut().expect("a running relay");
        let line = serde_json::to_string(&command).expect("a command in JSON");
        writeln!(talk.stdin, "{line}").and_then(|()| talk.stdin.flush()).expect("the relay heard");
        talk.replies.recv_timeout(WAIT).unwrap_or_else(|_| panic!("the relay {} answered {command:?}", self.name))
    }

    fn done(&self, command: Command) {
        let said = self.ask(command);
        assert_eq!(said, Reply::Done, "the relay took {command:?}");
    }

    /// Drops every datagram, both ways, until [`Relay::mend`].
    pub fn cut(&self) {
        self.done(Command::Cut);
    }

    /// Carries datagrams again.
    pub fn mend(&self) {
        self.done(Command::Mend);
    }

    /// Impairs the server's video on its way down from now on, as `schedule` says.
    pub fn impair(&self, schedule: Schedule) {
        self.done(Command::Impair(schedule));
    }

    /// Loses RTP by chance from now on, as `loss` says, beside any schedule.
    pub fn lose(&self, loss: Loss) {
        self.done(Command::Lose(loss));
    }

    /// What the loss has done so far; nothing without one.
    pub fn lost(&self) -> Lost {
        match self.ask(Command::Lost) {
            Reply::Lost(lost) => lost,
            other => panic!("the relay said {other:?} when asked what it lost"),
        }
    }

    /// What the schedule has done so far; nothing without one.
    pub fn impaired(&self) -> Impaired {
        match self.ask(Command::Impaired) {
            Reply::Impaired(impaired) => impaired,
            other => panic!("the relay said {other:?} when asked what it impaired"),
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        drop(self.talk.get_mut().unwrap_or_else(PoisonError::into_inner).take());
        let asked = Instant::now();
        while matches!(self.child.try_wait(), Ok(None)) && asked.elapsed() < END {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = runtime(&["kill", &self.name]).output();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    mod server {
        use crate::{Loss, Server, Way};

        #[tokio::test]
        async fn a_server_starts_with_its_media_behind_a_relay_that_takes_each_command() {
            let (server, relay, codes) = Server::start_relayed("relayed", 1).await;
            assert_eq!(codes.len(), 1);
            assert!(server.url().starts_with("ws://127.0.0.1:"));
            assert_ne!(relay.address().port(), 0);
            relay.cut();
            relay.mend();
            relay.lose(Loss { way: Way::Up, per_thousand: 20, seed: 1 });
            assert_eq!(relay.lost(), crate::Lost::default(), "nothing has crossed it yet");
        }
    }
}
