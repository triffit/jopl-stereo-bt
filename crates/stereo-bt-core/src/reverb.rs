use crate::{Frame, Percent, SampleRate};

/// Comb filter lengths of Freeverb (Jezar at Dreampoint, public domain) at
/// 44.1 kHz; scaled to the actual sample rate.
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
/// All-pass filter lengths at 44.1 kHz.
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
/// Extra length of the right channel's filters, so left and right reverb
/// differ: that difference is what makes the sound wider.
const STEREO_SPREAD: usize = 23;
const TUNING_RATE: usize = 44_100;
/// Input level into the comb bank (Freeverb's fixed gain).
const INPUT_GAIN: f32 = 0.015;
/// Comb feedback for room size 0 % and the extra at 100 %.
const ROOM_OFFSET: f32 = 0.7;
const ROOM_SCALE: f32 = 0.28;
/// High-frequency damping inside the combs (Freeverb's default).
const DAMPING: f32 = 0.5;
const ALLPASS_FEEDBACK: f32 = 0.5;
/// Wet level at 100 % reverb.
const MAX_WET: f32 = 1.5;

/// A feedback comb filter with a one-pole low-pass in the loop.
#[derive(Debug, Clone)]
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    filter_store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
            filter_store: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filter_store = output * (1.0 - DAMPING) + self.filter_store * DAMPING;
        self.buffer[self.index] = input + self.filter_store * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.filter_store = 0.0;
    }
}

/// A Schroeder all-pass filter.
#[derive(Debug, Clone)]
struct AllPass {
    buffer: Vec<f32>,
    index: usize,
}

impl AllPass {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        self.buffer[self.index] = input + buffered * ALLPASS_FEEDBACK;
        self.index = (self.index + 1) % self.buffer.len();
        buffered - input
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// One channel's reverb network: parallel combs, then serial all-passes.
#[derive(Debug, Clone)]
struct Tank {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl Tank {
    fn new(sample_rate: SampleRate, spread: usize) -> Self {
        let scale = |len: usize| {
            usize::try_from(sample_rate.as_hz()).unwrap_or(TUNING_RATE) * (len + spread)
                / TUNING_RATE
        };
        Self {
            combs: COMB_TUNING.iter().map(|&n| Comb::new(scale(n))).collect(),
            allpasses: ALLPASS_TUNING
                .iter()
                .map(|&n| AllPass::new(scale(n)))
                .collect(),
        }
    }

    fn process(&mut self, input: f32, feedback: f32) -> f32 {
        let mut out: f32 = self
            .combs
            .iter_mut()
            .map(|c| c.process(input, feedback))
            .sum();
        for allpass in &mut self.allpasses {
            out = allpass.process(out);
        }
        out
    }

    fn clear(&mut self) {
        self.combs.iter_mut().for_each(Comb::clear);
        self.allpasses.iter_mut().for_each(AllPass::clear);
    }
}

/// A stereo room reverb (Freeverb design) added to the dry signal, which
/// itself passes undelayed: no latency, so sync is unaffected.
///
/// Left and right use slightly different filter lengths, so each speaker
/// of a stereo pair gets its own reverb. Off is a bit-exact pass-through.
/// Buffers are allocated once, at construction; processing never allocates.
#[derive(Debug, Clone)]
pub struct Reverb {
    left: Tank,
    right: Tank,
    amount: Percent,
    feedback: f32,
}

impl Reverb {
    /// Creates a reverb that is off, with a medium room.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        let mut reverb = Self {
            left: Tank::new(sample_rate, 0),
            right: Tank::new(sample_rate, STEREO_SPREAD),
            amount: Percent::ZERO,
            feedback: 0.0,
        };
        reverb.set_room_size(Percent::new(50).unwrap_or(Percent::ZERO));
        reverb
    }

    /// Sets how much reverb is added; `0` is off.
    pub fn set_amount(&mut self, amount: Percent) {
        if self.amount == Percent::ZERO && amount != Percent::ZERO {
            // Start from silence, not from a tail left over from before.
            self.left.clear();
            self.right.clear();
        }
        self.amount = amount;
    }

    /// Sets how large the room sounds (the length of the tail).
    pub fn set_room_size(&mut self, size: Percent) {
        self.feedback = ROOM_OFFSET + ROOM_SCALE * size.fraction();
    }

    /// Processes one frame.
    pub fn process_frame(&mut self, frame: Frame) -> Frame {
        if self.amount == Percent::ZERO {
            return frame;
        }
        let input = (frame.left + frame.right) * INPUT_GAIN;
        let wet = self.amount.fraction() * MAX_WET;
        Frame::new(
            frame.left + self.left.process(input, self.feedback) * wet,
            frame.right + self.right.process(input, self.feedback) * wet,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reverb(amount: u8, room: u8) -> Reverb {
        let mut r = Reverb::new(SampleRate::HZ_48000);
        r.set_amount(Percent::new(amount).unwrap());
        r.set_room_size(Percent::new(room).unwrap());
        r
    }

    /// The reverb's added signal after an impulse, per channel.
    fn tail(r: &mut Reverb, frames: usize) -> Vec<Frame> {
        (0..frames)
            .map(|n| {
                let x = if n == 0 { 1.0 } else { 0.0 };
                let out = r.process_frame(Frame::new(x, x));
                Frame::new(out.left - x, out.right - x)
            })
            .collect()
    }

    fn energy(frames: &[Frame]) -> f32 {
        frames.iter().map(|f| f.left * f.left).sum()
    }

    #[test]
    fn off_is_a_bit_exact_pass_through() {
        let mut r = reverb(0, 80);
        for x in [0.0, 0.3, -1.0] {
            assert_eq!(r.process_frame(Frame::new(x, -x)), Frame::new(x, -x));
        }
    }

    #[test]
    fn the_dry_signal_is_not_delayed() {
        let mut r = reverb(50, 50);

        let first = r.process_frame(Frame::new(1.0, 1.0));

        assert_eq!(
            first,
            Frame::new(1.0, 1.0),
            "reverb starts after the dry sound"
        );
    }

    #[test]
    fn an_impulse_leaves_a_tail() {
        let mut r = reverb(50, 50);
        let out = tail(&mut r, 48_000);

        let late = energy(&out[9_600..19_200]);

        assert!(late > 1e-4, "tail between 200 and 400 ms: {late}");
    }

    #[test]
    fn a_larger_room_rings_longer() {
        let small = energy(&tail(&mut reverb(50, 10), 48_000)[24_000..]);
        let large = energy(&tail(&mut reverb(50, 90), 48_000)[24_000..]);

        assert!(large > 5.0 * small, "{small} vs {large}");
    }

    #[test]
    fn left_and_right_reverb_differ() {
        let mut r = reverb(50, 50);
        let out = tail(&mut r, 24_000);

        let (mut lr, mut ll, mut rr) = (0.0_f32, 0.0_f32, 0.0_f32);
        for f in &out {
            lr += f.left * f.right;
            ll += f.left * f.left;
            rr += f.right * f.right;
        }
        let correlation = lr / (ll * rr).sqrt();

        assert!(correlation.abs() < 0.5, "correlation {correlation}");
    }

    #[test]
    fn switching_on_again_starts_without_an_old_tail() {
        let mut r = reverb(50, 90);
        tail(&mut r, 100);
        r.set_amount(Percent::ZERO);
        r.set_amount(Percent::new(50).unwrap());

        let out = r.process_frame(Frame::SILENCE);

        assert_eq!(out, Frame::SILENCE);
    }
}
