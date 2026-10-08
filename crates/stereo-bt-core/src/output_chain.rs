use crate::{ChannelMode, Delay, DelayLine, Frame, SampleRate, Volume};

/// The per-device processing chain: delay, channel mapping, then volume.
///
/// Never allocates after construction; safe on a real-time render thread.
#[derive(Debug, Clone)]
pub struct OutputChain {
    delay_line: DelayLine,
    channel_mode: ChannelMode,
    gain: f32,
}

impl OutputChain {
    /// Creates a chain with zero delay, stereo channel mode and full volume.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        Self {
            delay_line: DelayLine::new(sample_rate),
            channel_mode: ChannelMode::default(),
            gain: Volume::default().gain(),
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
        self.gain = volume.gain();
    }

    /// Processes `frames` in place.
    pub fn process(&mut self, frames: &mut [Frame]) {
        for frame in frames {
            let delayed = self.delay_line.process_frame(*frame);
            *frame = self.channel_mode.apply(delayed).scaled(self.gain);
        }
    }
}
