use crate::{
    BassCut, BassShaper, ChannelMode, Delay, DelayLine, Frame, Limiter, Reverb, SampleRate, Space,
    StereoWidener, VirtualBass, Volume,
};

/// The per-device processing chain: delay, space (stereo width, reverb),
/// channel mapping, bass shaping (bass cut and virtual bass), gain (device
/// volume times the shared master gain), then a limiter. Space comes before
/// the channel mapping, so each speaker of a stereo pair gets its own side
/// of the widened image and its own reverb. Nothing in it adds latency beyond the delay, so
/// sound settings never move a device out of sync.
///
/// Gain changes ramp linearly across one processed block, so volume moves
/// never click. Never allocates after construction; safe on a real-time
/// render thread.
#[derive(Debug, Clone)]
pub struct OutputChain {
    delay_line: DelayLine,
    channel_mode: ChannelMode,
    widener: StereoWidener,
    reverb: Reverb,
    bass: BassShaper,
    limiter: Limiter,
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
            widener: StereoWidener::default(),
            reverb: Reverb::new(sample_rate),
            bass: BassShaper::new(sample_rate),
            limiter: Limiter::new(sample_rate),
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

    /// Sets the stereo width and reverb.
    pub fn set_space(&mut self, space: Space) {
        self.widener.set_width(space.width);
        self.reverb.set_amount(space.reverb);
        self.reverb.set_room_size(space.room_size);
    }

    /// Sets the high-pass cutoff for small speakers.
    pub fn set_bass_cut(&mut self, cut: BassCut) {
        self.bass.set_bass_cut(cut);
    }

    /// Sets how much virtual bass is added.
    pub fn set_virtual_bass(&mut self, amount: VirtualBass) {
        self.bass.set_virtual_bass(amount);
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
            let spacious = self
                .reverb
                .process_frame(self.widener.process_frame(delayed));
            let shaped = self.bass.process_frame(self.channel_mode.apply(spacious));
            *frame = self.limiter.process_frame(shaped.scaled(gain));
        }
        if !frames.is_empty() {
            self.current_gain = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amplitude: f32, frames: usize) -> Vec<Frame> {
        (0..frames)
            .map(|n| {
                #[allow(clippy::cast_precision_loss)]
                let x = amplitude * (std::f32::consts::TAU * hz * n as f32 / 48_000.0).sin();
                Frame::new(x, x)
            })
            .collect()
    }

    #[test]
    fn default_chain_passes_audio_unchanged() {
        let mut chain = OutputChain::new(SampleRate::HZ_48000);
        let input = sine(440.0, 0.8, 480);
        let mut frames = input.clone();

        chain.process(&mut frames);

        assert_eq!(frames, input);
    }

    #[test]
    fn each_speaker_of_a_pair_gets_its_side_of_the_widened_image() {
        let space = Space {
            width: crate::StereoWidth::from_percent(200).unwrap(),
            ..Space::default()
        };
        let mut left = OutputChain::new(SampleRate::HZ_48000);
        left.set_space(space);
        left.set_channel_mode(ChannelMode::LeftOnly);
        let mut right = left.clone();
        right.set_channel_mode(ChannelMode::RightOnly);
        let mut l = [Frame::new(0.6, 0.2)];
        let mut r = l;

        left.process(&mut l);
        right.process(&mut r);

        assert!((l[0].left - 0.8).abs() < 1e-6, "{l:?}");
        assert!(r[0].left.abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn loud_virtual_bass_is_limited_to_full_scale() {
        let mut chain = OutputChain::new(SampleRate::HZ_48000);
        chain.set_bass_cut(BassCut::from_hz(150).unwrap());
        chain.set_virtual_bass(VirtualBass::from_percent(100).unwrap());
        let mut frames = sine(60.0, 1.0, 48_000);

        chain.process(&mut frames);

        let peak = frames.iter().map(|f| f.left.abs()).fold(0.0, f32::max);
        assert!(peak <= 1.0 + 1e-6, "{peak}");
        assert!(peak > 0.5, "the bass is still heard: {peak}");
    }
}
