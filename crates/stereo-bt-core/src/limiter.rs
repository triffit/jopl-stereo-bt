use crate::{Frame, SampleRate};

/// The output ceiling: full scale. Anything louder would clip in the
/// Windows mixer or the Bluetooth encoder.
const CEILING: f32 = 1.0;
/// Time for the gain to recover about 63 % of the way back to unity.
const RELEASE_SECONDS: f32 = 0.15;

/// A stereo-linked peak limiter without lookahead, so it adds no latency
/// and never moves a device out of sync.
///
/// The attack is instant: a peak above the ceiling is scaled to exactly the
/// ceiling, and the gain then recovers smoothly. Signals that never exceed
/// the ceiling pass bit-exact. Allocation-free.
#[derive(Debug, Clone)]
pub struct Limiter {
    /// `1 - gain`, kept as the state so its decay stays exact near unity.
    reduction: f32,
    /// Per-sample recovery factor towards unity gain.
    release: f32,
}

impl Limiter {
    /// Creates an idle limiter.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        // Sample rates are far inside f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let samples = RELEASE_SECONDS * sample_rate.as_hz() as f32;
        Self {
            reduction: 0.0,
            release: (-1.0 / samples).exp(),
        }
    }

    /// The gain applied to the last frame (`1.0` when not limiting).
    #[must_use]
    pub fn gain(&self) -> f32 {
        1.0 - self.reduction
    }

    /// Processes one frame.
    pub fn process_frame(&mut self, frame: Frame) -> Frame {
        if self.reduction > 0.0 {
            self.reduction *= self.release;
            // End the tail so quiet passages become bit-exact again.
            if self.reduction < 1e-5 {
                self.reduction = 0.0;
            }
        }
        let peak = frame.left.abs().max(frame.right.abs());
        if peak * (1.0 - self.reduction) > CEILING {
            self.reduction = 1.0 - CEILING / peak;
        }
        if self.reduction > 0.0 {
            frame.scaled(1.0 - self.reduction)
        } else {
            frame
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn limiter() -> Limiter {
        Limiter::new(SampleRate::HZ_48000)
    }

    #[test]
    fn signals_within_full_scale_pass_unchanged() {
        let mut l = limiter();
        for x in [0.0, 0.5, -1.0, 1.0, 0.999] {
            assert_eq!(l.process_frame(Frame::new(x, -x)), Frame::new(x, -x));
        }
    }

    #[test]
    fn a_peak_is_scaled_to_the_ceiling_on_both_channels() {
        let mut l = limiter();

        let out = l.process_frame(Frame::new(2.0, 0.5));

        assert!((out.left - 1.0).abs() < 1e-6);
        assert!((out.right - 0.25).abs() < 1e-6, "stereo-linked");
    }

    #[test]
    fn gain_recovers_after_the_peak() {
        let mut l = limiter();
        l.process_frame(Frame::new(2.0, 2.0));
        assert!((l.gain() - 0.5).abs() < 1e-6);

        for _ in 0..7_200 {
            l.process_frame(Frame::new(0.1, 0.1));
        }
        let after_150_ms = l.gain();
        // About 13 time constants later the gain is back at exactly unity.
        for _ in 0..96_000 {
            l.process_frame(Frame::new(0.1, 0.1));
        }

        assert!((0.7..0.9).contains(&after_150_ms), "{after_150_ms}");
        assert!((l.gain() - 1.0).abs() < f32::EPSILON);
    }

    proptest! {
        #[test]
        fn output_never_exceeds_full_scale(samples in prop::collection::vec((-8.0f32..8.0, -8.0f32..8.0), 1..500)) {
            let mut l = limiter();
            for (left, right) in samples {
                let out = l.process_frame(Frame::new(left, right));
                prop_assert!(out.left.abs() <= CEILING + 1e-6);
                prop_assert!(out.right.abs() <= CEILING + 1e-6);
            }
        }
    }
}
