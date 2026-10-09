use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use rtrb::{Consumer, Producer, RingBuffer};
use stereo_bt_core::{ChannelMode, Delay, DeviceId, Frame, Playout, SampleRate, Volume};

use crate::backend::{Backend, CaptureStream, RenderSink};
use crate::fan_out::FanOut;
use crate::output_controls::OutputControls;
use crate::render_pipeline::RenderPipeline;
use crate::state_cell::StateCell;
use crate::tuning::{CUSHION_FRAMES, MAX_EXCESS_FRAMES, QUEUE_FRAMES, WAIT_TIMEOUT_MS};
use crate::{OutputConfig, OutputStatus, StreamState};

/// Most extra packets read after one wake-up of the capture thread.
const MAX_DRAINED_PACKETS: usize = 16;

/// A new output queue handed to the capture thread.
type NewOutput = (Producer<Frame>, Arc<OutputControls>);

/// State and last error of the capture thread.
#[derive(Debug, Default)]
struct CaptureShared {
    state: StateCell,
    error: Mutex<Option<String>>,
}

struct OutputHandle {
    id: DeviceId,
    controls: Arc<OutputControls>,
    thread: Option<JoinHandle<()>>,
}

/// A running route: one capture thread fanning out to one render thread per
/// output device. Outputs can be added, removed and restarted while it runs
/// without disturbing the others. Stops and joins all threads when dropped.
pub struct Session<B: Backend> {
    backend: Arc<B>,
    sample_rate: SampleRate,
    stop: Arc<AtomicBool>,
    capture: Arc<CaptureShared>,
    capture_thread: Option<JoinHandle<()>>,
    new_outputs: Sender<NewOutput>,
    outputs: Vec<OutputHandle>,
    master_gain: f32,
    /// The only output left audible, while isolating one for assisted sync.
    isolated: Option<DeviceId>,
}

impl<B: Backend> Session<B> {
    /// Starts streaming from the backend's capture source to every output.
    ///
    /// Returns immediately; streams open on their own threads. A device that
    /// fails to open, or fails later, is reported as [`StreamState::Failed`]
    /// (with its error) in [`Session::output_status`] without affecting the
    /// other outputs.
    ///
    /// # Errors
    ///
    /// Returns an error when a thread cannot be spawned.
    pub fn start(
        backend: &Arc<B>,
        sample_rate: SampleRate,
        configs: &[OutputConfig],
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let capture = Arc::new(CaptureShared::default());
        let (new_outputs, new_output_rx) = mpsc::channel();

        let capture_thread = {
            let backend = Arc::clone(backend);
            let stop = Arc::clone(&stop);
            let capture = Arc::clone(&capture);
            thread::Builder::new()
                .name("capture".into())
                .spawn(move || {
                    let result = capture_thread(&*backend, &new_output_rx, &capture.state, &stop);
                    finish(&capture.state, &capture.error, result);
                })?
        };

        let mut session = Self {
            backend: Arc::clone(backend),
            sample_rate,
            stop,
            capture,
            capture_thread: Some(capture_thread),
            new_outputs,
            outputs: Vec::with_capacity(configs.len()),
            master_gain: 1.0,
            isolated: None,
        };
        for config in configs {
            session.add_output(config)?;
        }
        Ok(session)
    }

    /// Starts playing on one more device; replaces it if it is already present.
    ///
    /// # Errors
    ///
    /// Returns an error when the render thread cannot be spawned.
    pub fn add_output(&mut self, config: &OutputConfig) -> std::io::Result<()> {
        self.remove_output(&config.id);
        let controls = Arc::new(OutputControls::new(config));
        controls.set_master_gain(self.master_gain);
        controls.set_silenced(self.is_silenced(&config.id));
        let (producer, consumer) = RingBuffer::new(QUEUE_FRAMES);

        let backend = Arc::clone(&self.backend);
        let stop = Arc::clone(&self.stop);
        let thread_controls = Arc::clone(&controls);
        let id = config.id.clone();
        let sample_rate = self.sample_rate;
        let thread = thread::Builder::new()
            .name(format!("render {id}"))
            .spawn(move || {
                let result = render_thread(
                    &*backend,
                    &id,
                    consumer,
                    sample_rate,
                    &thread_controls,
                    &stop,
                );
                if let Err(error) = result {
                    thread_controls.set_error(error.to_string());
                    thread_controls.set_state(StreamState::Failed);
                } else {
                    thread_controls.set_state(StreamState::Stopped);
                }
            })?;

        // A closed channel means the capture thread has ended; the output then
        // simply receives no audio, and the capture failure is reported.
        let _ = self.new_outputs.send((producer, Arc::clone(&controls)));
        self.outputs.push(OutputHandle {
            id: config.id.clone(),
            controls,
            thread: Some(thread),
        });
        Ok(())
    }

    /// Stops playing on a device. Returns `false` when it is not in the session.
    pub fn remove_output(&mut self, id: &DeviceId) -> bool {
        let Some(index) = self.outputs.iter().position(|o| &o.id == id) else {
            return false;
        };
        let mut output = self.outputs.remove(index);
        output.controls.request_stop();
        if let Some(thread) = output.thread.take() {
            // A panicked render thread has already stopped; nothing to recover.
            let _ = thread.join();
        }
        // Its queue is now abandoned and pruned by the capture thread.
        true
    }

    /// Scales every output (current and future) by `amplitude`, clamped to
    /// `0.0..=1.0`; changes ramp smoothly on the render threads.
    pub fn set_master_gain(&mut self, amplitude: f32) {
        self.master_gain = amplitude.clamp(0.0, 1.0);
        for output in &self.outputs {
            output.controls.set_master_gain(self.master_gain);
        }
    }

