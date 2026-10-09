use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use stereo_bt_core::{ClickGenerator, DeviceId, Frame, SampleRate};

use crate::backend::{Backend, RenderSink};
use crate::tuning::WAIT_TIMEOUT_MS;

/// How many recent click times are kept for tap analysis.
const KEPT_CLICKS: usize = 64;

/// Plays a click track into the default device for assisted sync.
///
/// Windows plays it on the default device itself, and because the default
/// device is the loopback source, the same clicks also reach every other
/// device through its real pipeline and delay. Each click's emission time is
/// recorded (including the audio already queued in the device buffer), so
/// taps can be related to it. Stops when dropped.
pub(crate) struct ClickTrack {
    device: DeviceId,
    stop: Arc<AtomicBool>,
    clicks: Arc<Mutex<VecDeque<Instant>>>,
    thread: Option<JoinHandle<()>>,
}

impl ClickTrack {
    /// Starts clicking on `device` every `period_millis` milliseconds.
    pub(crate) fn start<B: Backend>(
        backend: &Arc<B>,
        device: DeviceId,
        sample_rate: SampleRate,
        period_millis: u32,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let clicks = Arc::new(Mutex::new(VecDeque::with_capacity(KEPT_CLICKS)));
        let thread = {
            let backend = Arc::clone(backend);
            let stop = Arc::clone(&stop);
            let clicks = Arc::clone(&clicks);
            let device = device.clone();
            thread::Builder::new()
                .name("click track".into())
                .spawn(move || {
                    // A device that fails simply produces no clicks; the
                    // assistant then gets no taps and says so.
                    let _ = run(
                        &*backend,
                        &device,
                        sample_rate,
                        period_millis,
                        &clicks,
                        &stop,
                    );
                })?
        };
        Ok(Self {
            device,
            stop,
            clicks,
            thread: Some(thread),
        })
    }

    /// The device the clicks are played on.
    pub(crate) fn device(&self) -> &DeviceId {
        &self.device
    }

    /// When recent clicks leave the device, oldest first.
    pub(crate) fn clicks(&self) -> Vec<Instant> {
        lock(&self.clicks).iter().copied().collect()
    }
}

impl Drop for ClickTrack {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A panicked click thread has already stopped.
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn run<B: Backend>(
    backend: &B,
    device: &DeviceId,
    sample_rate: SampleRate,
    period_millis: u32,
    clicks: &Mutex<VecDeque<Instant>>,
    stop: &AtomicBool,
) -> Result<(), B::Error> {
    let _guard = backend.enter_thread()?;
    let mut render = backend.open_render(device)?;
    let mut generator = ClickGenerator::new(sample_rate, period_millis);
    let mut block = vec![Frame::SILENCE; render.buffer_frames()];
    render.start()?;
    while !stop.load(Ordering::Relaxed) {
        if !render.wait(WAIT_TIMEOUT_MS) {
            continue;
        }
        let writable = render.writable_frames()?.min(block.len());
        // Audio already queued in the device plays before this block.
        let queued = render.buffer_frames().saturating_sub(writable);
        let block = &mut block[..writable];
        if let Some(offset) = generator.fill(block) {
            let ahead = frames_to_duration(queued + offset, sample_rate);
            let mut recent = lock(clicks);
            if recent.len() == KEPT_CLICKS {
                recent.pop_front();
            }
            recent.push_back(Instant::now() + ahead);
        }
        render.write(block)?;
    }
    Ok(())
}

fn frames_to_duration(frames: usize, sample_rate: SampleRate) -> Duration {
    let frames = u64::try_from(frames).unwrap_or(u64::MAX);
    Duration::from_micros(frames.saturating_mul(1_000_000) / u64::from(sample_rate.as_hz()))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::test_fakes::{SharedWorld, SilentBackend, World};

    fn backend() -> Arc<SilentBackend> {
        let world: SharedWorld = Arc::new(Mutex::new(World::default()));
        Arc::new(SilentBackend::new(&world))
    }

    #[test]
    fn records_one_emission_time_per_click() {
        let device = DeviceId::new("A").unwrap();
        let track = ClickTrack::start(&backend(), device, SampleRate::HZ_48000, 50).unwrap();

        thread::sleep(Duration::from_millis(400));
        let clicks = track.clicks();
        drop(track);

        assert!(clicks.len() >= 3, "{} clicks", clicks.len());
        for pair in clicks.windows(2) {
            // The fake device consumes audio faster than real time, so only
            // check that clicks are recorded in order.
            assert!(pair[1] >= pair[0]);
        }
    }

    #[test]
    fn frames_convert_to_time_at_the_sample_rate() {
        assert_eq!(
            frames_to_duration(48_000, SampleRate::HZ_48000),
            Duration::from_secs(1)
        );
        assert_eq!(
            frames_to_duration(480, SampleRate::HZ_48000),
            Duration::from_millis(10)
        );
    }
}
