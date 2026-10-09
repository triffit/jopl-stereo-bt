use std::f32::consts::{PI, TAU};

use crate::{Frame, SampleRate};

/// Click length: short enough to locate precisely by ear, long enough to be
/// clearly audible on small Bluetooth speakers.
const CLICK_MILLIS: u32 = 5;
/// Tone of the click; 2 kHz is where small speakers and ears are sensitive.
const CLICK_HZ: f32 = 2_000.0;
const CLICK_AMPLITUDE: f32 = 0.6;

/// Chirp length and sweep for microphone measurement: a sweep correlates
/// sharply (one clear peak) and stays within what small speakers and laptop
/// microphones reproduce well.
const CHIRP_MILLIS: u32 = 60;
const CHIRP_FROM_HZ: f32 = 500.0;
const CHIRP_TO_HZ: f32 = 4_000.0;
const CHIRP_AMPLITUDE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Burst {
    /// Short tone burst, for listening.
    Click,
    /// Frequency sweep, for locating precisely in a microphone recording.
    Chirp,
}

/// A metronome-like test-signal track: one short burst (a click or a chirp)
/// at the start of every period, silence in between.
///
/// Allocation-free while filling; safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct ClickGenerator {
    burst: Burst,
    rate: f32,
    period: usize,
    burst_len: usize,
    /// Frames since the start of the current period.
    position: usize,
}

impl ClickGenerator {
    /// A click every `period_millis` milliseconds, the first one immediately.
    #[must_use]
    pub fn new(sample_rate: SampleRate, period_millis: u32) -> Self {
        Self::with_burst(Burst::Click, sample_rate, period_millis)
    }

    /// A chirp every `period_millis` milliseconds, the first one immediately.
    #[must_use]
    pub fn chirp(sample_rate: SampleRate, period_millis: u32) -> Self {
        Self::with_burst(Burst::Chirp, sample_rate, period_millis)
    }

    fn with_burst(burst: Burst, sample_rate: SampleRate, period_millis: u32) -> Self {
        let hz = sample_rate.as_hz();
        let frames = |millis: u32| usize::try_from(u64::from(millis) * u64::from(hz) / 1_000);
        let millis = match burst {
            Burst::Click => CLICK_MILLIS,
            Burst::Chirp => CHIRP_MILLIS,
        };
        let burst_len = frames(millis).unwrap_or(1).max(1);
        Self {
            burst,
            // Sample rates are at most 384 kHz, exact in f32.
            #[allow(clippy::cast_precision_loss)]
            rate: hz as f32,
            period: frames(period_millis)
                .unwrap_or(usize::MAX)
                .max(burst_len + 1),
            burst_len,
            position: 0,
        }
    }

    /// One burst as mono samples, e.g. as the template to search for.
    #[must_use]
    pub fn template(&self) -> Vec<f32> {
        (0..self.burst_len).map(|n| self.sample(n)).collect()
    }

    /// Fills `out` and returns the frame index in `out` where a burst starts,
    /// if one does.
    pub fn fill(&mut self, out: &mut [Frame]) -> Option<usize> {
        let mut start = None;
        for (index, frame) in out.iter_mut().enumerate() {
            if self.position == 0 && start.is_none() {
                start = Some(index);
            }
            let sample = if self.position < self.burst_len {
                self.sample(self.position)
            } else {
                0.0
            };
            *frame = Frame::new(sample, sample);
            self.position = (self.position + 1) % self.period;
        }
        start
    }

    fn sample(&self, position: usize) -> f32 {
        let n = to_f32(position);
        let len = to_f32(self.burst_len);
        // Hann window: a smooth onset and decay, so the burst does not pop.
        let envelope = 0.5 - 0.5 * (TAU * n / len).cos();
        match self.burst {
            Burst::Click => {
                CLICK_AMPLITUDE * envelope * (2.0 * PI * CLICK_HZ * n / self.rate).sin()
            }
            Burst::Chirp => {
                let t = n / self.rate;
                let duration = len / self.rate;
                // Linear sweep: phase is the integral of the instantaneous frequency.
                let phase = TAU
                    * (CHIRP_FROM_HZ * t
                        + (CHIRP_TO_HZ - CHIRP_FROM_HZ) * t * t / (2.0 * duration));
                CHIRP_AMPLITUDE * envelope * phase.sin()
            }
        }
    }
}

fn to_f32(value: usize) -> f32 {
    // Burst positions are below a few thousand, exact in f32.
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}