    /// Leaves only `device` audible (`None`: all outputs audible again).
    /// Isolating a device outside the session silences every output.
    pub fn isolate(&mut self, device: Option<&DeviceId>) {
        self.isolated = device.cloned();
        for output in &self.outputs {
            output.controls.set_silenced(self.is_silenced(&output.id));
        }
    }

    /// Whether `id` is currently silenced by isolation (test helper).
    #[cfg(test)]
    pub(crate) fn isolated_for_test(&self, id: &DeviceId) -> bool {
        self.is_silenced(id)
    }

    fn is_silenced(&self, id: &DeviceId) -> bool {
        self.isolated
            .as_ref()
            .is_some_and(|isolated| isolated != id)
    }

    /// The gain shared by all outputs.
    #[must_use]
    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// The devices in this session, in the order they were added.
    #[must_use]
    pub fn output_ids(&self) -> Vec<DeviceId> {
        self.outputs.iter().map(|o| o.id.clone()).collect()
    }

    /// State of the capture stream.
    #[must_use]
    pub fn capture_state(&self) -> StreamState {
        self.capture.state.get()
    }

    /// Why the capture stream failed, if it did.
    #[must_use]
    pub fn capture_error(&self) -> Option<String> {
        lock(&self.capture.error).clone()
    }

    /// Health of every output, in the order they were added.
    #[must_use]
    pub fn output_status(&self) -> Vec<OutputStatus> {
        self.outputs
            .iter()
            .map(|o| OutputStatus {
                id: o.id.clone(),
                state: o.controls.state(),
                underruns: o.controls.underruns(),
                overruns: o.controls.overruns(),
                trims: o.controls.trims(),
                error: o.controls.error(),
            })
            .collect()
    }

    /// Changes an output's sync delay live. Returns `false` for an unknown device.
    #[must_use]
    pub fn set_delay(&self, id: &DeviceId, delay: Delay) -> bool {
        self.with_output(id, |c| c.set_delay(delay))
    }

    /// Changes an output's volume live. Returns `false` for an unknown device.
    #[must_use]
    pub fn set_volume(&self, id: &DeviceId, volume: Volume) -> bool {
        self.with_output(id, |c| c.set_volume(volume))
    }

    /// Changes an output's channel mode live. Returns `false` for an unknown device.
    #[must_use]
    pub fn set_channel_mode(&self, id: &DeviceId, mode: ChannelMode) -> bool {
        self.with_output(id, |c| c.set_channel_mode(mode))
    }

    /// Stops all streams and waits for their threads to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn with_output(&self, id: &DeviceId, apply: impl FnOnce(&OutputControls)) -> bool {
        match self.outputs.iter().find(|o| &o.id == id) {
            Some(output) => {
                apply(&output.controls);
                true
            }
            None => false,
        }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let threads = self
            .outputs
            .iter_mut()
            .filter_map(|o| o.thread.take())
            .chain(self.capture_thread.take());
        for thread in threads {
            // A panicked stream thread has already stopped; nothing to recover.
            let _ = thread.join();
        }
    }
}

impl<B: Backend> Drop for Session<B> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn finish<E: Display>(state: &StateCell, error: &Mutex<Option<String>>, result: Result<(), E>) {
    match result {
        Ok(()) => state.set(StreamState::Stopped),
        Err(e) => {
            *lock(error) = Some(e.to_string());
            state.set(StreamState::Failed);
        }
    }
}

fn capture_thread<B: Backend>(
    backend: &B,
    new_outputs: &Receiver<NewOutput>,
    state: &StateCell,
    stop: &AtomicBool,
) -> Result<(), B::Error> {
    let _guard = backend.enter_thread()?;
    let mut capture = backend.open_capture()?;
    capture.start()?;
    state.set(StreamState::Running);
    let mut fan_out = FanOut::new(Vec::new());
    while !stop.load(Ordering::Relaxed) {
        // Rare and cheap: only when an output is added.
        for (producer, controls) in new_outputs.try_iter() {
            fan_out.add(producer, controls);
        }
        let frames = capture.read(WAIT_TIMEOUT_MS)?;
        fan_out.push(frames);
        // Drain packets that arrived together, so capture never falls behind
        // (a microphone was measured delivering several per wake-up). Bounded,
        // so a source that always has data cannot starve the stop check.
        for _ in 0..MAX_DRAINED_PACKETS {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let more = capture.read(0)?;
            if more.is_empty() {
                break;
            }
            fan_out.push(more);
        }
    }
    Ok(())
}

fn render_thread<B: Backend>(
    backend: &B,
    id: &DeviceId,
    queue: Consumer<Frame>,
    sample_rate: SampleRate,
    controls: &Arc<OutputControls>,
    stop: &AtomicBool,
) -> Result<(), B::Error> {
    let _guard = backend.enter_thread()?;
    let mut render = backend.open_render(id)?;
    let mut block = vec![Frame::SILENCE; render.buffer_frames()];
    let mut pipeline = RenderPipeline::new(
        queue,
        Playout::new(CUSHION_FRAMES, MAX_EXCESS_FRAMES),
        CUSHION_FRAMES,
        block.len(),
        sample_rate,
        Arc::clone(controls),
    );
    render.start()?;
    controls.set_state(StreamState::Running);
    while !stop.load(Ordering::Relaxed) && !controls.stop_requested() {
        if !render.wait(WAIT_TIMEOUT_MS) {
            continue;
        }
        let writable = render.writable_frames()?.min(block.len());
        let block = &mut block[..writable];
        pipeline.fill(block);
        render.write(block)?;
    }
    Ok(())
}
