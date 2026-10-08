//! Smoke tests against the real Windows audio system.
//!
//! Ignored by default because they need at least one active output device.
//! Run with: `cargo test -p stereo-bt-wasapi -- --ignored`

#![cfg(windows)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use stereo_bt_core::Frame;
use stereo_bt_wasapi::{
    ComGuard, DeviceWatcher, LoopbackCapture, MmcssGuard, RenderStream, default_output_id,
    is_muted, list_output_devices, set_muted,
};

#[test]
#[ignore = "needs an active audio output device"]
fn lists_output_devices_with_exactly_one_default() {
    let _com = ComGuard::init_mta().unwrap();

    let devices = list_output_devices().unwrap();

    assert!(!devices.is_empty());
    assert_eq!(devices.iter().filter(|d| d.is_default).count(), 1);
    assert!(devices.iter().all(|d| !d.name.is_empty()));
}

#[test]
#[ignore = "needs an active audio output device"]
fn loopback_capture_runs_without_errors() {
    let _com = ComGuard::init_mta().unwrap();
    let mut capture = LoopbackCapture::open_default().unwrap();
    capture.start().unwrap();

    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        capture.read(20).unwrap();
    }

    capture.stop().unwrap();
}

#[test]
#[ignore = "needs an active audio output device; plays 200 ms of silence"]
fn render_stream_accepts_silence_on_default_device() {
    let _com = ComGuard::init_mta().unwrap();
    let default = list_output_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.is_default)
        .unwrap();
    let mut render = RenderStream::open(&default.id).unwrap();
    let silence = vec![Frame::SILENCE; render.buffer_frames()];
    let writable = render.writable_frames().unwrap();
    render.write(&silence[..writable]).unwrap();
    render.start().unwrap();

    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        if render.wait(50) {
            let writable = render.writable_frames().unwrap();
            assert_eq!(render.write(&silence[..writable]).unwrap(), writable);
        }
    }

    render.stop().unwrap();
}

#[test]
#[ignore = "needs the Windows audio service"]
fn device_watcher_registers_and_unregisters() {
    let _com = ComGuard::init_mta().unwrap();
    let (sender, _receiver) = mpsc::channel();

    let watcher = DeviceWatcher::start(&sender).unwrap();

    drop(watcher);
}

#[test]
#[ignore = "needs an active audio output device"]
fn default_output_id_matches_listed_default() {
    let _com = ComGuard::init_mta().unwrap();

    let default = default_output_id().unwrap().unwrap();

    let listed = list_output_devices().unwrap();
    assert!(listed.iter().any(|d| d.is_default && d.id == default));
}

#[test]
#[ignore = "needs an active audio output device; rewrites its current mute state"]
fn endpoint_mute_can_be_read_and_written_back_unchanged() {
    let _com = ComGuard::init_mta().unwrap();
    let default = default_output_id().unwrap().unwrap();

    let muted = is_muted(&default).unwrap();
    set_muted(&default, muted).unwrap();

    assert_eq!(is_muted(&default).unwrap(), muted);
}

#[test]
#[ignore = "needs the Windows MMCSS service"]
fn thread_can_register_for_pro_audio_scheduling() {
    let guard = MmcssGuard::pro_audio().unwrap();

    drop(guard);
}
