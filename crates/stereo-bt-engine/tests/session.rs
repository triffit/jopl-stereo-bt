//! Integration tests for a [`Session`] running real threads on a fake backend.

// The fake backend below is test-only support code outside #[test] functions,
// where clippy's allow-unwrap-in-tests does not reach; panicking is the intent.
#![allow(clippy::unwrap_used)]

use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use stereo_bt_core::{ChannelMode, DeviceId, Frame, SampleRate};
use stereo_bt_engine::{Backend, CaptureStream, OutputConfig, RenderSink, Session, StreamState};

const BLOCK: usize = 480;
const MISSING: &str = "missing-device";

#[derive(Debug)]
struct FakeError;

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fake device error")
    }
}

impl std::error::Error for FakeError {}

/// Capture produces a ramp (1, -1), (2, -2), ... at roughly real time.
struct FakeCapture {
    next: u32,
    block: Vec<Frame>,
}

impl CaptureStream for FakeCapture {
    type Error = FakeError;

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn read(&mut self, _timeout_ms: u32) -> Result<&[Frame], FakeError> {
        thread::sleep(Duration::from_millis(10));
        for frame in &mut self.block {
            self.next += 1;
            // Ramp stays below 2^24, so every value is exact in f32.
            let value = f32::from(u16::try_from(self.next % 60_000).unwrap());
            *frame = Frame::new(value, -value);
        }
        Ok(&self.block)
    }
}

/// Render pulls faster than capture produces, so it never drops audio.
struct FakeRender {
    written: Recording,
}

impl RenderSink for FakeRender {
    type Error = FakeError;

    fn buffer_frames(&self) -> usize {
        BLOCK
    }

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn wait(&mut self, _timeout_ms: u32) -> bool {
        thread::sleep(Duration::from_millis(2));
        true
    }

    fn writable_frames(&mut self) -> Result<usize, FakeError> {
        Ok(BLOCK)
    }

    fn write(&mut self, frames: &[Frame]) -> Result<(), FakeError> {
        self.written.lock().unwrap().extend_from_slice(frames);
        Ok(())
    }
}

type Recording = Arc<Mutex<Vec<Frame>>>;

#[derive(Default)]
struct FakeBackend {
    outputs: Mutex<Vec<(DeviceId, Recording)>>,
}

impl FakeBackend {
    fn written(&self, id: &DeviceId) -> Vec<Frame> {
        let outputs = self.outputs.lock().unwrap();
        let (_, written) = outputs.iter().find(|(known, _)| known == id).unwrap();
        written.lock().unwrap().clone()
    }
}

impl Backend for FakeBackend {
    type Error = FakeError;
    type ThreadGuard = ();
    type Capture = FakeCapture;
    type Render = FakeRender;

    fn enter_thread(&self) -> Result<(), FakeError> {
        Ok(())
    }

    fn open_capture(&self) -> Result<FakeCapture, FakeError> {
        Ok(FakeCapture {
            next: 0,
            block: vec![Frame::SILENCE; BLOCK],
        })
    }

    fn open_render(&self, id: &DeviceId) -> Result<FakeRender, FakeError> {
        if id.as_str() == MISSING {
            return Err(FakeError);
        }
        let written = Arc::new(Mutex::new(Vec::new()));
        self.outputs
            .lock()
            .unwrap()
            .push((id.clone(), Arc::clone(&written)));
        Ok(FakeRender { written })
    }
}

fn id(raw: &str) -> DeviceId {
    DeviceId::new(raw).unwrap()
}

fn audible(frames: &[Frame]) -> Vec<Frame> {
    frames
        .iter()
        .copied()
        .filter(|f| *f != Frame::SILENCE)
        .collect()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

fn config(raw: &str, mode: ChannelMode) -> OutputConfig {
    OutputConfig {
        channel_mode: mode,
        ..OutputConfig::new(id(raw))
    }
}

#[test]
fn f1_f2_stereo_pair_receives_left_and_right_without_gaps() {
    let backend = Arc::new(FakeBackend::default());
    let session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[
            config("left", ChannelMode::LeftOnly),
            config("right", ChannelMode::RightOnly),
        ],
    )
    .unwrap();

    wait_until(|| {
        backend.outputs.lock().unwrap().len() == 2
            && audible(&backend.written(&id("left"))).len() >= 4_800
            && audible(&backend.written(&id("right"))).len() >= 4_800
    });
    session.stop();

    let left = audible(&backend.written(&id("left")));
    let right = audible(&backend.written(&id("right")));
    // Left device plays (x, x) for x = 1, 2, 3, ...; right device (-x, -x).
    for (i, pair) in left.windows(2).enumerate() {
        let x = pair[0].left;
        assert_eq!(
            pair[0],
            Frame::new(x, x),
            "left device frame {i} not mono-left"
        );
        assert_eq!(
            pair[1],
            Frame::new(x + 1.0, x + 1.0),
            "left device gap at {i}"
        );
    }
    for (i, pair) in right.windows(2).enumerate() {
        let x = pair[0].right;
        assert!(x < 0.0, "right device frame {i} not from right channel");
        assert_eq!(
            pair[0],
            Frame::new(x, x),
            "right device frame {i} not mono-right"
        );
        assert_eq!(
            pair[1],
            Frame::new(x - 1.0, x - 1.0),
            "right device gap at {i}"
        );
    }
}

#[test]
fn f6_failing_device_does_not_stop_the_others() {
    let backend = Arc::new(FakeBackend::default());
    let session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[
            config(MISSING, ChannelMode::Stereo),
            config("ok", ChannelMode::Stereo),
        ],
    )
    .unwrap();

    wait_until(|| {
        backend.outputs.lock().unwrap().len() == 1
            && audible(&backend.written(&id("ok"))).len() >= 4_800
    });
    let status = session.output_status();
    session.stop();

    assert_eq!(status[0].state, StreamState::Failed);
    assert_eq!(status[1].state, StreamState::Running);
}

#[test]
fn stop_joins_all_threads_promptly() {
    let backend = Arc::new(FakeBackend::default());
    let session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[config("a", ChannelMode::Stereo)],
    )
    .unwrap();
    wait_until(|| session.capture_state() == StreamState::Running);

    let started = Instant::now();
    session.stop();

    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn live_setting_change_on_unknown_device_is_rejected() {
    let backend = Arc::new(FakeBackend::default());
    let session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[config("a", ChannelMode::Stereo)],
    )
    .unwrap();

    assert!(session.set_channel_mode(&id("a"), ChannelMode::MonoMix));
    assert!(!session.set_channel_mode(&id("b"), ChannelMode::MonoMix));
}
