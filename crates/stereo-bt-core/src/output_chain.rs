use crate::{ChannelMode, Delay, DelayLine, Frame, SampleRate, Volume};

/// The per-device processing chain: delay, channel mapping, then gain
/// (device volume times the shared master gain).
///
/// Gain changes ramp linearly across one processed block, so volume moves
/// never click. Never allocates after construction; safe on a real-time
/// render thread.
#[derive(Debug, Clone)]
pub struct OutputChain {
    delay_line: DelayLine,
    channel_mode: ChannelMode,
    volume_gain: f32,
    master_gain: f32,
    /// The gain applied at the end of the previous block.
    current_gain: f32,
}

impl OutputChain {
    /// Creates a chain with zero delay, stereo channel mode and full volume.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        Self {
            delay_line: DelayLine::new(sample_rate),
            channel_mode: ChannelMode::default(),
            volume_gain: Volume::default().gain(),
            master_gain: 1.0,
            current_gain: Volume::default().gain(),
        }
    }

    /// Sets the sync delay.
    pub fn set_delay(&mut self, delay: Delay) {
        self.delay_line.set_delay(delay);
    }

    /// Sets which part of the stereo signal this device plays.
    pub fn set_channel_mode(&mut self, mode: ChannelMode) {
        self.channel_mode = mode;
    }

    /// Sets the device volume.
    pub fn set_volume(&mut self, volume: Volume) {
        self.volume_gain = volume.gain();
    }

    /// Sets the gain shared by all devices, e.g. from the system volume;
    /// clamped to `0.0..=1.0`.
    pub fn set_master_gain(&mut self, amplitude: f32) {
        self.master_gain = amplitude.clamp(0.0, 1.0);
    }

    /// Processes `frames` in place.
    pub fn process(&mut self, frames: &mut [Frame]) {
        let target = self.volume_gain * self.master_gain;
        let start = self.current_gain;
        // Block sizes are small, far inside f32's exact integer range.
        #[allow(clippy::cast_precision_loss)]
        let steps = frames.len() as f32;
        for (index, frame) in frames.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let progress = (index + 1) as f32 / steps;
            let gain = start + (target - start) * progress;
            let delayed = self.delay_line.process_frame(*frame);
            *frame = self.channel_mode.apply(delayed).scaled(gain);
        }
        if !frames.is_empty() {
            self.current_gain = target;
        }
    }
}
