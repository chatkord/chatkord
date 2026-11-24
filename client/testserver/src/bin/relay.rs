//! The relay as a process of its own, run beside the server in its container's network: it listens on the address its
//! one argument names, carries to the same port on loopback, says that port, then takes each command its input brings
//! and answers once it took effect, until the input ends.
#![forbid(unsafe_code)]

use std::io::{BufRead, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::process::ExitCode;
use testserver::relay_process::{Command, Forwarder, Reply};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            let _ = writeln!(std::io::stderr(), "chatkord-test-relay: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let ip: IpAddr = std::env::args()
        .nth(1)
        .ok_or("usage: chatkord-test-relay ADDRESS")?
        .parse()
        .map_err(|_| "the argument is not an IP address")?;
    // Its own thread carries the datagrams while this one reads the commands.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|why| format!("no runtime: {why}"))?;
    let forwarder = runtime.block_on(Forwarder::new(ip, IpAddr::V4(Ipv4Addr::LOCALHOST)));
    say(Reply::Listening(forwarder.address().port()))?;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|why| format!("its input: {why}"))?;
        let command = serde_json::from_str(&line).map_err(|_| format!("not a command: {line:?}"))?;
        say(match command {
            Command::Cut => {
                forwarder.cut();
                Reply::Done
            }
            Command::Mend => {
                forwarder.mend();
                Reply::Done
            }
            Command::Impair(schedule) => {
                forwarder.impair(schedule);
                Reply::Done
            }
            Command::Lose(loss) => {
                forwarder.lose(loss);
                Reply::Done
            }
            Command::Lost => Reply::Lost(forwarder.lost()),
            Command::Impaired => Reply::Impaired(forwarder.impaired()),
        })?;
    }
    Ok(())
}

fn say(reply: Reply) -> Result<(), String> {
    let line = serde_json::to_string(&reply).map_err(|why| format!("a reply in JSON: {why}"))?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").and_then(|()| out.flush()).map_err(|why| format!("its output: {why}"))
}
