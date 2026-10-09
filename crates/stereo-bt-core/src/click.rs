use std::f32::consts::{PI, TAU};

use crate::{Frame, SampleRate};

/// Click length: short enough to locate precisely by ear, long enough to be
/// clearly audible on small Bluetooth speakers.
const CLICK_MILLIS: u32 = 5;
/// Tone of the click; 2 kHz is where small speakers and ears are sensitive.
const CLICK_HZ: f32 = 2_000.0;
const CLICK_AMPLITUDE: f32 = 0.6;

/// A metronome-like click track for assisted sync: one short tone burst at
/// the start of every period, silence in between.
///
/// Allocation-free; safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct ClickGenerator {
    rate: f32,
    period: usize,
    click_len: usize,
    /// Frames since the start of the current period.
    position: usize,
}

impl ClickGenerator {
    /// A click every `period_millis` milliseconds, the first one immediately.
    #[must_use]
    pub fn new(sample_rate: SampleRate, period_millis: u32) -> Self {
        let hz = sample_rate.as_hz();
        let frames = |millis: u32| usize::try_from(u64::from(millis) * u64::from(hz) / 1_000);
        let click_len = frames(CLICK_MILLIS).unwrap_or(1).max(1);
        Self {
            // Sample rates are at most 384 kHz, exact in f32.
            #[allow(clippy::cast_precision_loss)]
            rate: hz as f32,
            period: frames(period_millis)
                .unwrap_or(usize::MAX)
                .max(click_len + 1),
            click_len,
            position: 0,
        }
    }

    /// Fills `out` and returns the frame index in `out` where a click starts,
    /// if one does.
    pub fn fill(&mut self, out: &mut [Frame]) -> Option<usize> {
        let mut start = None;
        for (index, frame) in out.iter_mut().enumerate() {
            if self.position == 0 && start.is_none() {
                start = Some(index);
            }
            let sample = if self.position < self.click_len {
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
        // Hann window: a smooth onset and decay, so the burst does not pop.
        let envelope = 0.5 - 0.5 * (TAU * n / to_f32(self.click_len)).cos();
        CLICK_AMPLITUDE * envelope * (2.0 * PI * CLICK_HZ * n / self.rate).sin()
    }
}

fn to_f32(value: usize) -> f32 {
    // Click positions are below a few hundred, exact in f32.
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}
