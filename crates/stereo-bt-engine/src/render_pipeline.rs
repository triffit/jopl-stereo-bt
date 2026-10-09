use std::sync::Arc;

use rtrb::Consumer;
use stereo_bt_core::{DriftControl, DriftResampler, Frame, OutputChain, Playout, SampleRate};

use crate::output_controls::OutputControls;

/// Everything one render cycle does between the output queue and the device
/// buffer: playout policy, drift-correcting resampling, then delay, channel
/// mapping and volume.
///
/// Allocation- and lock-free after construction; runs on the render thread.
pub(crate) struct RenderPipeline {
    queue: Consumer<Frame>,
    playout: Playout,
    drift: DriftControl,
    resampler: DriftResampler,
    /// Scratch for the queue frames one cycle consumes.
    input: Vec<Frame>,
    chain: OutputChain,
    controls: Arc<OutputControls>,
}

impl RenderPipeline {
    /// `max_block` is the most frames one [`Self::fill`] call will produce.
    pub(crate) fn new(
        queue: Consumer<Frame>,
        playout: Playout,
        cushion: usize,
        max_block: usize,
        sample_rate: SampleRate,
        controls: Arc<OutputControls>,
    ) -> Self {
        // Room for the fastest ratio plus the resampler's fractional carry.
        let max_input = max_block + max_block / 100 + 2;
        Self {
            queue,
            playout,
            drift: DriftControl::new(cushion),
            resampler: DriftResampler::new(),
            input: vec![Frame::SILENCE; max_input],
            chain: OutputChain::new(sample_rate),
            controls,
        }
    }

    /// Fills `out` completely with the next frames this device must play.
    pub(crate) fn fill(&mut self, out: &mut [Frame]) {
        self.apply_controls();

        let available = self.queue.slots();
        // Measure drift only while audio flows; priming says nothing about clocks.
        if self.playout.is_primed() {
            self.drift.update(available);
        }
        let ratio = self.drift.ratio();
        let needed = self
            .resampler
            .input_needed(out.len(), ratio)
            .min(self.input.len());

        let plan = self.playout.plan(available, needed);
        if plan.skip > 0
            && let Ok(chunk) = self.queue.read_chunk(plan.skip)
        {
            chunk.commit_all();
        }
        let input = &mut self.input[..needed];
        let (popped, _) = self.queue.pop_partial_slice(&mut input[..plan.read]);
        let read = popped.len();
        input[read..].fill(Frame::SILENCE);

        self.resampler.process(input, ratio, out);
        self.chain.process(out);
        self.controls
            .record_playout(self.playout.underruns(), self.playout.trims());
    }

