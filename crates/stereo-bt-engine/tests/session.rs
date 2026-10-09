//! Integration tests for a [`Session`] running real threads on a fake backend.

// The fake backend below is test-only support code outside #[test] functions,
// where clippy's allow-unwrap-in-tests does not reach; panicking is the intent.
#![allow(clippy::unwrap_used)]

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use stereo_bt_core::{ChannelMode, DeviceId, Frame, SampleRate};
use stereo_bt_engine::{Backend, CaptureStream, OutputConfig, RenderSink, Session, StreamState};

const BLOCK: usize = 480;
const MISSING: &str = "missing-device";
/// The engine's queue cushion (40 ms at 48 kHz).
const CUSHION: usize = 1_920;

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
    produced: Arc<AtomicUsize>,
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
            // A power-of-two step keeps every value exact in f32, and the
            // ramp below full scale, so the output limiter leaves it alone.
            let value = f32::from(u16::try_from(self.next % 60_000).unwrap()) * STEP;
            *frame = Frame::new(value, -value);
        }
        self.produced.fetch_add(BLOCK, Ordering::Relaxed);
        Ok(&self.block)
    }
}

/// Render behaves like a device on the same clock as the capture: it asks
/// for a block only once that block plus the cushion has been produced, so
/// it neither underruns nor lets the queue grow.
struct FakeRender {
    written: Recording,
    produced: Arc<AtomicUsize>,
    rendered: usize,
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
        if self.produced.load(Ordering::Relaxed) >= self.rendered + CUSHION + BLOCK {
            true
        } else {
            thread::sleep(Duration::from_millis(1));
            false
        }
    }

    fn writable_frames(&mut self) -> Result<usize, FakeError> {
        Ok(BLOCK)
    }

    fn write(&mut self, frames: &[Frame]) -> Result<(), FakeError> {
        self.written.lock().unwrap().extend_from_slice(frames);
        self.rendered += frames.len();
        Ok(())
    }
}

type Recording = Arc<Mutex<Vec<Frame>>>;

#[derive(Default)]
struct FakeBackend {
    outputs: Mutex<Vec<(DeviceId, Recording)>>,
    produced: Arc<AtomicUsize>,
}

impl FakeBackend {
    /// Everything written to `id` so far; empty while it has not opened yet.
    fn written(&self, id: &DeviceId) -> Vec<Frame> {
        let outputs = self.outputs.lock().unwrap();
        outputs
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, written)| written.lock().unwrap().clone())
            .unwrap_or_default()
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
            produced: Arc::clone(&self.produced),
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
        Ok(FakeRender {
            written,
            produced: Arc::clone(&self.produced),
            rendered: 0,
        })
    }
}

fn id(raw: &str) -> DeviceId {
    DeviceId::new(raw).unwrap()
}

/// Frames clearly above silence; drops the drift resampler's tiny
/// interpolation tail where playback starts.
fn audible(frames: &[Frame]) -> Vec<Frame> {
    frames
        .iter()
        .copied()
        .filter(|f| f.left.abs() > 0.5 * STEP || f.right.abs() > 0.5 * STEP)
        .collect()
}

/// One step of the test ramp.
const STEP: f32 = 1.0 / 65_536.0;

/// The drift resampler may play a hair faster or slower, so consecutive ramp
/// values differ by one step within interpolation tolerance.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01 * STEP
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
    // Left device plays (x, x) for x = 1, 2, 3, ... steps; right device (-x, -x).
    for (i, pair) in left.windows(2).enumerate() {
        let x = pair[0].left;
        assert!(x > 0.0, "left device frame {i} not from left channel");
        assert!(
            close(pair[0].right, x),
            "left device frame {i} not mono-left"
        );
        assert!(
            close(pair[1].left, x + STEP),
            "left device gap at {i}: {pair:?}"
        );
    }
    for (i, pair) in right.windows(2).enumerate() {
        let x = pair[0].right;
        assert!(x < 0.0, "right device frame {i} not from right channel");
        assert!(
            close(pair[0].left, x),
            "right device frame {i} not mono-right"
        );
        assert!(
            close(pair[1].right, x - STEP),
            "right device gap at {i}: {pair:?}"
        );
    }
}

/// Asserts `frames` (audible only) is one continuous ramp of mono `sign` values.
fn assert_continuous(frames: &[Frame], sign: f32) {
    for (i, pair) in frames.windows(2).enumerate() {
        let x = pair[0].left;
        assert!(x * sign > 0.0, "frame {i} from the wrong channel");
        assert!(close(pair[0].right, x), "frame {i} not mono");
        assert!(close(pair[1].left, x + sign * STEP), "gap at {i}: {pair:?}");
    }
}

#[test]
fn adding_a_device_while_streaming_leaves_the_others_uninterrupted() {
    let backend = Arc::new(FakeBackend::default());
    let mut session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[config("left", ChannelMode::LeftOnly)],
    )
    .unwrap();
    wait_until(|| audible(&backend.written(&id("left"))).len() >= 2_400);

    session
        .add_output(&config("right", ChannelMode::RightOnly))
        .unwrap();
    wait_until(|| {
        backend.outputs.lock().unwrap().len() == 2
            && audible(&backend.written(&id("right"))).len() >= 2_400
    });
    session.stop();

    assert_continuous(&audible(&backend.written(&id("left"))), 1.0);
    assert_continuous(&audible(&backend.written(&id("right"))), -1.0);
}

#[test]
fn removing_a_device_stops_only_that_device() {
    let backend = Arc::new(FakeBackend::default());
    let mut session = Session::start(
        &backend,
        SampleRate::HZ_48000,
        &[
            config("a", ChannelMode::Stereo),
            config("b", ChannelMode::Stereo),
        ],
    )
    .unwrap();
    wait_until(|| backend.outputs.lock().unwrap().len() == 2);

    assert!(session.remove_output(&id("b")));

    assert_eq!(session.output_ids(), vec![id("a")]);
    assert!(!session.remove_output(&id("b")));
    session.stop();
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
