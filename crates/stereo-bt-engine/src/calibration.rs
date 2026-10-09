//! Automatic latency measurement with a microphone: plays test chirps on
//! one device at a time, finds them in a microphone recording, and plans
//! the best default device and delays (see `stereo_bt_core::plan_first_round`).

use stereo_bt_core::{CalibrationOutcome, Delay, DeviceId, DeviceLatency};

/// What the measurement needs to know about the current setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalibrationRequest {
    /// The sync delay every device has now.
    pub delays: Vec<(DeviceId, Delay)>,
    /// Whether the default device plays (otherwise it is muted).
    pub include_default: bool,
}

/// The result of a measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationReport {
    /// The microphone that listened.
    pub microphone: String,
    /// The latest measurement round.
    pub measured: Vec<DeviceLatency>,
    /// Devices whose chirps were not heard (too quiet or too far away).
    pub not_heard: Vec<DeviceId>,
    /// The recommended default device and delays.
    pub outcome: CalibrationOutcome,
    /// Per device: chirps played, and the latency of each chirp clearly heard.
    pub heard: Vec<(DeviceId, usize, Vec<f64>)>,
}

/// Progress of a measurement, for the UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CalibrationState {
    /// Not measuring.
    #[default]
    Idle,
    /// Measuring; the text says what is happening.
    Running(String),
    /// Finished.
    Done(CalibrationReport),
    /// Stopped with an explanation.
    Failed(String),
}

#[cfg(windows)]
pub(crate) use windows_run::run;

#[cfg(windows)]
mod windows_run {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread;
    use std::time::{Duration, Instant};

    use stereo_bt_core::{
        CalibrationOutcome, ClickGenerator, Delay, DeviceId, DeviceLatency, plan_first_round,
        plan_second_round,
    };
    use stereo_bt_wasapi::{ComGuard, MicCapture, STREAM_SAMPLE_RATE};

    use super::{CalibrationReport, CalibrationRequest, CalibrationState};
    use crate::engine::Client;
    use crate::mic_timeline::{MicTimeline, robust_latency};
    use crate::supervisor::CHIRP_PERIOD_MILLIS;
    use crate::{EngineStatus, StreamState, TestSignal};

    /// Bluetooth speakers need a moment to wake up; the first chirps after
    /// isolating a device are not measured.
    const WARM_UP: Duration = Duration::from_millis(1_400);
    /// Chirps measured per device.
    const CHIRPS: u32 = 5;
    /// How long after each chirp's emission it may be heard.
    const SEARCH_WINDOW: Duration = Duration::from_millis(600);
    /// Time for streaming to restart after the default device changes.
    const SETTLE_AFTER_DEFAULT: Duration = Duration::from_secs(3);

    /// Runs a complete measurement, updating `state` as it goes. Always ends
    /// with the test signal off and every device audible again.
    pub(crate) fn run(
        client: &Client,
        request: &CalibrationRequest,
        state: &Mutex<CalibrationState>,
        cancel: &AtomicBool,
    ) {
        let result = measure(client, request, state, cancel);
        // Best effort: the engine also resets these when streaming stops.
        let _ = client.isolate(None);
        let _ = client.set_test_signal(None);
        set(
            state,
            match result {
                Ok(report) => CalibrationState::Done(report),
                Err(message) => CalibrationState::Failed(message),
            },
        );
    }

