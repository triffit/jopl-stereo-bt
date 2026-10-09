use crate::biquad::{BUTTERWORTH_Q2, BUTTERWORTH_Q4, Biquad};
use crate::{BassCut, Frame, SampleRate, VirtualBass};

/// Split frequency for virtual bass while the bass cut is off.
const DEFAULT_SPLIT_HZ: u16 = 150;
/// Harmonics are kept up to this multiple of the split frequency.
const HARMONIC_SPAN: f64 = 4.0;
/// Harmonic gain at 100 % virtual bass. A full-wave rectified sine has its
/// second harmonic at about 0.42 of the input, so this makes it about 0.85.
const MAX_HARMONIC_GAIN: f32 = 2.0;

/// Tailors the bass to a small speaker, without added latency:
///
/// - **Bass cut**: a 24 dB/octave Butterworth high-pass keeps bass the
///   speaker cannot play from wasting its headroom and distorting.
/// - **Virtual bass**: harmonics (2×, 4×, …) of the bass below the split
///   frequency, which the speaker can play; the ear then hears the missing
///   fundamental.
///
/// Both off is a bit-exact pass-through. Allocation-free.
#[derive(Debug, Clone)]
pub struct BassShaper {
    sample_rate: SampleRate,
    cut: BassCut,
    virtual_bass: VirtualBass,
    /// Two high-pass stages per channel.
    high_pass: [[Biquad; 2]; 2],
    /// Isolates the bass of the mono mix (two stages).
    bass: [Biquad; 2],
    /// Removes DC and the fundamentals from the rectified bass.
    harmonics_floor: Biquad,
    /// Removes harmonics too high to sound like bass.
    harmonics_ceiling: Biquad,
}

impl BassShaper {
    /// Creates a shaper with bass cut and virtual bass off.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        let mut shaper = Self {
            sample_rate,
            cut: BassCut::OFF,
            virtual_bass: VirtualBass::OFF,
            high_pass: [[Biquad::high_pass(sample_rate, 100.0, BUTTERWORTH_Q2); 2]; 2],
            bass: [Biquad::low_pass(sample_rate, 100.0, BUTTERWORTH_Q2); 2],
            harmonics_floor: Biquad::high_pass(sample_rate, 100.0, BUTTERWORTH_Q2),
            harmonics_ceiling: Biquad::low_pass(sample_rate, 100.0, BUTTERWORTH_Q2),
        };
        shaper.retune();
        shaper
    }

    /// Sets the high-pass cutoff; the filters keep their state, so moving
    /// it does not click.
    pub fn set_bass_cut(&mut self, cut: BassCut) {
        if cut != self.cut {
            self.cut = cut;
            self.retune();
        }
    }

    /// Sets how much virtual bass is added.
    pub fn set_virtual_bass(&mut self, amount: VirtualBass) {
        self.virtual_bass = amount;
    }

    fn retune(&mut self) {
        let rate = self.sample_rate;
        let split = f64::from(self.cut.hz().unwrap_or(DEFAULT_SPLIT_HZ));
        for channel in &mut self.high_pass {
            for (stage, q) in channel.iter_mut().zip(BUTTERWORTH_Q4) {
                stage.retune(Biquad::high_pass(rate, split, q));
            }
        }
        for (stage, q) in self.bass.iter_mut().zip(BUTTERWORTH_Q4) {
            stage.retune(Biquad::low_pass(rate, split, q));
        }
        self.harmonics_floor
            .retune(Biquad::high_pass(rate, split, BUTTERWORTH_Q2));
        self.harmonics_ceiling.retune(Biquad::low_pass(
            rate,
            split * HARMONIC_SPAN,
            BUTTERWORTH_Q2,
        ));
    }

    /// Processes one frame.
    pub fn process_frame(&mut self, frame: Frame) -> Frame {
        let mut out = frame;
        if self.cut.hz().is_some() {
            let [left, right] = &mut self.high_pass;
            out.left = cascade(left, frame.left);
            out.right = cascade(right, frame.right);
        }
        if self.virtual_bass != VirtualBass::OFF {
            let mono = f32::midpoint(frame.left, frame.right);
            let bass = cascade(&mut self.bass, mono);
            // Full-wave rectification doubles every bass frequency.
            let harmonics = self
                .harmonics_ceiling
                .process(self.harmonics_floor.process(bass.abs()));
            let gain = f32::from(self.virtual_bass.as_percent()) / 100.0 * MAX_HARMONIC_GAIN;
            out.left += harmonics * gain;
            out.right += harmonics * gain;
        }
        out
    }
}

/// Runs `x` through both stages.
fn cascade([first, second]: &mut [Biquad; 2], x: f32) -> f32 {
    let once = first.process(x);
    second.process(once)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::tests::sine_gain;

    fn shaper(cut: u16, virtual_bass: u8) -> BassShaper {
        let mut s = BassShaper::new(SampleRate::HZ_48000);
        s.set_bass_cut(BassCut::from_hz(cut).unwrap());
        s.set_virtual_bass(VirtualBass::from_percent(virtual_bass).unwrap());
        s
    }

    fn gain(mut s: BassShaper, hz: f64) -> f32 {
        sine_gain(|x| s.process_frame(Frame::new(x, x)).left, hz)
    }

    #[test]
    fn everything_off_is_a_bit_exact_pass_through() {
        let mut s = shaper(0, 0);
        for x in [0.0, 0.25, -0.7, 1.0] {
            assert_eq!(s.process_frame(Frame::new(x, -x)), Frame::new(x, -x));
        }
    }

    #[test]
    fn bass_cut_removes_bass_and_keeps_the_rest() {
        assert!(gain(shaper(150, 0), 50.0) < 0.02);
        assert!((gain(shaper(150, 0), 1_000.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn bass_cut_follows_the_chosen_cutoff() {
        assert!(gain(shaper(60, 0), 100.0) > 0.8);
        assert!(gain(shaper(250, 0), 100.0) < 0.05);
    }

    #[test]
    fn virtual_bass_makes_cut_bass_audible_again() {
        let without = gain(shaper(150, 0), 60.0);
        let with = gain(shaper(150, 100), 60.0);

        assert!(with > 0.3, "with virtual bass: {with}");
        assert!(with > 10.0 * without);
    }

    #[test]
    fn virtual_bass_leaves_treble_alone() {
        let treble = gain(shaper(150, 100), 2_000.0);

        assert!((treble - 1.0).abs() < 0.02, "{treble}");
    }

    #[test]
    fn virtual_bass_scales_with_the_amount() {
        let half = gain(shaper(150, 50), 60.0);
        let full = gain(shaper(150, 100), 60.0);

        assert!((full / half - 2.0).abs() < 0.1, "{half} vs {full}");
    }
}