    fn apply_controls(&mut self) {
        self.chain.set_delay(self.controls.delay());
        self.chain.set_volume(self.controls.volume());
        self.chain.set_channel_mode(self.controls.channel_mode());
        self.chain.set_bass_cut(self.controls.bass_cut());
        self.chain.set_virtual_bass(self.controls.virtual_bass());
        self.chain.set_space(self.controls.space());
        self.chain.set_master_gain(self.controls.master_gain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputConfig;
    use rtrb::{Producer, RingBuffer};
    use stereo_bt_core::{ChannelMode, Delay, DeviceId};

    const TARGET: usize = 4;
    /// Constant latency of the drift resampler, in frames.
    const RESAMPLER_LATENCY: usize = 3;

    fn pipeline() -> (Producer<Frame>, RenderPipeline, Arc<OutputControls>) {
        let (producer, consumer) = RingBuffer::new(64);
        let controls = Arc::new(OutputControls::new(&OutputConfig::new(
            DeviceId::new("dev").unwrap(),
        )));
        let pipeline = RenderPipeline::new(
            consumer,
            Playout::new(TARGET, 64),
            TARGET,
            64,
            SampleRate::HZ_48000,
            Arc::clone(&controls),
        );
        (producer, pipeline, controls)
    }

    /// Numbered frames (`i / 16`), within full scale so the limiter
    /// leaves them alone.
    fn ramp(from: u8, to: u8) -> Vec<Frame> {
        (from..=to)
            .map(|i| Frame::new(f32::from(i) / 16.0, -f32::from(i) / 16.0))
            .collect()
    }

    /// Runs `cycles` fills of `block` frames and returns everything played.
    fn play(pipeline: &mut RenderPipeline, cycles: usize, block: usize) -> Vec<Frame> {
        let mut played = Vec::new();
        let mut out = vec![Frame::SILENCE; block];
        for _ in 0..cycles {
            pipeline.fill(&mut out);
            played.extend_from_slice(&out);
        }
        played
    }

    /// Frames clearly above silence; drops the resampler's tiny
    /// interpolation tail around signal edges.
    fn audible(frames: &[Frame]) -> Vec<Frame> {
        frames
            .iter()
            .copied()
            .filter(|f| f.left.abs() > 0.01 || f.right.abs() > 0.01)
            .collect()
    }

    /// The drift controller may nudge the ratio slightly, so compare within
    /// interpolation tolerance rather than bit-exactly.
    fn assert_close(actual: &[Frame], expected: &[Frame]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a.left - e.left).abs() < 1e-3 && (a.right - e.right).abs() < 1e-3,
                "{a:?} != {e:?}"
            );
        }
    }

    #[test]
    fn plays_silence_while_priming() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 3)).unwrap();

        assert!(audible(&play(&mut pipeline, 4, 2)).is_empty());
    }

    #[test]
    fn plays_queued_frames_in_order_once_primed() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        let mut played = play(&mut pipeline, 1, 3);
        producer.push_entire_slice(&ramp(5, 7)).unwrap();

        played.extend(play(&mut pipeline, 3, 3));

        assert_close(&audible(&played), &ramp(1, 7));
    }

    #[test]
    fn priming_discards_surplus_so_latency_equals_the_cushion() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 10)).unwrap();

        let played = play(&mut pipeline, 3, 4);

        assert_close(&audible(&played)[..1], &ramp(7, 7));
    }

    #[test]
    fn underrun_pads_with_silence_and_is_counted() {
        let (mut producer, mut pipeline, controls) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();

        let played = play(&mut pipeline, 4, 3);

        assert_close(&audible(&played), &ramp(1, 4));
        assert_eq!(controls.underruns(), 1);
    }

    #[test]
    fn f2_channel_mode_change_applies_on_next_cycle() {
        let (mut producer, mut pipeline, controls) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        controls.set_channel_mode(ChannelMode::LeftOnly);

        let played = play(&mut pipeline, 2, 4);

        assert_close(
            &audible(&played)[..1],
            &[Frame::new(1.0 / 16.0, 1.0 / 16.0)],
        );
    }

    #[test]
    fn f3_delay_shifts_playback_by_delay_frames() {
        let (mut producer, mut pipeline, controls) = pipeline();
        controls.set_delay(Delay::from_millis(1).unwrap()); // 48 frames
        producer.push_entire_slice(&ramp(1, 4)).unwrap();

        let played = play(&mut pipeline, 1, 60);

        let first = played.iter().position(|f| *f != Frame::SILENCE);
        assert_eq!(first, Some(48 + RESAMPLER_LATENCY));
        assert_eq!(played[48 + RESAMPLER_LATENCY], ramp(1, 1)[0]);
    }

    #[test]
    fn f9_full_queue_is_drained_by_playing_slightly_faster() {
        let (mut producer, mut pipeline, _) = pipeline();
        producer.push_entire_slice(&ramp(1, 4)).unwrap();
        play(&mut pipeline, 1, 4);

        // Keep the queue persistently above its cushion.
        for _ in 0..500 {
            let free = producer.slots().min(5);
            let _ = producer.push_partial_slice(&vec![Frame::new(1.0, 1.0); free]);
            play(&mut pipeline, 1, 4);
        }

        assert!(pipeline.drift.ratio() > 1.0);
    }
}
