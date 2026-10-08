//! Acceptance tests for the per-output playout policy (jitter cushion,
//! underrun re-priming and drift trimming; PRODUCT.md F3, F9).

use stereo_bt_core::{Playout, PlayoutPlan};

const TARGET: usize = 1_920; // 40 ms at 48 kHz
const MAX_EXCESS: usize = 1_440; // 30 ms at 48 kHz

fn playout() -> Playout {
    Playout::new(TARGET, MAX_EXCESS)
}

#[test]
fn plays_silence_until_cushion_is_reached() {
    let mut p = playout();

    assert_eq!(p.plan(TARGET - 1, 480), PlayoutPlan { skip: 0, read: 0 });
    assert!(!p.is_primed());
}

#[test]
fn starts_reading_once_cushion_is_reached() {
    let mut p = playout();

    assert_eq!(p.plan(TARGET, 480), PlayoutPlan { skip: 0, read: 480 });
    assert!(p.is_primed());
}

#[test]
fn priming_aligns_queue_to_exactly_the_cushion_without_counting_a_trim() {
    let mut p = playout();
    let available = TARGET + MAX_EXCESS + 500;

    let plan = p.plan(available, 480);

    assert_eq!(plan.skip, available - TARGET);
    assert_eq!(p.trims(), 0);
}

#[test]
fn once_primed_reads_below_cushion_without_waiting() {
    let mut p = playout();
    p.plan(TARGET, 480);

    assert_eq!(p.plan(600, 480), PlayoutPlan { skip: 0, read: 480 });
}

#[test]
fn underrun_reads_what_is_left_then_re_primes() {
    let mut p = playout();
    p.plan(TARGET, 480);

    assert_eq!(p.plan(100, 480), PlayoutPlan { skip: 0, read: 100 });
    assert!(!p.is_primed());
    assert_eq!(p.underruns(), 1);
    assert_eq!(p.plan(TARGET - 1, 480), PlayoutPlan { skip: 0, read: 0 });
}

#[test]
fn excess_above_limit_is_trimmed_back_to_cushion() {
    let mut p = playout();
    p.plan(TARGET, 480);
    let available = TARGET + MAX_EXCESS + 1;

    let plan = p.plan(available, 480);

    assert_eq!(plan.skip, available - TARGET);
    assert_eq!(plan.read, 480);
    assert_eq!(p.trims(), 1);
}

#[test]
fn excess_within_limit_is_kept() {
    let mut p = playout();
    p.plan(TARGET, 480);

    assert_eq!(p.plan(TARGET + MAX_EXCESS, 480).skip, 0);
}

#[test]
fn reset_requires_priming_again() {
    let mut p = playout();
    p.plan(TARGET, 480);

    p.reset();

    assert!(!p.is_primed());
}
