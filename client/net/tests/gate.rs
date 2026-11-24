//! The gate keeps a rate change to the ones worth a key frame. The traces are targets a real
//! engine gave, so the counts are what a call would have cost, not what a model predicts.

mod common;

use net::{RateGate, Verdict};
use std::time::Duration;

const OPEN: u32 = 4_000_000;

fn at(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

fn applied(trace: &[(u64, u32)]) -> usize {
    let mut gate = RateGate::new(OPEN, Duration::ZERO);
    trace.iter().filter(|(ms, rate)| matches!(gate.offer(Duration::from_millis(*ms), *rate), Verdict::Apply)).count()
}

#[test]
fn a_target_close_to_the_running_rate_is_kept() {
    let mut gate = RateGate::new(OPEN, Duration::ZERO);
    assert_eq!(gate.offer(at(10.0), 4_000_000), Verdict::Keep, "the same rate");
    assert_eq!(gate.offer(at(10.0), 3_300_000), Verdict::Keep, "17.5% lower is under the threshold");
    assert_eq!(gate.offer(at(10.0), 3_200_000), Verdict::Apply, "20% lower is the threshold");
}

#[test]
fn a_slow_drift_is_applied_once_it_adds_up() {
    let mut gate = RateGate::new(1_000_000, Duration::ZERO);
    let steps: Vec<Verdict> =
        (1..=25u32).map(|n| gate.offer(at(10.0 + f64::from(n)), 1_000_000 + n * 10_000)).collect();
    let applied: Vec<usize> =
        steps.iter().enumerate().filter(|(_, verdict)| matches!(verdict, Verdict::Apply)).map(|(at, _)| at).collect();
    assert_eq!(applied, [19], "1% a step is kept until the rate is 20% above the running one, then once");
}

#[test]
fn an_increase_waits_and_a_decrease_does_not() {
    let mut gate = RateGate::new(1_000_000, Duration::ZERO);
    assert_eq!(gate.offer(at(1.9), 2_000_000), Verdict::Keep, "a big increase 1.9 s after the start");
    assert_eq!(gate.offer(at(2.0), 2_000_000), Verdict::Apply, "and applied at 2 s");
    assert_eq!(gate.offer(at(2.1), 1_000_000), Verdict::Apply, "a big decrease 0.1 s later is not held");
}

#[test]
fn after_a_rest_the_rate_the_link_carried_is_kept_until_it_is_measured_again() {
    let mut gate = RateGate::woken(20_000_000, at(30.0));
    // The engine's estimate after 20 s with nothing sent, from a 4K share on Machine A: its floor, then back up.
    assert_eq!(gate.offer(at(30.03), 4_000_000), Verdict::Keep, "the floor, measured on nothing");
    assert_eq!(gate.offer(at(31.9), 35_000_000), Verdict::Keep, "nor the climb back");
    assert_eq!(gate.offer(at(32.0), 4_000_000), Verdict::Apply, "measured again, it applies");
}

#[test]
fn the_traces_a_real_engine_gave_cost_a_key_frame_every_couple_of_seconds_not_every_target() {
    let runs = [common::RUN_1, common::RUN_2, common::RUN_3];
    let counts: Vec<usize> = runs.iter().map(|run| applied(run)).collect();
    let lengths: Vec<usize> = runs.iter().map(|run| run.len()).collect();
    eprintln!("targets {lengths:?} -> applied {counts:?}");
    assert_eq!(counts, [2, 2, 6]);
    assert_eq!(lengths, [7, 7, 89]);
}