    fn measure(
        client: &Client,
        request: &CalibrationRequest,
        state: &Mutex<CalibrationState>,
        cancel: &AtomicBool,
    ) -> Result<CalibrationReport, String> {
        let timeline = Arc::new(Mutex::new(MicTimeline::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let recorder = {
            let timeline = Arc::clone(&timeline);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("microphone".into())
                .spawn(move || record(&timeline, &stop, &ready_tx))
                .map_err(|e| e.to_string())?
        };
        let microphone = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "The microphone did not start.".to_owned())
            .and_then(|ready| ready);

        let result = microphone.and_then(|microphone| {
            let rounds = Rounds {
                client,
                request,
                timeline: &timeline,
                state,
                cancel,
            };
            rounds.run(microphone)
        });
        stop.store(true, Ordering::Relaxed);
        // A panicked recorder has already stopped.
        let _ = recorder.join();
        result
    }

    /// Records the microphone into `timeline` until `stop`.
    fn record(
        timeline: &Mutex<MicTimeline>,
        stop: &AtomicBool,
        ready: &std::sync::mpsc::Sender<Result<String, String>>,
    ) {
        let opened = ComGuard::init_mta()
            .and_then(|com| Ok((com, MicCapture::open()?)))
            .and_then(|(com, mic)| {
                mic.start()?;
                Ok((com, mic))
            });
        let (_com, mut mic) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                let _ = ready.send(Err(format!("The microphone could not be opened: {e}")));
                return;
            }
        };
        let _ = ready.send(Ok(mic.name().to_owned()));
        // Wait for the next packet only when nothing is queued: the
        // microphone may deliver several packets per wake-up, and reading one
        // per wake-up falls behind until Windows drops audio (measured).
        let mut timeout_ms = 20;
        while !stop.load(Ordering::Relaxed) {
            let Ok(packet) = mic.read(timeout_ms) else {
                return;
            };
            if packet.is_empty() {
                timeout_ms = 20;
                continue;
            }
            timeout_ms = 0;
            // Copied so the packet can be dated with the microphone's queue
            // (this thread is not real-time; a small allocation is fine).
            let packet = packet.to_vec();
            let now = Instant::now();
            let queued = mic.queued_frames().unwrap_or(0);
            let age = frames_to_duration(packet.len() + queued);
            let first = now.checked_sub(age).unwrap_or(now);
            lock(timeline).push(first, &packet);
        }
    }

    struct Rounds<'a> {
        client: &'a Client,
        request: &'a CalibrationRequest,
        timeline: &'a Mutex<MicTimeline>,
        state: &'a Mutex<CalibrationState>,
        cancel: &'a AtomicBool,
    }

    impl Rounds<'_> {
        fn run(&self, microphone: String) -> Result<CalibrationReport, String> {
            self.client
                .set_test_signal(Some(TestSignal::Chirp))
                .map_err(|e| e.to_string())?;
            let (first, not_heard, heard) = self.round()?;

            if !self.request.include_default {
                return Ok(CalibrationReport {
                    microphone,
                    outcome: CalibrationOutcome::Aligned {
                        delays: stereo_bt_core::align_to_slowest(&first),
                    },
                    measured: first,
                    not_heard,
                    heard,
                });
            }
            let outcome = plan_first_round(&first).ok_or_else(|| self.not_heard_message(&heard))?;
            let CalibrationOutcome::TryDefault(candidate) = outcome else {
                return Ok(CalibrationReport {
                    microphone,
                    measured: first,
                    not_heard,
                    outcome,
                    heard,
                });
            };

            self.progress(&format!(
                "{} plays later than the default device; trying it as the default…",
                self.name(&candidate)
            ));
            self.client
                .make_default(candidate)
                .map_err(|e| e.to_string())?;
            self.wait(SETTLE_AFTER_DEFAULT)?;
            self.client
                .set_test_signal(Some(TestSignal::Chirp))
                .map_err(|e| e.to_string())?;
            let (second, not_heard, heard) = self.round()?;
            let outcome =
                plan_second_round(&first, &second).ok_or_else(|| self.not_heard_message(&heard))?;
            Ok(CalibrationReport {
                microphone,
                measured: second,
                not_heard,
                outcome,
                heard,
            })
        }

        /// Measures the default device (when it plays) and every playing device.
        /// Measured devices, devices not heard, and per device the chirps
        /// played and heard.
        #[allow(clippy::type_complexity)]
        fn round(
            &self,
        ) -> Result<
            (
                Vec<DeviceLatency>,
                Vec<DeviceId>,
                Vec<(DeviceId, usize, Vec<f64>)>,
            ),
            String,
        > {
            let status = self.client.status();
            let source = status
                .source
                .clone()
                .ok_or_else(|| "Start streaming first.".to_owned())?;
            let mut steps: Vec<(DeviceId, bool)> = Vec::new();
            if self.request.include_default {
                steps.push((source, true));
            }
            steps.extend(
                status
                    .outputs
                    .iter()
                    .filter(|o| o.state == StreamState::Running)
                    .map(|o| (o.id.clone(), false)),
            );
            if steps.len() < 2 {
                return Err("At least two playing devices are needed.".into());
            }

            let template =
                ClickGenerator::chirp(STREAM_SAMPLE_RATE, CHIRP_PERIOD_MILLIS).template();
            let mut measured = Vec::new();
            let mut not_heard = Vec::new();
            let mut heard = Vec::new();
            for (index, (id, is_source)) in steps.iter().enumerate() {
                self.progress(&format!(
                    "Measuring {} ({} of {})…",
                    self.name(id),
                    index + 1,
                    steps.len()
                ));
                self.client
                    .isolate(Some(id.clone()))
                    .map_err(|e| e.to_string())?;
                self.wait(WARM_UP)?;
                let from = Instant::now();
                self.wait(Duration::from_millis(u64::from(
                    CHIRPS * CHIRP_PERIOD_MILLIS,
                )))?;
                self.wait(SEARCH_WINDOW)?;
                let until = from + Duration::from_millis(u64::from(CHIRPS * CHIRP_PERIOD_MILLIS));
                let emissions: Vec<Instant> = self
                    .client
                    .status()
                    .click_times
                    .into_iter()
                    .filter(|t| (from..until).contains(t))
                    .collect();
                let latencies =
                    lock(self.timeline).latencies_ms(&emissions, &template, SEARCH_WINDOW);
                heard.push((id.clone(), emissions.len(), latencies.clone()));
                match robust_latency(&latencies) {
                    Some(measured_ms) => measured.push(DeviceLatency {
                        id: id.clone(),
                        is_source: *is_source,
                        measured_ms,
                        delay: self.delay(id),
                    }),
                    None => not_heard.push(id.clone()),
                }
            }
            self.client.isolate(None).map_err(|e| e.to_string())?;
            Ok((measured, not_heard, heard))
        }

        fn delay(&self, id: &DeviceId) -> Delay {
            self.request
                .delays
                .iter()
                .find(|(known, _)| known == id)
                .map_or(Delay::ZERO, |(_, delay)| *delay)
        }

        fn name(&self, id: &DeviceId) -> String {
            device_name(&self.client.status(), id)
        }

        fn not_heard_message(&self, heard: &[(DeviceId, usize, Vec<f64>)]) -> String {
            let details: Vec<String> = heard
                .iter()
                .map(|(id, played, clear)| {
                    format!("{}: {} of {played} heard", self.name(id), clear.len())
                })
                .collect();
            format!(
                "The default device was not heard clearly. Turn it up, move it closer to the microphone and pause other audio. ({})",
                details.join("; ")
            )
        }

        fn progress(&self, text: &str) {
            set(self.state, CalibrationState::Running(text.to_owned()));
        }

        /// Sleeps in small steps so a cancel takes effect promptly.
        fn wait(&self, duration: Duration) -> Result<(), String> {
            let until = Instant::now() + duration;
            while Instant::now() < until {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err("Measurement cancelled.".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        }
    }

    fn device_name(status: &EngineStatus, id: &DeviceId) -> String {
        status
            .devices
            .iter()
            .find(|d| &d.id == id)
            .map_or_else(|| id.to_string(), |d| d.name.clone())
    }

    fn frames_to_duration(frames: usize) -> Duration {
        let frames = u64::try_from(frames).unwrap_or(u64::MAX);
        Duration::from_micros(
            frames.saturating_mul(1_000_000) / u64::from(STREAM_SAMPLE_RATE.as_hz()),
        )
    }

    fn set(state: &Mutex<CalibrationState>, value: CalibrationState) {
        *lock(state) = value;
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
