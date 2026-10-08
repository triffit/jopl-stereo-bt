use crate::{Delay, Frame, SampleRate};

/// A fixed-capacity frame delay line sized for [`Delay::MAX`].
///
/// All memory is allocated in [`DelayLine::new`]; changing the delay and
/// processing never allocate, so this is safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buffer: Box<[Frame]>,
    write_index: usize,
    delay_frames: usize,
    sample_rate: SampleRate,
}

impl DelayLine {
    /// Creates a silent delay line with zero delay.
    #[must_use]
    pub fn new(sample_rate: SampleRate) -> Self {
        let capacity = Delay::MAX.frames_at(sample_rate) + 1;
        Self {
            buffer: vec![Frame::SILENCE; capacity].into_boxed_slice(),
            write_index: 0,
            delay_frames: 0,
            sample_rate,
        }
    }

    /// Changes the delay. Takes effect from the next processed frame.
    pub fn set_delay(&mut self, delay: Delay) {
        self.delay_frames = delay.frames_at(self.sample_rate);
    }

    /// Pushes one frame in and returns the frame delayed by the current delay.
    pub fn process_frame(&mut self, input: Frame) -> Frame {
        let capacity = self.buffer.len();
        self.buffer[self.write_index] = input;
        let read_index = (self.write_index + capacity - self.delay_frames) % capacity;
        let output = self.buffer[read_index];
        self.write_index = (self.write_index + 1) % capacity;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_delay_reads_correct_frame_after_buffer_wraps() {
        let rate = SampleRate::from_hz(8_000).unwrap();
        let mut line = DelayLine::new(rate);
        line.set_delay(Delay::MAX);
        let delay_frames = Delay::MAX.frames_at(rate);
        let total = delay_frames * 3;

        let mut last_output = Frame::SILENCE;
        for i in 0..total {
            let marker = f32::from(u16::try_from(i % 4_000).unwrap());
            last_output = line.process_frame(Frame::new(marker, 0.0));
        }

        let expected_index = (total - 1 - delay_frames) % 4_000;
        let expected = f32::from(u16::try_from(expected_index).unwrap());
        assert_eq!(last_output, Frame::new(expected, 0.0));
    }
}
