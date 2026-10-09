//! Automatic latency measurement with a microphone: locating test chirps
//! in a recording, and planning the default device and delays.

use crate::{Delay, DeviceId};

/// Peaks weaker than this, relative to the median correlation, are noise:
/// pure noise reaches about 7, a clearly heard chirp 20 or more.
const MIN_QUALITY: f32 = 12.0;
/// Latency differences this small are inaudible.
const TOLERANCE_MS: f64 = 5.0;

/// Where a chirp was found in a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrival {
    /// Frames from the start of the search window to the chirp's start.
    pub offset: usize,
    /// Correlation peak relative to the median correlation (higher is clearer).
    pub quality: f32,
}

/// Finds `template` in `recording[start..start + len]` by cross-correlation.
/// Returns `None` when no clear peak stands out from the noise.
#[must_use]
pub fn find_arrival(
    recording: &[f32],
    template: &[f32],
    start: usize,
    len: usize,
) -> Option<Arrival> {
    let last = recording.len().checked_sub(template.len())?;
    let end = start.saturating_add(len).min(last + 1);
    if start >= end {
        return None;
    }
    let mut scores = Vec::with_capacity(end - start);
    let mut best = (0, 0.0_f32);
    for (offset, position) in (start..end).enumerate() {
        let score: f32 = template
            .iter()
            .zip(&recording[position..])
            .map(|(t, r)| t * r)
            .sum::<f32>()
            .abs();
        if score > best.1 {
            best = (offset, score);
        }
        scores.push(score);
    }
    scores.sort_by(f32::total_cmp);
    let median = scores[scores.len() / 2].max(f32::MIN_POSITIVE);
    let quality = best.1 / median;
    (quality >= MIN_QUALITY).then_some(Arrival {
        offset: best.0,
        quality,
    })
}

/// One device's measured latency: from the test chirp leaving the default
/// device's stream until the microphone hears it from this device.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceLatency {
    /// The measured device.
    pub id: DeviceId,
    /// The Windows default device, which plays natively (not delayed by us).
    pub is_source: bool,
    /// Milliseconds from the chirp leaving the stream to the microphone hearing it.
    pub measured_ms: f64,
    /// The sync delay the device had while being measured.
    pub delay: Delay,
}

impl DeviceLatency {
    /// The latency without our sync delay.
    fn undelayed_ms(&self) -> f64 {
        self.measured_ms - f64::from(self.delay.as_millis())
    }
}

/// What to do after a measurement round.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOutcome {
    /// The current default device works: these delays align every device to it.
    Aligned {
        /// Delay for every other device.
        delays: Vec<(DeviceId, Delay)>,
    },
    /// A device plays later than the default even without delay; make it the
    /// default and measure again.
    TryDefault(DeviceId),
    /// The best achievable setup, from two rounds.
    Best {
        /// The device to make the Windows default.
        default: DeviceId,
        /// Delays for every other device.
        delays: Vec<(DeviceId, Delay)>,
        /// Extra latency of devices played through the app (capture, buffer,
        /// playback) compared with the natively playing default device.
        overhead_ms: f64,
        /// How far the other devices still play after the default device;
        /// zero or less means perfect sync.
        behind_ms: f64,
    },
}

/// Plans after the first round, measured with the current default device.
/// Returns `None` when the default device could not be measured.
#[must_use]
pub fn plan_first_round(measured: &[DeviceLatency]) -> Option<CalibrationOutcome> {
    let reference = measured.iter().find(|d| d.is_source)?.measured_ms;
    let targets: Vec<&DeviceLatency> = measured.iter().filter(|d| !d.is_source).collect();
    let slowest = targets
        .iter()
        .max_by(|a, b| a.undelayed_ms().total_cmp(&b.undelayed_ms()));
    if let Some(slowest) = slowest
        && slowest.undelayed_ms() > reference + TOLERANCE_MS
    {
        return Some(CalibrationOutcome::TryDefault(slowest.id.clone()));
    }
    Some(CalibrationOutcome::Aligned {
        delays: targets
            .iter()
            .map(|t| (t.id.clone(), delay_of(reference - t.undelayed_ms())))
            .collect(),
    })
}

/// Plans after a second round with the slowest device as default: the same
/// device measured natively and through the app gives the app's overhead,
/// and with it every device's own latency, so the best default follows.
/// Returns `None` when the rounds do not fit together.
#[must_use]
pub fn plan_second_round(
    first: &[DeviceLatency],
    second: &[DeviceLatency],
) -> Option<CalibrationOutcome> {
    let new_default = second.iter().find(|d| d.is_source)?;
    let as_target = first
        .iter()
        .find(|d| d.id == new_default.id && !d.is_source)?;
    let overhead_ms = as_target.undelayed_ms() - new_default.measured_ms;

    // Each device's own (native) latency.
    let natives: Vec<(&DeviceId, f64)> = first
        .iter()
        .map(|d| {
            let native = if d.id == new_default.id {
                new_default.measured_ms
            } else if d.is_source {
                d.measured_ms
            } else {
                d.undelayed_ms() - overhead_ms
            };
            (&d.id, native)
        })
        .collect();

    // How far the others would play behind each candidate default.
    let behind = |candidate: &DeviceId| {
        let own = natives
            .iter()
            .find(|(id, _)| *id == candidate)
            .map(|(_, l)| *l)?;
        let latest_target = natives
            .iter()
            .filter(|(id, _)| *id != candidate)
            .map(|(_, l)| l + overhead_ms)
            .fold(f64::NEG_INFINITY, f64::max);
        Some((latest_target - own, own))
    };
    let (default, (behind_ms, own)) = natives
        .iter()
        .filter_map(|(id, _)| behind(id).map(|b| (*id, b)))
        .min_by(|a, b| a.1.0.total_cmp(&b.1.0))?;

    // Targets wait for whichever plays last: the default or the latest target.
    let reference = own.max(own + behind_ms);
    let target_order = second
        .iter()
        .map(|d| &d.id)
        .chain(first.iter().map(|d| &d.id))
        .filter(|id| *id != default);
    let mut delays: Vec<(DeviceId, Delay)> = Vec::new();
    for id in target_order {
        if delays.iter().any(|(known, _)| known == id) {
            continue;
        }
        if let Some((_, native)) = natives.iter().find(|(known, _)| *known == id) {
            delays.push((id.clone(), delay_of(reference - (native + overhead_ms))));
        }
    }
    Some(CalibrationOutcome::Best {
        default: default.clone(),
        delays,
        overhead_ms,
        behind_ms,
    })
}

/// Delays that align every device to the slowest one; used when the default
/// device is muted, so every audible device plays through the app.
#[must_use]
pub fn align_to_slowest(measured: &[DeviceLatency]) -> Vec<(DeviceId, Delay)> {
    let slowest = measured
        .iter()
        .map(DeviceLatency::undelayed_ms)
        .fold(f64::NEG_INFINITY, f64::max);
    measured
        .iter()
        .map(|d| (d.id.clone(), delay_of(slowest - d.undelayed_ms())))
        .collect()
}

fn delay_of(millis: f64) -> Delay {
    // Clamped to 0..=MAX_MILLIS first, so the cast cannot truncate or wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let millis = millis.round().clamp(0.0, f64::from(Delay::MAX_MILLIS)) as u16;
    Delay::from_millis(millis).unwrap_or(Delay::MAX)
}
