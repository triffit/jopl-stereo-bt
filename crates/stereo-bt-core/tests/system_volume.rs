//! Acceptance tests for following the Windows system volume (all devices
//! get louder and quieter together).

use stereo_bt_core::{Frame, OutputChain, SampleRate, SystemVolume, Volume};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn zero_decibels_is_unity_gain() {
    let volume = SystemVolume::new(0.0, 1.0, false);

    assert!(close(volume.amplitude(), 1.0));
}

#[test]
fn minus_six_decibels_is_half_amplitude() {
    let volume = SystemVolume::new(-6.020_6, 0.6, false);

    assert!(close(volume.amplitude(), 0.5));
}

#[test]
fn muted_or_zero_slider_is_silent() {
    assert!(close(SystemVolume::new(-10.0, 0.5, true).amplitude(), 0.0));
    assert!(close(
        SystemVolume::new(-65.25, 0.0, false).amplitude(),
        0.0
    ));
}

#[test]
fn slider_position_is_reported_in_percent() {
    assert_eq!(SystemVolume::new(-10.0, 0.456, false).percent(), 46);
}

#[test]
fn master_gain_scales_on_top_of_device_volume() {
    let mut chain = OutputChain::new(SampleRate::HZ_48000);
    chain.set_volume(Volume::from_percent(50).unwrap()); // gain 0.25
    chain.set_master_gain(0.5);
    let mut frames = vec![Frame::new(1.0, 1.0); 4];

    chain.process(&mut frames); // ramps from 1.0 to 0.125
    frames.fill(Frame::new(1.0, 1.0));
    chain.process(&mut frames);

    assert!(frames.iter().all(|f| close(f.left, 0.125)));
}

#[test]
fn gain_changes_ramp_across_the_block_instead_of_jumping() {
    let mut chain = OutputChain::new(SampleRate::HZ_48000);
    chain.set_master_gain(0.0);
    let mut frames = vec![Frame::new(1.0, 1.0); 100];

    chain.process(&mut frames);

    // Starts near the old gain, ends exactly at the new one, never jumps.
    assert!(frames[0].left > 0.95);
    assert!(close(frames[99].left, 0.0));
    for pair in frames.windows(2) {
        assert!(pair[0].left - pair[1].left < 0.02);
    }
}
