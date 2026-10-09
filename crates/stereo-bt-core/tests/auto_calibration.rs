//! Acceptance tests for automatic latency measurement with a microphone:
//! finding a chirp in a recording and planning default device and delays.

// Helpers outside #[test] functions unwrap on purpose; panicking is the intent.
#![allow(clippy::unwrap_used, clippy::panic)]

use stereo_bt_core::{
    CalibrationOutcome, ClickGenerator, Delay, DeviceId, DeviceLatency, Frame, SampleRate,
    align_to_slowest, find_arrival, plan_first_round, plan_second_round,
};

fn id(raw: &str) -> DeviceId {
    DeviceId::new(raw).unwrap()
}

fn latency(raw: &str, is_source: bool, measured_ms: f64, delay_ms: u16) -> DeviceLatency {
    DeviceLatency {
        id: id(raw),
        is_source,
        measured_ms,
        delay: Delay::from_millis(delay_ms).unwrap(),
    }
}

fn chirp_template() -> Vec<f32> {
    ClickGenerator::chirp(SampleRate::HZ_48000, 1_000).template()
}

/// A noisy recording with the chirp starting at frame `at`, scaled by `gain`.
fn recording(len: usize, at: usize, gain: f32) -> Vec<f32> {
    let template = chirp_template();
    let mut seed = 12_345_u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (f32::from(u16::try_from(seed >> 16).unwrap()) / 65_535.0 - 0.5) * 0.02
    };
    (0..len)
        .map(|i| {
            let signal = i
                .checked_sub(at)
                .and_then(|k| template.get(k))
                .map_or(0.0, |s| s * gain);
            signal + noise()
        })
        .collect()
}

#[test]
fn chirp_generator_plays_the_chirp_once_per_period() {
    let mut generator = ClickGenerator::chirp(SampleRate::HZ_48000, 500);
    let mut out = vec![Frame::SILENCE; 48_000];
    let template = generator.template();

    assert_eq!(generator.fill(&mut out), Some(0));

    assert!(template.len() > 2_000, "a chirp lasts tens of milliseconds");
    assert!((out[100].left - template[100]).abs() < 1e-6);
    assert!((out[24_100].left - template[100]).abs() < 1e-6);
}

#[test]
fn arrival_of_a_quiet_chirp_is_found_to_the_frame() {
    let rec = recording(20_000, 7_321, 0.05);

    let arrival = find_arrival(&rec, &chirp_template(), 0, 20_000).unwrap();

    assert_eq!(arrival.offset, 7_321);
    assert!(arrival.quality > 8.0, "quality {}", arrival.quality);
}

#[test]
fn search_window_limits_where_the_chirp_may_be() {
    let rec = recording(20_000, 7_321, 0.05);

    let arrival = find_arrival(&rec, &chirp_template(), 5_000, 10_000).unwrap();

    assert_eq!(
        arrival.offset, 2_321,
        "offset is relative to the window start"
    );
}

#[test]
fn noise_without_a_chirp_is_rejected() {
    let rec = recording(20_000, usize::MAX, 0.0);

    assert_eq!(find_arrival(&rec, &chirp_template(), 0, 20_000), None);
}

#[test]
fn current_default_works_when_every_device_can_wait_for_it() {
    // Default plays after 150 ms; targets arrive after 90 and 120 ms (no delay yet).
    let measured = [
        latency("S", true, 150.0, 0),
        latency("A", false, 90.0, 0),
        latency("B", false, 130.0, 10),
    ];

    let outcome = plan_first_round(&measured).unwrap();

    assert_eq!(
        outcome,
        CalibrationOutcome::Aligned {
            delays: vec![
                (id("A"), Delay::from_millis(60).unwrap()),
                (id("B"), Delay::from_millis(30).unwrap()),
            ]
        }
    );
}

#[test]
fn slowest_late_device_is_tried_as_default() {
    let measured = [
        latency("S", true, 85.0, 0),
        latency("A", false, 160.0, 0),
        latency("B", false, 200.0, 20),
    ];

    assert_eq!(
        plan_first_round(&measured),
        Some(CalibrationOutcome::TryDefault(id("B")))
    );
}

#[test]
fn second_round_finds_the_pipeline_overhead_and_the_best_default() {
    // Natives: S 85, A 70, B 113 ms; pipeline overhead 60 ms.
    let first = [
        latency("S", true, 85.0, 0),
        latency("A", false, 130.0, 0),
        latency("B", false, 173.0, 0),
    ];
    // B is now the default; S and A go through the pipeline.
    let second = [
        latency("B", true, 113.0, 0),
        latency("S", false, 145.0, 0),
        latency("A", false, 130.0, 0),
    ];

    let Some(CalibrationOutcome::Best {
        default,
        delays,
        overhead_ms,
        behind_ms,
    }) = plan_second_round(&first, &second)
    else {
        panic!("expected a final plan");
    };

    assert_eq!(default, id("B"), "the slowest device by itself");
    assert!((overhead_ms - 60.0).abs() < 1e-9);
    // With B as default, S (85 + 60 = 145) is the latest target: 32 ms behind B.
    assert!((behind_ms - 32.0).abs() < 1e-9);
    // Targets wait for the latest target so they are in sync with each other.
    assert_eq!(
        delays,
        vec![
            (id("S"), Delay::ZERO),
            (id("A"), Delay::from_millis(15).unwrap()),
        ]
    );
}

#[test]
fn second_round_reports_perfect_sync_when_the_default_is_slow_enough() {
    // Natives: S 85, A 70, B 200 ms; overhead 60 ms → B as default leaves room.
    let first = [
        latency("S", true, 85.0, 0),
        latency("A", false, 130.0, 0),
        latency("B", false, 260.0, 0),
    ];
    let second = [
        latency("B", true, 200.0, 0),
        latency("S", false, 145.0, 0),
        latency("A", false, 130.0, 0),
    ];

    let Some(CalibrationOutcome::Best {
        default,
        delays,
        behind_ms,
        ..
    }) = plan_second_round(&first, &second)
    else {
        panic!("expected a final plan");
    };

    assert_eq!(default, id("B"));
    assert!(behind_ms <= 0.0);
    assert_eq!(
        delays,
        vec![
            (id("S"), Delay::from_millis(55).unwrap()),
            (id("A"), Delay::from_millis(70).unwrap()),
        ]
    );
}

#[test]
fn with_a_muted_default_every_device_waits_for_the_slowest() {
    let measured = [
        latency("A", false, 130.0, 0),
        latency("B", false, 173.0, 0),
        latency("C", false, 150.0, 10),
    ];

    assert_eq!(
        align_to_slowest(&measured),
        vec![
            (id("A"), Delay::from_millis(43).unwrap()),
            (id("B"), Delay::ZERO),
            (id("C"), Delay::from_millis(33).unwrap()),
        ]
    );
}

#[test]
fn unmeasured_default_gives_no_plan() {
    assert_eq!(plan_first_round(&[latency("A", false, 90.0, 0)]), None);
}
