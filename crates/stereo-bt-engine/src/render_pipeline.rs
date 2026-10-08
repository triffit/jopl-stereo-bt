use std::sync::Arc;

use rtrb::Consumer;
use stereo_bt_core::{Frame, OutputChain, Playout, SampleRate};

use crate::output_controls::OutputControls;

/// Everything one render cycle does between the output queue and the device
/// buffer: playout policy, then delay, channel mapping and volume.
///
/// Allocation- and lock-free; runs on the render thread.
pub(crate) struct RenderPipeline {
    queue: Consumer<Frame>,
    playout: Playout,
    chain: OutputChain,
    controls: Arc<OutputControls>,
}

impl RenderPipeline {
    pub(crate) fn new(
        queue: Consumer<Frame>,
        playout: Playout,
        sample_rate: SampleRate,
        controls: Arc<OutputControls>,
    ) -> Self {
        Self {
            queue,
            playout,
            chain: OutputChain::new(sample_rate),
            controls,
        }
    }

    /// Fills `out` completely with the next frames this device must play.
    pub(crate) fn fill(&mut self, out: &mut [Frame]) {
        self.apply_controls();

        let plan = self.playout.plan(self.queue.slots(), out.len());
        if plan.skip > 0
            && let Ok(chunk) = self.queue.read_chunk(plan.skip)
        {
            chunk.commit_all();
        }
        let (popped, _) = self.queue.pop_partial_slice(&mut out[..plan.read]);
        let read = popped.len();
        out[read..].fill(Frame::SILENCE);

        self.chain.process(out);
        self.controls
            .record_playout(self.playout.underruns(), self.playout.trims());
    }

    fn apply_controls(&mut self) {
        self.chain.set_delay(self.controls.delay());
        self.chain.set_volume(self.controls.volume());
        self.chain.set_channel_mode(self.controls.channel_mode());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputConfig;
    use rtrb::{Producer, RingBuffer};
    use stereo_bt_core::{ChannelMode, Delay, DeviceId};

    const TARGET: usize = 4;

    fn pipeline() -> (Producer<Frame>, RenderPipeline, Arc<OutputControls>) {
        let (producer, consumer) = RingBuffer::new(64);
        let controls = Arc::new(OutputControls::new(&OutputConfig::new(
            DeviceId::new("dev").unwrap(),
        )));
        let pipeline = RenderPipeline::new(
            consumer,
            Playout::new(TARGET, 64),
            SampleRate::HZ_48000,
            Arc::clone(&controls),
        );
        (producer, pipeline, controls)
    }

    fn ramp(from: u8, to: u8) -> Vec<Frame> {
        (from..=to)
            .map(|i| Frame::new(f32::from(i), -f32::from(i)))
            .collect()
    }

    #[test]
    fn plays_silence_while_priming() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 3)).unwrap();
        let mut out = [Frame::new(9.0, 9.0); 2];

        pipeline.fill(&mut out);

        assert_eq!(out, [Frame::SILENCE; 2]);
    }

    #[test]
    fn plays_queued_frames_in_order_once_primed() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        let mut out = [Frame::SILENCE; 3];

        pipeline.fill(&mut out);
        assert_eq!(out.to_vec(), ramp(1, 3));
        producer.push_entire_slice(&ramp(5, 7)).unwrap();
        pipeline.fill(&mut out);
        assert_eq!(out.to_vec(), ramp(4, 6));
    }

    #[test]
    fn priming_discards_surplus_so_latency_equals_the_cushion() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 10)).unwrap();
        let mut out = [Frame::SILENCE; 2];

        pipeline.fill(&mut out);

        assert_eq!(out.to_vec(), ramp(7, 8));
    }

    #[test]
    fn underrun_pads_with_silence_and_is_counted() {
        let (mut producer, mut pipeline, controls) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        let mut out = [Frame::SILENCE; 3];
        pipeline.fill(&mut out);

        pipeline.fill(&mut out);

        assert_eq!(out, [Frame::new(4.0, -4.0), Frame::SILENCE, Frame::SILENCE]);
        assert_eq!(controls.underruns(), 1);
    }

    #[test]
    fn f2_channel_mode_change_applies_on_next_cycle() {
        let (mut producer, mut pipeline, controls) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        controls.set_channel_mode(ChannelMode::LeftOnly);
        let mut out = [Frame::SILENCE; 1];

        pipeline.fill(&mut out);

        assert_eq!(out, [Frame::new(1.0, 1.0)]);
    }

    #[test]
    fn f3_delay_shifts_playback_by_delay_frames() {
        let (mut producer, mut pipeline, controls) = pipeline();
        controls.set_delay(Delay::from_millis(1).unwrap()); // 48 frames
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        let mut out = vec![Frame::SILENCE; 50];

        pipeline.fill(&mut out);

        assert!(out[..48].iter().all(|f| *f == Frame::SILENCE));
        assert_eq!(out[48], Frame::new(1.0, -1.0));
    }
}
