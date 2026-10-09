use std::time::{Duration, Instant};

use stereo_bt_core::find_arrival;

/// Microphone samples per second (the stream rate).
const RATE: f64 = 48_000.0;

/// A continuous microphone recording dated on the same clock as the test
/// chirps, so each chirp's emission time maps to a position in it.
///
/// Every packet's arrival implies where the recording started. Scheduling
/// jitter only ever makes a packet look later, never earlier, so the earliest
/// implied start is the most accurate one.
#[derive(Debug, Default)]
pub(crate) struct MicTimeline {
    samples: Vec<f32>,
    anchor: Option<Instant>,
    /// Implied recording start per packet, in microseconds after `anchor`.
    origins_us: Vec<i64>,
}

impl MicTimeline {
    /// Appends a packet whose first sample was recorded at `first_at`.
    pub(crate) fn push(&mut self, first_at: Instant, packet: &[f32]) {
        if packet.is_empty() {
            return;
        }
        let anchor = *self.anchor.get_or_insert(first_at);
        let elapsed_us = signed_us(first_at, anchor);
        let recorded_us = to_us(self.samples.len());
        self.origins_us.push(elapsed_us - recorded_us);
        self.samples.extend_from_slice(packet);
    }

    /// The recording position at instant `at`, if it lies after the start.
    pub(crate) fn index_at(&self, at: Instant) -> Option<usize> {
        let anchor = self.anchor?;
        let origin_us = *self.origins_us.iter().min()?;
        let offset_us = signed_us(at, anchor) - origin_us;
        let index = f64::from(i32::try_from(offset_us).ok()?) * RATE / 1_000_000.0;
        // Non-negative and far below usize::MAX after the check.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (index >= 0.0).then_some(index.round() as usize)
    }

    /// Latency in milliseconds from each emission until `template` is heard,
    /// searching `window` after it; emissions not clearly heard are skipped.
    pub(crate) fn latencies_ms(
        &self,
        emissions: &[Instant],
        template: &[f32],
        window: Duration,
    ) -> Vec<f64> {
        // A window of under a second at 48 kHz is far below u32::MAX frames.
        let window_frames = usize::try_from(window.as_millis() * 48).unwrap_or(usize::MAX);
        emissions
            .iter()
            .filter_map(|&emitted| {
                let start = self.index_at(emitted)?;
                let arrival = find_arrival(&self.samples, template, start, window_frames)?;
                let frames = u32::try_from(arrival.offset).ok()?;
                Some(f64::from(frames) * 1_000.0 / RATE)
            })
            .collect()
    }
}

/// The typical value of `latencies`, ignoring strays; `None` with fewer than
/// three consistent measurements.
pub(crate) fn robust_latency(latencies: &[f64]) -> Option<f64> {
    /// Measurements further than this from the median are strays.
    const TOLERANCE_MS: f64 = 10.0;
    let center = median(latencies)?;
    let consistent: Vec<f64> = latencies
        .iter()
        .copied()
        .filter(|l| (l - center).abs() <= TOLERANCE_MS)
        .collect();
    (consistent.len() >= 3)
        .then(|| median(&consistent))
        .flatten()
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(sorted[sorted.len() / 2])
}

fn to_us(frames: usize) -> i64 {
    // Recordings last minutes at most, far inside i64 microseconds.
    i64::try_from(frames).unwrap_or(i64::MAX / 2) * 1_000_000 / 48_000
}

fn signed_us(at: Instant, reference: Instant) -> i64 {
    let micros = |d: Duration| i64::try_from(d.as_micros()).unwrap_or(i64::MAX / 2);
    match at.checked_duration_since(reference) {
        Some(after) => micros(after),
        None => -micros(reference.duration_since(at)),
    }
}

#[cfg(test)]
mod tests {
    use stereo_bt_core::{ClickGenerator, SampleRate};

    use super::*;

    fn template() -> Vec<f32> {
        ClickGenerator::chirp(SampleRate::HZ_48000, 700).template()
    }

    /// Records `seconds` of silence with chirps at `chirp_frames`, delivered in
    /// 480-frame packets dated with `jitter_us` of scheduling noise.
    fn record(seconds: usize, chirp_frames: &[usize], jitter_us: &[u64]) -> (Instant, MicTimeline) {
        let start = Instant::now();
        let template = template();
        let mut audio = vec![0.0_f32; seconds * 48_000];
        for &at in chirp_frames {
            for (k, s) in template.iter().enumerate() {
                audio[at + k] += s * 0.1;
            }
        }
        let mut timeline = MicTimeline::default();
        for (n, packet) in audio.chunks(480).enumerate() {
            let jitter = Duration::from_micros(jitter_us[n % jitter_us.len()]);
            timeline.push(
                start + Duration::from_millis(10 * n as u64) + jitter,
                packet,
            );
        }
        (start, timeline)
    }

    #[test]
    fn instants_map_to_recording_positions_despite_jitter() {
        let (start, timeline) = record(1, &[], &[0, 3_000, 500, 8_000, 0, 200]);

        assert_eq!(
            timeline.index_at(start + Duration::from_millis(500)),
            Some(24_000)
        );
    }

    #[test]
    fn latency_is_measured_from_emission_to_arrival() {
        // Chirps emitted at 0.5 s and 1.2 s arrive 85 ms later.
        let (start, timeline) = record(2, &[24_000 + 4_080, 57_600 + 4_080], &[0]);
        let emissions = [
            start + Duration::from_millis(500),
            start + Duration::from_millis(1_200),
        ];

        let latencies = timeline.latencies_ms(&emissions, &template(), Duration::from_millis(600));

        assert_eq!(latencies.len(), 2);
        for latency in latencies {
            assert!((latency - 85.0).abs() < 0.1, "{latency}");
        }
    }

    #[test]
    fn silence_after_an_emission_gives_no_latency() {
        let (start, timeline) = record(1, &[], &[0]);

        let latencies = timeline.latencies_ms(
            &[start + Duration::from_millis(100)],
            &template(),
            Duration::from_millis(600),
        );

        assert!(latencies.is_empty());
    }

    #[test]
    fn typical_latency_ignores_strays_and_needs_three_values() {
        assert_eq!(robust_latency(&[85.0, 84.0, 450.0, 86.0, 85.5]), Some(85.5));
        assert_eq!(robust_latency(&[85.0, 450.0]), None);
    }
}
