//! The desktop portal's Realtime, which asks RTKit for a thread that names itself. It is asked from a thread of its
//! own, so a voice thread never waits on D-Bus.

use ashpd::desktop::realtime::Realtime;
use rustix::process::{Pid, Resource, Rlimit, getpid, getrlimit, setrlimit};
use rustix::thread::gettid;

/// Why a thread was not made real-time.
#[derive(report::Public)]
pub(super) enum Refused {
    /// The thread that asks the portal did not start.
    Asking,
    /// The portal, or RTKit behind it, said no or is not there.
    Portal,
    /// The process could not bound how long its real-time threads run: the system's error number.
    Bound(#[public] i32),
}

/// Nothing to give back: the standing ends with the thread.
pub(super) struct Held;

pub(super) fn ask() -> Result<Held, Refused> {
    let (process, thread) = (getpid(), gettid());
    std::thread::Builder::new()
        .name("chatkord-ahead".into())
        .spawn(move || {
            if let Err(why) = async_io::block_on(raise(process, thread)) {
                super::say(&why);
            }
        })
        .map(|_| Held)
        .map_err(|_| Refused::Asking)
}

fn id(of: Pid) -> u32 {
    of.as_raw_nonzero().get().cast_unsigned()
}

async fn raise(process: Pid, thread: Pid) -> Result<(), Refused> {
    let portal = Realtime::new().await.map_err(|_| Refused::Portal)?;
    // Read from the proxy: ashpd 0.13.13's own accessors ask for a `u32` and an `i64`, and the portal sends an `x`
    // and an `i`, so both fail.
    let read = |_: ashpd::zbus::Error| Refused::Portal;
    let longest: i64 = portal.get_property("RTTimeUSecMax").await.map_err(read)?;
    let most: i32 = portal.get_property("MaxRealtimePriority").await.map_err(read)?;
    bound_running(u64::try_from(longest).unwrap_or(0))?;
    let most = u32::try_from(most).unwrap_or(0);
    // One below the most a desktop grants, which PipeWire's own module takes for the thread the device is served on:
    // that one is never kept waiting by this.
    let priority = most.saturating_sub(1).max(1);
    portal.max_thread_realtime_with_pid(id(process), u64::from(id(thread)), priority).await.map_err(|_| Refused::Portal)
}

/// Bounds how long a real-time thread of this process may run without sleeping to `longest` microseconds: RTKit
/// grants nothing to a process that could hold a core for ever.
fn bound_running(longest: u64) -> Result<(), Refused> {
    let now = getrlimit(Resource::Rttime);
    let within = |limit: Option<u64>| Some(limit.map_or(longest, |limit| limit.min(longest)));
    setrlimit(Resource::Rttime, Rlimit { current: within(now.current), maximum: within(now.maximum) })
        .map_err(|why| Refused::Bound(why.raw_os_error()))
}

/// Whether the kernel schedules the calling thread by a real-time policy: the 41st field of its `stat`, counted from
/// the state after the name, is not `SCHED_OTHER`.
#[cfg(test)]
pub(super) fn ahead() -> bool {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap_or_default();
    let after = stat.rsplit_once(')').map_or("", |(_, after)| after);
    after.split_whitespace().nth(38).is_some_and(|policy| policy != "0")
}
