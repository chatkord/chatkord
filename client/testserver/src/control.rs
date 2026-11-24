//! What a test tells the relay's process and what the process answers: one JSON line each way per command, so the
//! relay runs beside the server, in the container's network, and is driven from the test's process on either system.

use crate::impair::{Impaired, Loss, Lost, Schedule};
use serde::{Deserialize, Serialize};

/// What a test asks of the relay.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Command {
    /// Drop every datagram, both ways.
    Cut,
    /// Carry datagrams again.
    Mend,
    /// Impair the server's video on its way down as the schedule says.
    Impair(Schedule),
    /// Lose RTP by chance as the loss says.
    Lose(Loss),
    /// Say what the loss has done.
    Lost,
    /// Say what the schedule has done.
    Impaired,
}

/// What the relay says: once the port it listens on, then one reply per command, after the command took effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    /// The port the clients reach it on, which the server must listen on too.
    Listening(u16),
    /// The command took effect.
    Done,
    /// What the loss has done so far.
    Lost(Lost),
    /// What the schedule has done so far.
    Impaired(Impaired),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impair::Way;
    use std::time::Duration;

    #[test]
    fn each_command_and_reply_reads_back_as_written() {
        let schedule = Schedule {
            late_at: 1,
            late_by: Duration::from_millis(2),
            lost_at: 3,
            lost_for: Duration::from_millis(4),
            held_at: 5,
            held_by: Duration::from_millis(6),
            drift_at: 7,
            drift_to: Duration::from_millis(8),
            drift_over: Duration::from_millis(9),
        };
        let loss = Loss { way: Way::Up, per_thousand: 20, seed: 0x6464 };
        for command in [Command::Cut, Command::Mend, Command::Impair(schedule), Command::Lose(loss)] {
            let line = serde_json::to_string(&command).unwrap();
            let read: Command = serde_json::from_str(&line).unwrap();
            assert_eq!(format!("{read:?}"), format!("{command:?}"), "{line}");
        }
        let lost = Lost { seen: 10, lost: 2 };
        let impaired = Impaired { late: true, drifted: 3, ..Impaired::default() };
        for reply in [Reply::Listening(40_000), Reply::Done, Reply::Lost(lost), Reply::Impaired(impaired)] {
            let line = serde_json::to_string(&reply).unwrap();
            assert_eq!(serde_json::from_str::<Reply>(&line).unwrap(), reply, "{line}");
        }
    }
}
