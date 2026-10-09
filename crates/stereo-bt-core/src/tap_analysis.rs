use crate::Delay;

/// Fewest accepted taps for a latency estimate.
const MIN_TAPS: usize = 5;
/// Taps further than this from the median are strays (a missed click
/// answered late, a double tap), unless the taps are generally more spread.
const MIN_TOLERANCE_MS: f64 = 60.0;

/// Estimates when a device plays the click track, from the user tapping
/// along: the median lag from each click to the tap that follows it.
///
/// The user's reaction time is included, but it is the same for every
/// device, so it cancels out when devices are compared.
#[derive(Debug, Clone, PartialEq)]
pub struct TapAnalysis {
    lags: Vec<f64>,
}

impl TapAnalysis {
    /// Analyzes tap times against click emission times (both in
    /// milliseconds on the same clock); a lag must be shorter than `period_ms`.
    #[must_use]
    pub fn new(clicks_ms: &[f64], taps_ms: &[f64], period_ms: f64) -> Self {
        let raw: Vec<f64> = taps_ms
            .iter()
            .filter_map(|&tap| {
                let click = clicks_ms
                    .iter()
                    .copied()
                    .filter(|&click| click <= tap)
                    .fold(None, |latest: Option<f64>, click| {
                        Some(latest.map_or(click, |l| l.max(click)))
                    })?;
                let lag = tap - click;
                (lag < period_ms).then_some(lag)
            })
            .collect();

        let Some(center) = median(&raw) else {
            return Self { lags: raw };
        };
        let deviations: Vec<f64> = raw.iter().map(|lag| (lag - center).abs()).collect();
        let spread = median(&deviations).unwrap_or(0.0);
        let tolerance = (3.0 * spread).max(MIN_TOLERANCE_MS);
        let lags = raw
            .into_iter()
            .filter(|lag| (lag - center).abs() <= tolerance)
            .collect();
        Self { lags }
    }

    /// Number of taps accepted after removing strays.
    #[must_use]
    pub fn taps(&self) -> usize {
        self.lags.len()
    }

    /// The estimated click-to-tap latency, once there are enough taps.
    #[must_use]
    pub fn latency_ms(&self) -> Option<f64> {
        if self.lags.len() < MIN_TAPS {
            return None;
        }
        median(&self.lags)
    }
}

/// A suggested delay for one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelaySuggestion {
    /// The delay to use for the device.
    pub delay: Delay,
    /// The device plays later than the reference even without delay; only
    /// making the slowest device the default can fix that.
    pub too_late: bool,
}

/// The delay that makes a device (measured at `device_ms`, currently using
/// `current`) play together with the reference device (`reference_ms`).
#[must_use]
pub fn suggest_delay(reference_ms: f64, device_ms: f64, current: Delay) -> DelaySuggestion {
    /// Shortfalls this small are inaudible; not worth a warning.
    const TOLERANCE_MS: f64 = 5.0;
    let wanted = f64::from(current.as_millis()) + reference_ms - device_ms;
    let max = f64::from(Delay::MAX_MILLIS);
    // Clamped to 0..=MAX_MILLIS first, so the cast cannot truncate or wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let millis = wanted.round().clamp(0.0, max) as u16;
    DelaySuggestion {
        delay: Delay::from_millis(millis).unwrap_or(Delay::MAX),
        too_late: wanted < -TOLERANCE_MS,
    }
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    } else {
        sorted[middle]
    })
}
