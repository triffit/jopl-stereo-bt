//! Acceptance tests for the per-device output chain (PRODUCT.md F2, F3, F4).

use proptest::prelude::*;
use stereo_bt_core::{ChannelMode, Delay, Frame, OutputChain, SampleRate, Volume};

// Test-only helper outside a #[test] fn, so clippy's allow-unwrap-in-tests does not apply.
#[allow(clippy::unwrap_used)]
fn rate_48k() -> SampleRate {
    SampleRate::from_hz(48_000).unwrap()
}

fn impulse_response(chain: &mut OutputChain, len: usize) -> Vec<Frame> {
    let mut frames = vec![Frame::SILENCE; len];
    frames[0] = Frame::new(1.0, 1.0);
    chain.process(&mut frames);
    frames
}

#[test]
fn f3_delay_of_10_ms_shifts_impulse_by_480_frames_at_48_khz() {
    let mut chain = OutputChain::new(rate_48k());
    chain.set_delay(Delay::from_millis(10).unwrap());

    let out = impulse_response(&mut chain, 1_000);

    let first_non_silent = out.iter().position(|f| *f != Frame::SILENCE);
    assert_eq!(first_non_silent, Some(480));
}

#[test]
fn f3_zero_delay_passes_audio_through_unchanged() {
    let mut chain = OutputChain::new(rate_48k());
    let input: Vec<Frame> = (0..64u8)
        .map(|i| Frame::new(f32::from(i), -f32::from(i)))
        .collect();
    let mut frames = input.clone();

    chain.process(&mut frames);

    assert_eq!(frames, input);
}

#[test]
fn f3_maximum_delay_is_500_ms() {
    assert!(Delay::from_millis(500).is_ok());
    assert!(Delay::from_millis(501).is_err());
}

#[test]
fn f2_left_only_plays_left_channel_on_both_speakers() {
    let mut chain = OutputChain::new(rate_48k());
    chain.set_channel_mode(ChannelMode::LeftOnly);
    let mut frames = [Frame::new(0.25, -0.75)];

    chain.process(&mut frames);

    assert_eq!(frames, [Frame::new(0.25, 0.25)]);
}

#[test]
fn f2_right_only_plays_right_channel_on_both_speakers() {
    let mut chain = OutputChain::new(rate_48k());
    chain.set_channel_mode(ChannelMode::RightOnly);
    let mut frames = [Frame::new(0.25, -0.75)];

    chain.process(&mut frames);

    assert_eq!(frames, [Frame::new(-0.75, -0.75)]);
}

#[test]
fn f2_mono_mix_averages_both_channels() {
    let mut chain = OutputChain::new(rate_48k());
    chain.set_channel_mode(ChannelMode::MonoMix);
    let mut frames = [Frame::new(0.5, -0.25)];

    chain.process(&mut frames);

    assert_eq!(frames, [Frame::new(0.125, 0.125)]);
}

#[test]
fn f4_zero_volume_silences_output() {
    let mut chain = OutputChain::new(rate_48k());
    chain.set_volume(Volume::from_percent(0).unwrap());
    let mut frames = [Frame::new(0.5, -0.5)];

    chain.process(&mut frames);

    assert_eq!(frames, [Frame::SILENCE]);
}

#[test]
fn f4_volume_above_100_percent_is_rejected() {
    assert!(Volume::from_percent(100).is_ok());
    assert!(Volume::from_percent(101).is_err());
}

#[test]
fn sample_rate_outside_supported_range_is_rejected() {
    assert!(SampleRate::from_hz(0).is_err());
    assert!(SampleRate::from_hz(7_999).is_err());
    assert!(SampleRate::from_hz(384_001).is_err());
    assert!(SampleRate::from_hz(44_100).is_ok());
}

proptest! {
    #[test]
    fn f3_any_delay_shifts_impulse_by_exactly_ms_times_rate(ms in 0u16..=500) {
        let mut chain = OutputChain::new(rate_48k());
        chain.set_delay(Delay::from_millis(ms).unwrap());

        let out = impulse_response(&mut chain, 24_001);

        let expected = usize::from(ms) * 48;
        prop_assert_eq!(out.iter().position(|f| *f != Frame::SILENCE), Some(expected));
    }

    #[test]
    fn f2_left_only_never_leaks_right_channel(left in -1.0f32..=1.0, right in -1.0f32..=1.0) {
        let mut chain = OutputChain::new(rate_48k());
        chain.set_channel_mode(ChannelMode::LeftOnly);
        let mut frames = [Frame::new(left, right)];

        chain.process(&mut frames);

        prop_assert_eq!(frames[0], Frame::new(left, left));
    }

    #[test]
    fn f4_output_never_exceeds_input_amplitude(
        left in -1.0f32..=1.0,
        right in -1.0f32..=1.0,
        percent in 0u8..=100,
    ) {
        let mut chain = OutputChain::new(rate_48k());
        chain.set_volume(Volume::from_percent(percent).unwrap());
        let mut frames = [Frame::new(left, right)];

        chain.process(&mut frames);

        prop_assert!(frames[0].left.abs() <= left.abs());
        prop_assert!(frames[0].right.abs() <= right.abs());
    }

    #[test]
    fn f4_gain_is_monotonic_in_percent(a in 0u8..=100, b in 0u8..=100) {
        let (low, high) = (a.min(b), a.max(b));
        let gain_low = Volume::from_percent(low).unwrap().gain();
        let gain_high = Volume::from_percent(high).unwrap().gain();
        prop_assert!(gain_low <= gain_high);
    }
}
