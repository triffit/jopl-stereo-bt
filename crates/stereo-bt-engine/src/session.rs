use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use rtrb::{Consumer, RingBuffer};
use stereo_bt_core::{ChannelMode, Delay, DeviceId, Frame, Playout, SampleRate, Volume};

use crate::backend::{Backend, CaptureStream, RenderSink};
use crate::fan_out::FanOut;
use crate::output_controls::OutputControls;
use crate::render_pipeline::RenderPipeline;
use crate::state_cell::StateCell;
use crate::tuning::{CUSHION_FRAMES, MAX_EXCESS_FRAMES, QUEUE_FRAMES, WAIT_TIMEOUT_MS};
use crate::{OutputConfig, OutputStatus, StreamState};

/// A running route: one capture thread fanning out to one render thread per
/// output device. Stops and joins all threads when dropped.
pub struct Session {
    stop: Arc<AtomicBool>,
    capture_state: Arc<StateCell>,
    outputs: Vec<(DeviceId, Arc<OutputControls>)>,
    threads: Vec<JoinHandle<()>>,
}

impl Session {
    /// Starts streaming from the backend's capture source to every output.
    ///
    /// Returns immediately; streams open on their own threads. A device that
    /// fails to open, or fails later, is reported as [`StreamState::Failed`]
    /// in [`Session::output_status`] without affecting the other outputs.
    ///
    /// # Errors
    ///
    /// Returns an error when a thread cannot be spawned.
    pub fn start<B: Backend>(
        backend: &Arc<B>,
        sample_rate: SampleRate,
        configs: &[OutputConfig],
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let capture_state = Arc::new(StateCell::new());
        let mut session = Self {
            stop: Arc::clone(&stop),
            capture_state: Arc::clone(&capture_state),
            outputs: Vec::with_capacity(configs.len()),
            threads: Vec::with_capacity(configs.len() + 1),
        };

        let mut producers = Vec::with_capacity(configs.len());
        for config in configs {
            let controls = Arc::new(OutputControls::new(config));
            let (producer, consumer) = RingBuffer::new(QUEUE_FRAMES);
            producers.push((producer, Arc::clone(&controls)));
            session
                .outputs
                .push((config.id.clone(), Arc::clone(&controls)));

            let backend = Arc::clone(backend);
            let stop = Arc::clone(&stop);
            let id = config.id.clone();
            let handle = thread::Builder::new()
                .name(format!("render {id}"))
                .spawn(move || {
                    let state = match render_thread(
                        &*backend,
                        &id,
                        consumer,
                        sample_rate,
                        &controls,
                        &stop,
                    ) {
                        Ok(()) => StreamState::Stopped,
                        Err(_) => StreamState::Failed,
                    };
                    controls.set_state(state);
                })?;
            session.threads.push(handle);
        }

        let backend = Arc::clone(backend);
        let handle = thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                let state = match capture_thread(
                    &*backend,
                    FanOut::new(producers),
                    &capture_state,
                    &stop,
                ) {
                    Ok(()) => StreamState::Stopped,
                    Err(_) => StreamState::Failed,
                };
                capture_state.set(state);
            })?;
        session.threads.push(handle);
        Ok(session)
    }

    /// State of the capture stream.
    #[must_use]
    pub fn capture_state(&self) -> StreamState {
        self.capture_state.get()
    }

    /// Health of every output, in configuration order.
    #[must_use]
    pub fn output_status(&self) -> Vec<OutputStatus> {
        self.outputs
            .iter()
            .map(|(id, c)| OutputStatus {
                id: id.clone(),
                state: c.state(),
                underruns: c.underruns(),
                overruns: c.overruns(),
                trims: c.trims(),
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
        match self.outputs.iter().find(|(known, _)| known == id) {
            Some((_, controls)) => {
                apply(controls);
                true
            }
            None => false,
        }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            // A panicked stream thread has already stopped; nothing to recover.
            let _ = handle.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn capture_thread<B: Backend>(
    backend: &B,
    mut fan_out: FanOut,
    state: &StateCell,
    stop: &AtomicBool,
) -> Result<(), B::Error> {
    let _guard = backend.enter_thread()?;
    let mut capture = backend.open_capture()?;
    capture.start()?;
    state.set(StreamState::Running);
    while !stop.load(Ordering::Relaxed) {
        let frames = capture.read(WAIT_TIMEOUT_MS)?;
        fan_out.push(frames);
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
    let mut pipeline = RenderPipeline::new(
        queue,
        Playout::new(CUSHION_FRAMES, MAX_EXCESS_FRAMES),
        sample_rate,
        Arc::clone(controls),
    );
    let mut block = vec![Frame::SILENCE; render.buffer_frames()];
    render.start()?;
    controls.set_state(StreamState::Running);
    while !stop.load(Ordering::Relaxed) {
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
