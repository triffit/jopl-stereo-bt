use crate::SampleRate;

/// Q of the two stages of a 4th-order Butterworth filter.
pub(crate) const BUTTERWORTH_Q4: [f64; 2] = [0.541_196_1, 1.306_563];
/// Q of a 2nd-order Butterworth filter.
pub(crate) const BUTTERWORTH_Q2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Normalised biquad coefficients (`a0 = 1`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// A second-order IIR filter (RBJ audio-EQ cookbook) for one channel,
/// transposed direct form II with `f64` state, so low cutoffs stay
/// accurate. Allocation-free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Biquad {
    c: Coefficients,
    s1: f64,
    s2: f64,
}

impl Biquad {
    pub(crate) fn high_pass(sample_rate: SampleRate, hz: f64, q: f64) -> Self {
        Self::design(sample_rate, hz, q, true)
    }

    pub(crate) fn low_pass(sample_rate: SampleRate, hz: f64, q: f64) -> Self {
        Self::design(sample_rate, hz, q, false)
    }

    fn design(sample_rate: SampleRate, hz: f64, q: f64, high: bool) -> Self {
        let nyquist = f64::from(sample_rate.as_hz()) / 2.0;
        let w0 =
            std::f64::consts::TAU * hz.clamp(1.0, nyquist * 0.95) / f64::from(sample_rate.as_hz());
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high {
            (f64::midpoint(1.0, cos), -(1.0 + cos))
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos)
        };
        Self {
            c: Coefficients {
                b0: b0 / a0,
                b1: b1 / a0,
                b2: b0 / a0,
                a1: -2.0 * cos / a0,
                a2: (1.0 - alpha) / a0,
            },
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// Changes the response while keeping the filter state, so a moving
    /// cutoff does not click.
    pub(crate) fn retune(&mut self, other: Self) {
        self.c = other.c;
    }

    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let x = f64::from(x);
        let y = self.c.b0 * x + self.s1;
        self.s1 = self.c.b1 * x - self.c.a1 * y + self.s2;
        self.s2 = self.c.b2 * x - self.c.a2 * y;
        // Audio samples are far inside f32 range.
        #[allow(clippy::cast_possible_truncation)]
        let out = y as f32;
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Peak amplitude of a filtered sine after the filter settled.
    pub(crate) fn sine_gain(mut filter: impl FnMut(f32) -> f32, hz: f64) -> f32 {
        let rate = 48_000.0;
        let mut peak = 0.0_f32;
        for n in 0..48_000 {
            #[allow(clippy::cast_possible_truncation)]
            let x = (std::f64::consts::TAU * hz * f64::from(n) / rate).sin() as f32;
            let y = filter(x);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn high_pass_passes_treble_and_halves_power_at_the_cutoff() {
        let make = || Biquad::high_pass(SampleRate::HZ_48000, 150.0, BUTTERWORTH_Q2);
        let mut f = make();
        assert!((sine_gain(|x| f.process(x), 2_000.0) - 1.0).abs() < 0.01);
        let mut f = make();
        let at_cutoff = sine_gain(|x| f.process(x), 150.0);
        assert!(
            (at_cutoff - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02,
            "{at_cutoff}"
        );
        let mut f = make();
        assert!(sine_gain(|x| f.process(x), 40.0) < 0.1);
    }

    #[test]
    fn low_pass_passes_bass_and_removes_treble() {
        let make = || Biquad::low_pass(SampleRate::HZ_48000, 150.0, BUTTERWORTH_Q2);
        let mut f = make();
        assert!((sine_gain(|x| f.process(x), 40.0) - 1.0).abs() < 0.02);
        let mut f = make();
        assert!(sine_gain(|x| f.process(x), 2_000.0) < 0.01);
    }
}
