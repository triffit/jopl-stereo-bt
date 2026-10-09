use crate::{Frame, SampleRate};

/// Pitch of the ping, clearly above music bass and easy to locate.
const PING_HZ: f32 = 1_320.0;
const BEEP_SECONDS: f32 = 0.12;
const GAP_SECONDS: f32 = 0.08;
/// Short fades so the beeps do not click.
const FADE_SECONDS: f32 = 0.005;
const PING_LEVEL: f32 = 0.35;

/// A short double beep that identifies one speaker ("which one is this?").
/// Mixed on top of whatever plays. Allocation-free.
#[derive(Debug, Clone)]
pub struct Ping {
    rate: f32,
    /// Frames since the ping started; `None` while silent.
    position: Option<u32>,
}

impl Ping {
    /// Creates a silent ping.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        // Sample rates are far inside f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let rate = sample_rate.as_hz() as f32;
        Self {
            rate,
            position: None,
        }
    }

    /// Starts (or restarts) the double beep.
    pub fn start(&mut self) {
        self.position = Some(0);
    }

    /// Whether the ping is sounding.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.position.is_some()
    }

    /// Adds the next ping sample to `frame`.
    pub fn process_frame(&mut self, frame: Frame) -> Frame {
        let Some(position) = self.position else {
            return frame;
        };
        // Positions stay below a second of frames, exact in f32.
        #[allow(clippy::cast_precision_loss)]
        let t = position as f32 / self.rate;
        let period = BEEP_SECONDS + GAP_SECONDS;
        let total = 2.0 * BEEP_SECONDS + GAP_SECONDS;
        if t >= total {
            self.position = None;
            return frame;
        }
        self.position = Some(position + 1);
        let in_beep = t % period;
        if in_beep >= BEEP_SECONDS {
            return frame;
        }
        let fade = (in_beep / FADE_SECONDS)
            .min((BEEP_SECONDS - in_beep) / FADE_SECONDS)
            .min(1.0);
        let tone = (std::f32::consts::TAU * PING_HZ * t).sin() * PING_LEVEL * fade;
        Frame::new(frame.left + tone, frame.right + tone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ping: &mut Ping, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|_| ping.process_frame(Frame::SILENCE).left)
            .collect()
    }

    #[test]
    fn silent_until_started() {
        let mut ping = Ping::new(SampleRate::HZ_48000);

        assert!(run(&mut ping, 1_000).iter().all(|&x| x == 0.0));
        assert!(!ping.is_active());
    }

    #[test]
    fn plays_two_beeps_then_stops() {
        let mut ping = Ping::new(SampleRate::HZ_48000);
        ping.start();

        let out = run(&mut ping, 48_000);
        let loud = |range: std::ops::Range<usize>| out[range].iter().any(|x| x.abs() > 0.2);

        assert!(loud(0..5_760), "first beep (0-120 ms)");
        assert!(!loud(5_900..9_400), "gap (120-200 ms)");
        assert!(loud(9_600..15_360), "second beep (200-320 ms)");
        assert!(out[16_000..].iter().all(|&x| x == 0.0), "silent afterwards");
        assert!(!ping.is_active());
    }

    #[test]
    fn beeps_start_and_end_without_a_click() {
        let mut ping = Ping::new(SampleRate::HZ_48000);
        ping.start();

        let out = run(&mut ping, 20);

        assert!(
            out[0].abs() < 0.01 && out[1].abs() < 0.02,
            "{:?}",
            &out[..3]
        );
    }
}
