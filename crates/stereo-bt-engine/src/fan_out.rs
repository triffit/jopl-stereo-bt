use std::sync::Arc;

use rtrb::Producer;
use stereo_bt_core::Frame;

use crate::output_controls::OutputControls;

/// Copies every captured block into each output's queue (PRODUCT.md F1).
///
/// Lock- and allocation-free. A full queue drops the newest frames for that
/// output only and counts an overrun; a queue whose render thread has exited
/// is skipped, so one failed device never affects the others.
pub(crate) struct FanOut {
    outputs: Vec<(Producer<Frame>, Arc<OutputControls>)>,
}

impl FanOut {
    pub(crate) fn new(outputs: Vec<(Producer<Frame>, Arc<OutputControls>)>) -> Self {
        Self { outputs }
    }

    pub(crate) fn push(&mut self, frames: &[Frame]) {
        for (producer, controls) in &mut self.outputs {
            if producer.is_abandoned() {
                continue;
            }
            let (_pushed, dropped) = producer.push_partial_slice(frames);
            if !dropped.is_empty() {
                controls.add_overrun();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputConfig;
    use rtrb::RingBuffer;
    use stereo_bt_core::DeviceId;

    fn controls() -> Arc<OutputControls> {
        Arc::new(OutputControls::new(&OutputConfig::new(
            DeviceId::new("dev").unwrap(),
        )))
    }

    fn frames(n: u8) -> Vec<Frame> {
        (1..=n).map(|i| Frame::new(f32::from(i), 0.0)).collect()
    }

    #[test]
    fn f1_every_output_receives_the_same_frames() {
        let (pa, mut ca) = RingBuffer::new(16);
        let (pb, mut cb) = RingBuffer::new(16);
        let mut fan_out = FanOut::new(vec![(pa, controls()), (pb, controls())]);

        fan_out.push(&frames(4));

        let mut a = [Frame::SILENCE; 4];
        let mut b = [Frame::SILENCE; 4];
        ca.pop_entire_slice(&mut a).unwrap();
        cb.pop_entire_slice(&mut b).unwrap();
        assert_eq!(a.to_vec(), frames(4));
        assert_eq!(b.to_vec(), frames(4));
    }

    #[test]
    fn full_output_counts_overrun_without_affecting_others() {
        let (small, _small_consumer) = RingBuffer::new(2);
        let (large, large_consumer) = RingBuffer::new(16);
        let small_controls = controls();
        let large_controls = controls();
        let mut fan_out = FanOut::new(vec![
            (small, Arc::clone(&small_controls)),
            (large, Arc::clone(&large_controls)),
        ]);

        fan_out.push(&frames(4));

        assert_eq!(small_controls.overruns(), 1);
        assert_eq!(large_controls.overruns(), 0);
        assert_eq!(large_consumer.slots(), 4);
    }

    #[test]
    fn output_whose_render_thread_exited_is_skipped() {
        let (producer, consumer) = RingBuffer::new(2);
        let c = controls();
        let mut fan_out = FanOut::new(vec![(producer, Arc::clone(&c))]);
        drop(consumer);

        fan_out.push(&frames(4));

        assert_eq!(c.overruns(), 0);
    }
}
