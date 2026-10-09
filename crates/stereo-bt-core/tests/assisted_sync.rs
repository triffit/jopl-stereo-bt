//! Acceptance tests for assisted sync (PRODUCT.md v2): a click track and
//! tap-based latency estimation that suggests per-device delays.

use stereo_bt_core::{ClickGenerator, Delay, Frame, SampleRate, TapAnalysis, suggest_delay};

fn click_starts(generator: &mut ClickGenerator, frames: usize, block: usize) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut out = vec![Frame::SILENCE; block];
    let mut position = 0;
    while position < frames {
        if let Some(offset) = generator.fill(&mut out) {
            starts.push(position + offset);
        }
        position += block;
    }
    starts
}

#[test]
fn clicks_start_once_per_period_at_exact_frame_positions() {
    let mut generator = ClickGenerator::new(SampleRate::HZ_48000, 1_000);

    let starts = click_starts(&mut generator, 48_000 * 3, 480);

    assert_eq!(starts, vec![0, 48_000, 96_000]);
}

#[test]
fn click_is_short_audible_and_silent_in_between() {
    let mut generator = ClickGenerator::new(SampleRate::HZ_48000, 500);
    let mut out = vec![Frame::SILENCE; 24_000];

    generator.fill(&mut out);

    let loud = out.iter().filter(|f| f.left.abs() > 0.01).count();
    let peak = out.iter().map(|f| f.left.abs()).fold(0.0, f32::max);
    assert!(peak > 0.3 && peak <= 1.0, "peak {peak}");
    assert!(loud > 50 && loud < 960, "click lasts {loud} frames");
    assert!(out[2_000..].iter().all(|f| *f == Frame::SILENCE));
    assert!(out.iter().all(|f| *f == Frame::new(f.left, f.left)));
}

#[test]
fn latency_is_the_median_lag_from_click_to_tap() {
    let clicks_ms = [0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0];
    let taps_ms = [250.0, 1_230.0, 2_270.0, 3_240.0, 4_260.0, 5_250.0];

    let analysis = TapAnalysis::new(&clicks_ms, &taps_ms, 1_000.0);

    assert_eq!(analysis.taps(), 6);
    assert!((analysis.latency_ms().unwrap() - 250.0).abs() < 1e-9);
}

#[test]
fn stray_taps_far_from_the_rest_are_ignored() {
    let clicks_ms = [0.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0];
    // One tap far too late (a missed click answered late).
    let taps_ms = [250.0, 1_250.0, 2_260.0, 3_900.0, 4_240.0, 5_250.0, 6_250.0];

    let analysis = TapAnalysis::new(&clicks_ms, &taps_ms, 1_000.0);

    assert_eq!(analysis.taps(), 6);
    assert!((analysis.latency_ms().unwrap() - 250.0).abs() < 1e-9);
}

#[test]
fn too_few_taps_give_no_estimate() {
    let analysis = TapAnalysis::new(&[0.0, 1_000.0], &[240.0, 1_250.0], 1_000.0);

    assert_eq!(analysis.latency_ms(), None);
}

#[test]
fn taps_before_the_first_click_are_ignored() {
    let clicks_ms = [1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0];
    let taps_ms = [500.0, 1_250.0, 2_250.0, 3_250.0, 4_250.0, 5_250.0];

    assert_eq!(TapAnalysis::new(&clicks_ms, &taps_ms, 1_000.0).taps(), 5);
}

#[test]
fn faster_device_gets_more_delay_to_match_the_reference() {
    let current = Delay::from_millis(20).unwrap();

    // Reference (default device) heard 230 ms after the click, this device after 80 ms.
    let suggestion = suggest_delay(230.0, 80.0, current);

    assert_eq!(suggestion.delay.as_millis(), 170);
    assert!(!suggestion.too_late);
}

#[test]
fn slower_device_than_reference_is_clamped_and_flagged() {
    let suggestion = suggest_delay(80.0, 230.0, Delay::from_millis(50).unwrap());

    assert_eq!(suggestion.delay, Delay::ZERO);
    assert!(suggestion.too_late);
}

#[test]
fn suggestion_never_exceeds_the_maximum_delay() {
    let suggestion = suggest_delay(900.0, 0.0, Delay::ZERO);

    assert_eq!(suggestion.delay, Delay::MAX);
}
