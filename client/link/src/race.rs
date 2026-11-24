//! Which of a name's addresses a connection is opened to: the first to answer, the families tried in turn, so one
//! family whose route leads nowhere costs a moment and not the whole wait (RFC 8305).

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::sleep;

/// How long one attempt is given alone before the next address is tried beside it: RFC 8305's recommended delay.
const NEXT_ATTEMPT: Duration = Duration::from_millis(250);

/// `found` in the order its addresses are tried: the resolver's, the two families taking turns from its first.
fn in_turns(found: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let first_is_v6 = found.first().is_some_and(SocketAddr::is_ipv6);
    let (first, other): (Vec<_>, Vec<_>) = found.into_iter().partition(|at| at.is_ipv6() == first_is_v6);
    let mut turns = Vec::with_capacity(first.len() + other.len());
    let (mut first, mut other) = (first.into_iter(), other.into_iter());
    loop {
        match (first.next(), other.next()) {
            (None, None) => return turns,
            (one, another) => turns.extend(one.into_iter().chain(another)),
        }
    }
}

/// Opens a connection to the first of `found` that answers: each address is tried [`NEXT_ATTEMPT`] after the one
/// before it, or at once when that one failed, and the attempts still under way end with the first that connects.
/// None when every address failed.
pub(crate) async fn first_to_answer<S, F>(found: Vec<SocketAddr>, connect: impl Fn(SocketAddr) -> F) -> Option<S>
where
    S: Send + 'static,
    F: Future<Output = std::io::Result<S>> + Send + 'static,
{
    let mut waiting = in_turns(found).into_iter();
    let mut flying = JoinSet::new();
    loop {
        match waiting.next() {
            Some(at) => drop(flying.spawn(connect(at))),
            None if flying.is_empty() => return None,
            None => {}
        }
        tokio::select! {
            answered = flying.join_next() => {
                if let Some(Ok(Ok(stream))) = answered {
                    return Some(stream);
                }
            }
            () = sleep(NEXT_ATTEMPT), if waiting.len() > 0 => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use tokio::time::Instant;

    const V6: &str = "[2001:db8::1]:443";
    const V6_TOO: &str = "[2001:db8::2]:443";
    const V4: &str = "192.0.2.1:443";
    const V4_TOO: &str = "192.0.2.2:443";

    fn at(address: &str) -> SocketAddr {
        address.parse().expect("an address")
    }

    /// A network where IPv6 leads nowhere: a connection over it never answers, one over IPv4 does at once.
    async fn over_ipv4_only(to: SocketAddr) -> std::io::Result<SocketAddr> {
        if to.is_ipv6() {
            std::future::pending::<()>().await;
        }
        Ok(to)
    }

    #[test]
    fn the_families_take_turns_from_the_resolvers_first() {
        let found = vec![at(V6), at(V6_TOO), at(V4), at(V4_TOO)];
        assert_eq!(in_turns(found), [at(V6), at(V4), at(V6_TOO), at(V4_TOO)]);
        assert_eq!(in_turns(vec![at(V4), at(V4_TOO), at(V6)]), [at(V4), at(V6), at(V4_TOO)]);
        assert_eq!(in_turns(vec![at(V4), at(V4_TOO)]), [at(V4), at(V4_TOO)]);
        assert!(in_turns(Vec::new()).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_family_that_never_answers_costs_one_delay_not_the_connection() {
        let begun = Instant::now();
        let reached = first_to_answer(vec![at(V6), at(V6_TOO), at(V4)], over_ipv4_only).await;
        assert_eq!(reached, Some(at(V4)));
        assert_eq!(begun.elapsed(), NEXT_ATTEMPT, "the second address is tried one delay after the first");
    }

    #[tokio::test(start_paused = true)]
    async fn an_address_that_fails_gives_way_to_the_next_at_once() {
        let refused = |to: SocketAddr| async move {
            if to == at(V4) { Ok(to) } else { Err(std::io::Error::from(ErrorKind::ConnectionRefused)) }
        };
        let begun = Instant::now();
        assert_eq!(first_to_answer(vec![at(V6), at(V6_TOO), at(V4)], refused).await, Some(at(V4)));
        assert_eq!(begun.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_to_answer_wins_though_it_was_tried_later() {
        let slow_first = |to: SocketAddr| async move {
            sleep(if to.is_ipv6() { Duration::from_secs(1) } else { Duration::from_millis(10) }).await;
            Ok(to)
        };
        assert_eq!(first_to_answer(vec![at(V6), at(V4)], slow_first).await, Some(at(V4)));
    }

    #[tokio::test(start_paused = true)]
    async fn addresses_that_all_fail_reach_nothing() {
        let refused =
            |_: SocketAddr| async { Err::<SocketAddr, _>(std::io::Error::from(ErrorKind::ConnectionRefused)) };
        assert_eq!(first_to_answer(vec![at(V6), at(V4)], refused).await, None);
        assert_eq!(first_to_answer(Vec::new(), refused).await, None);
    }
}
