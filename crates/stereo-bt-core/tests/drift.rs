//! Acceptance tests for click-free clock-drift correction (PRODUCT.md F9):
//! a variable-ratio resampler steered by a fill-level controller.

// Simulation arithmetic converts small frame counts between integers and
// floats, and helpers outside #[test] functions unwrap on purpose.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unwrap_used
)]

use std::f64::consts::TAU;

use proptest::prelude::*;
use stereo_bt_core::{DriftControl, DriftResampler, Frame};

fn ramp(from: u32, count: u32) -> Vec<Frame> {
    (from..from + count)
        .map(|i| {
            let v = f32::from(u16::try_from(i).unwrap());
            Frame::new(v, -v)
        })
        .collect()
}

#[test]
fn unity_ratio_passes_audio_through_with_three_frames_latency() {
    let mut resampler = DriftResampler::new();
    let input = ramp(1, 100);
    let mut out = vec![Frame::SILENCE; 100];

    assert_eq!(resampler.input_needed(out.len(), 1.0), 100);
    resampler.process(&input, 1.0, &mut out);

    assert_eq!(out[..3], [Frame::SILENCE; 3]);
    assert_eq!(out[3..], input[..97]);
}

#[test]
fn faster_ratio_consumes_proportionally_more_input() {
    let mut resampler = DriftResampler::new();
    let mut consumed = 0;
    let mut out = vec![Frame::SILENCE; 480];

    for _ in 0..100 {
        let needed = resampler.input_needed(out.len(), 1.001);
        consumed += needed;
        resampler.process(&vec![Frame::SILENCE; needed], 1.001, &mut out);
    }

    // 48_000 output frames at ratio 1.001 consume 48_048 input frames.
    assert!((48_047..=48_049).contains(&consumed), "consumed {consumed}");
}

#[test]
fn resampled_sine_stays_clean() {
    // A 1 kHz tone played 0.2 % fast must come out as a clean 1.002 kHz tone.
    let ratio = 1.002;
    let rate = 48_000.0;
    let mut resampler = DriftResampler::new();
    let mut out = vec![Frame::SILENCE; 4_800];
    let needed = resampler.input_needed(out.len(), ratio);
    let input: Vec<Frame> = (0..needed)
        .map(|n| {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let v = (TAU * 1_000.0 * n as f64 / rate).sin() as f32;
            Frame::new(v, v)
        })
        .collect();

    resampler.process(&input, ratio, &mut out);

    // Output frame k reads input position k * ratio, three frames behind.
    let worst = out
        .iter()
        .enumerate()
        .skip(10)
        .map(|(k, f)| {
            #[allow(clippy::cast_precision_loss)]
            let position = k as f64 * ratio - 3.0;
            let expected = (TAU * 1_000.0 * position / rate).sin();
            (f64::from(f.left) - expected).abs()
        })
        .fold(0.0, f64::max);
    assert!(worst < 1e-3, "worst error {worst}");
}

#[test]
fn controller_speeds_up_when_queue_is_too_full_and_slows_down_when_too_empty() {
    let mut full = DriftControl::new(1_920);
    let mut empty = DriftControl::new(1_920);

    for _ in 0..200 {
        full.update(1_920 + 480);
        empty.update(1_920 - 480);
    }

    assert!(full.ratio() > 1.0);
    assert!(empty.ratio() < 1.0);
}

#[test]
fn ratio_never_leaves_the_inaudible_range() {
    let mut control = DriftControl::new(1_920);

    for _ in 0..100_000 {
        control.update(48_000);
    }

    assert!(control.ratio() <= 1.0 + DriftControl::MAX_DEVIATION + f64::EPSILON);
}

#[test]
fn reset_forgets_the_measured_drift() {
    let mut control = DriftControl::new(1_920);
    for _ in 0..1_000 {
        control.update(4_000);
    }

    control.reset();

    assert!((control.ratio() - 1.0).abs() < f64::EPSILON);
}

/// Simulates a capture clock running `ppm` fast relative to the device and
/// checks the queue settles back at the cushion (no sync offset builds up).
fn settled_error(ppm: f64) -> f64 {
    const TARGET: usize = 1_920;
    const BLOCK: usize = 480;
    let mut control = DriftControl::new(TARGET);
    let mut resampler = DriftResampler::new();
    let mut queue = TARGET as f64;
    let mut produced_fraction = 0.0;
    let mut out = vec![Frame::SILENCE; BLOCK];
    let mut measured = TARGET;
    let silence = vec![Frame::SILENCE; 2 * BLOCK];
    // 5 simulated minutes of 10 ms render cycles (settling takes ~30 s).
    for _ in 0..30_000 {
        produced_fraction += BLOCK as f64 * (1.0 + ppm * 1e-6);
        let produced = produced_fraction.floor();
        produced_fraction -= produced;
        queue += produced;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let available = queue.max(0.0) as usize;
        measured = available;
        control.update(available);
        let needed = resampler.input_needed(BLOCK, control.ratio());
        resampler.process(&silence[..needed], control.ratio(), &mut out);
        #[allow(clippy::cast_precision_loss)]
        let consumed = needed as f64;
        queue -= consumed;
    }
    measured.abs_diff(TARGET) as f64
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4))]

    #[test]
    fn f9_queue_settles_at_the_cushion_for_realistic_clock_drift(ppm in -500.0f64..500.0) {
        // Settles within 2 ms of the cushion: no sync offset builds up.
        prop_assert!(settled_error(ppm) < 96.0, "ppm {ppm}");
    }
}
