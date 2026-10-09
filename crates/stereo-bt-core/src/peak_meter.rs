use crate::{Frame, SampleRate};

/// How fast the shown level falls after a peak.
const FALL_DB_PER_SECOND: f32 = 30.0;
/// The quietest level a meter shows; anything below reads as empty.
const FLOOR_DB: f32 = -60.0;

/// A peak level meter with a smooth fall-off, as on a mixing desk.
/// Allocation-free; meant for the render thread.
#[derive(Debug, Clone)]
pub struct PeakMeter {
    level: f32,
    /// Per-frame fall factor.
    fall: f32,
}

impl PeakMeter {
    /// Creates an empty meter.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        // Sample rates are far inside f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let rate = sample_rate.as_hz() as f32;
        Self {
            level: 0.0,
            fall: 10.0_f32.powf(-FALL_DB_PER_SECOND / 20.0 / rate),
        }
    }

    /// Takes in a played block; returns the current peak level (linear).
    pub fn process(&mut self, frames: &[Frame]) -> f32 {
        let peak = frames
            .iter()
            .map(|f| f.left.abs().max(f.right.abs()))
            .fold(0.0, f32::max);
        // Block sizes are small, far inside i32 range.
        let len = i32::try_from(frames.len()).unwrap_or(i32::MAX);
        self.level = peak.max(self.level * self.fall.powi(len));
        self.level
    }
}

/// A linear peak level as a meter fill in `0.0..=1.0`, on a decibel scale
/// from -60 dB (empty) to 0 dB (full).
#[must_use]
pub fn meter_fill(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }
    ((20.0 * level.log10() - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_the_peak_of_a_block() {
        let mut meter = PeakMeter::new(SampleRate::HZ_48000);

        let level = meter.process(&[Frame::new(0.2, -0.5), Frame::new(0.1, 0.1)]);

        assert!((level - 0.5).abs() < 1e-6);
    }

    #[test]
    fn falls_about_30_db_per_second_after_the_peak() {
        let mut meter = PeakMeter::new(SampleRate::HZ_48000);
        meter.process(&[Frame::new(1.0, 1.0)]);

        let mut level = 0.0;
        for _ in 0..100 {
            level = meter.process(&[Frame::SILENCE; 480]);
        }

        let db = 20.0 * level.log10();
        assert!((db + 30.0).abs() < 0.5, "{db} dB after a second");
    }

    #[test]
    fn fill_maps_decibels_onto_the_meter() {
        assert!((meter_fill(1.0) - 1.0).abs() < 1e-6);
        assert!((meter_fill(0.001) - 0.0).abs() < 1e-6, "-60 dB is empty");
        assert!((meter_fill(0.031_623) - 0.5).abs() < 1e-3, "-30 dB is half");
        assert!(meter_fill(0.0).abs() < f32::EPSILON);
        assert!((meter_fill(2.0) - 1.0).abs() < f32::EPSILON);
    }
}
