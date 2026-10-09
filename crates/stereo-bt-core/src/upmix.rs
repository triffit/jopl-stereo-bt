use crate::biquad::{BUTTERWORTH_Q2, Biquad};
use crate::{Frame, SampleRate};

/// Extra delay of rear speakers: the ear then still places sound at the
/// front (precedence effect) while the ambience comes from behind.
const REAR_DELAY_SECONDS: f32 = 0.015;
/// Rear speakers get no treble above this, so they add ambience rather
/// than detail that pulls sound backwards.
const REAR_TOP_HZ: f64 = 7_000.0;
/// The ambience (half the left/right difference) is quiet; lift it so the
/// device volume slider has a useful range.
const REAR_GAIN: f32 = 1.4;

/// Where a speaker stands in a surround setup made from stereo (F20),
/// in the spirit of passive Dolby Pro Logic decoding.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum SpeakerPosition {
    /// Plays the stereo signal as usual.
    #[default]
    Front,
    /// Plays the middle of the stereo image (voices) in mono.
    Center,
    /// Plays only the ambience (left minus right), delayed and without
    /// treble; the right rear channel is in opposite phase to the left.
    Rear,
}

/// Turns stereo into the signal for one [`SpeakerPosition`].
///
/// Front is a bit-exact pass-through. The rear delay buffer is allocated
/// once at construction; processing never allocates.
#[derive(Debug, Clone)]
pub struct Upmix {
    position: SpeakerPosition,
    delay: Vec<f32>,
    index: usize,
    top: Biquad,
}

impl Upmix {
    /// Creates an upmix for a front speaker.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        // Sample rates are far inside f32's exact integer range.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let frames = (REAR_DELAY_SECONDS * sample_rate.as_hz() as f32).round() as usize;
        Self {
            position: SpeakerPosition::Front,
            delay: vec![0.0; frames.max(1)],
            index: 0,
            top: Biquad::low_pass(sample_rate, REAR_TOP_HZ, BUTTERWORTH_Q2),
        }
    }

    /// Sets where the speaker stands.
    pub fn set_position(&mut self, position: SpeakerPosition) {
        if position != self.position && position == SpeakerPosition::Rear {
            // Start without ambience left over from an earlier rear spell.
            self.delay.fill(0.0);
        }
        self.position = position;
    }

    /// Processes one frame.
    pub fn process_frame(&mut self, frame: Frame) -> Frame {
        match self.position {
            SpeakerPosition::Front => frame,
            SpeakerPosition::Center => {
                let mid = f32::midpoint(frame.left, frame.right);
                Frame::new(mid, mid)
            }
            SpeakerPosition::Rear => {
                let side = self.top.process((frame.left - frame.right) / 2.0);
                let delayed = self.delay[self.index];
                self.delay[self.index] = side;
                self.index = (self.index + 1) % self.delay.len();
                let ambience = delayed * REAR_GAIN;
                Frame::new(ambience, -ambience)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upmix(position: SpeakerPosition) -> Upmix {
        let mut u = Upmix::new(SampleRate::HZ_48000);
        u.set_position(position);
        u
    }

    #[test]
    fn front_is_a_bit_exact_pass_through() {
        let mut u = upmix(SpeakerPosition::Front);
        let frame = Frame::new(0.3, -0.6);

        assert_eq!(u.process_frame(frame), frame);
    }

    #[test]
    fn center_plays_the_middle_in_mono() {
        let mut u = upmix(SpeakerPosition::Center);

        let out = u.process_frame(Frame::new(0.6, 0.2));

        assert!((out.left - 0.4).abs() < 1e-6 && (out.right - 0.4).abs() < 1e-6);
    }

    #[test]
    fn rear_is_silent_for_sound_in_the_middle() {
        let mut u = upmix(SpeakerPosition::Rear);

        let peak = (0..4_800)
            .map(|n| {
                #[allow(clippy::cast_precision_loss)]
                let x = (n as f32 * 0.05).sin() * 0.8;
                u.process_frame(Frame::new(x, x)).left.abs()
            })
            .fold(0.0, f32::max);

        assert!(peak < 1e-6, "{peak}");
    }

    #[test]
    fn rear_plays_the_ambience_15_ms_later_in_opposite_phase() {
        let mut u = upmix(SpeakerPosition::Rear);
        let out: Vec<Frame> = (0..2_000)
            .map(|n| {
                let x = if n < 400 { 0.5 } else { 0.0 };
                u.process_frame(Frame::new(x, -x))
            })
            .collect();

        let first = out.iter().position(|f| f.left.abs() > 1e-3);
        assert_eq!(first, Some(720), "15 ms at 48 kHz");
        let f = out[900];
        assert!(f.left > 0.5 && (f.left + f.right).abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn position_round_trips_through_serde() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            position: SpeakerPosition,
        }
        let text = toml::to_string(&Wrapper {
            position: SpeakerPosition::Rear,
        })
        .unwrap();

        assert_eq!(text.trim(), "position = \"rear\"");
        let back: Wrapper = toml::from_str(&text).unwrap();
        assert_eq!(back.position, SpeakerPosition::Rear);
    }
}
