//! The assisted-sync workflow: measure each device by tapping along with
//! the click track, then suggest delays, then fine-tune by ear.

use std::time::Instant;

use stereo_bt_core::{Delay, DelaySuggestion, DeviceId, TapAnalysis, suggest_delay};

/// Milliseconds between clicks (matches the engine's click track).
const CLICK_PERIOD_MS: f64 = 1_000.0;
/// Taps asked for per device; a few may be discarded as strays.
pub(crate) const TAPS_WANTED: usize = 8;
/// Taps needed before moving on is allowed.
const TAPS_REQUIRED: usize = 5;

/// One device to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    /// The Windows default device: the reference everything is aligned to.
    pub(crate) is_source: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Tapping along with one isolated device.
    Measuring,
    /// Showing suggested delays.
    Results,
    /// All devices click together; the user adjusts delays by ear.
    FineTune,
}

/// The outcome for one non-default device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeasuredDevice {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    /// `None` when this device or the reference could not be measured.
    pub(crate) suggestion: Option<DelaySuggestion>,
}

#[derive(Debug, Clone)]
pub(crate) struct Assistant {
    epoch: Instant,
    steps: Vec<Step>,
    current: usize,
    taps: Vec<Instant>,
    latencies: Vec<Option<f64>>,
    phase: Phase,
}

impl Assistant {
    /// Starts measuring `steps` (the default device first) at `now`.
    pub(crate) fn new(steps: Vec<Step>, now: Instant) -> Self {
        Self {
            epoch: now,
            steps,
            current: 0,
            taps: Vec::new(),
            latencies: Vec::new(),
            phase: Phase::Measuring,
        }
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    /// The device being measured, while measuring.
    pub(crate) fn current_step(&self) -> Option<&Step> {
        (self.phase == Phase::Measuring)
            .then(|| self.steps.get(self.current))
            .flatten()
    }

    /// 1-based position of the current step and the number of steps.
    pub(crate) fn progress(&self) -> (usize, usize) {
        (self.current + 1, self.steps.len())
    }

    pub(crate) fn tap(&mut self, at: Instant) {
        if self.phase == Phase::Measuring {
            self.taps.push(at);
        }
    }

    pub(crate) fn taps(&self) -> usize {
        self.taps.len()
    }

    pub(crate) fn can_advance(&self) -> bool {
        self.phase == Phase::Measuring && self.taps.len() >= TAPS_REQUIRED
    }

    /// Discards the taps of the current device.
    pub(crate) fn restart_step(&mut self) {
        self.taps.clear();
    }

    /// Records the current device's latency from its taps and the click
    /// times, then moves on. Returns the next device to isolate, or `None`
    /// when all are measured (the phase is then [`Phase::Results`]).
    pub(crate) fn advance(&mut self, clicks: &[Instant]) -> Option<&Step> {
        if !self.can_advance() {
            return self.current_step();
        }
        let clicks_ms: Vec<f64> = clicks.iter().map(|&t| self.ms(t)).collect();
        let taps_ms: Vec<f64> = self.taps.iter().map(|&t| self.ms(t)).collect();
        self.latencies
            .push(TapAnalysis::new(&clicks_ms, &taps_ms, CLICK_PERIOD_MS).latency_ms());
        self.taps.clear();
        self.current += 1;
        if self.current >= self.steps.len() {
            self.phase = Phase::Results;
        }
        self.current_step()
    }

    /// Suggested delays for every non-default device, given each one's
    /// current delay.
    pub(crate) fn suggestions(
        &self,
        current_delay: impl Fn(&DeviceId) -> Delay,
    ) -> Vec<MeasuredDevice> {
        let reference = self
            .steps
            .iter()
            .zip(&self.latencies)
            .find(|(step, _)| step.is_source)
            .and_then(|(_, latency)| *latency);
        self.steps
            .iter()
            .zip(&self.latencies)
            .filter(|(step, _)| !step.is_source)
            .map(|(step, latency)| MeasuredDevice {
                id: step.id.clone(),
                name: step.name.clone(),
                suggestion: reference
                    .zip(*latency)
                    .map(|(r, d)| suggest_delay(r, d, current_delay(&step.id))),
            })
            .collect()
    }

    pub(crate) fn start_fine_tune(&mut self) {
        self.phase = Phase::FineTune;
    }

    /// Milliseconds since the assistant started (negative for earlier times).
    fn ms(&self, at: Instant) -> f64 {
        match at.checked_duration_since(self.epoch) {
            Some(after) => after.as_secs_f64() * 1_000.0,
            None => -(self.epoch.duration_since(at).as_secs_f64() * 1_000.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    fn step(raw: &str, is_source: bool) -> Step {
        Step {
            id: id(raw),
            name: format!("Device {raw}"),
            is_source,
        }
    }

    fn at(epoch: Instant, ms: u64) -> Instant {
        epoch + Duration::from_millis(ms)
    }

    /// Clicks every second for a minute, starting at the epoch.
    fn clicks(epoch: Instant) -> Vec<Instant> {
        (0..60).map(|n| at(epoch, n * 1_000)).collect()
    }

    /// Taps `lag_ms` after each of 8 clicks starting at second `from`.
    fn tap_along(assistant: &mut Assistant, epoch: Instant, from: u64, lag_ms: u64) {
        for n in from..from + 8 {
            assistant.tap(at(epoch, n * 1_000 + lag_ms));
        }
    }

    #[test]
    fn measures_each_device_in_turn_then_shows_results() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true), step("B", false)], epoch);
        assert_eq!(a.current_step().unwrap().id, id("A"));
        assert!(!a.can_advance());

        tap_along(&mut a, epoch, 1, 250);
        assert_eq!(a.advance(&clicks(epoch)).unwrap().id, id("B"));
        assert_eq!(a.progress(), (2, 2));
        tap_along(&mut a, epoch, 10, 100);

        assert_eq!(a.advance(&clicks(epoch)), None);
        assert_eq!(a.phase(), Phase::Results);
    }

    #[test]
    fn faster_device_is_told_to_wait_for_the_reference() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true), step("B", false)], epoch);
        tap_along(&mut a, epoch, 1, 250);
        a.advance(&clicks(epoch));
        tap_along(&mut a, epoch, 10, 100);
        a.advance(&clicks(epoch));

        let suggestions = a.suggestions(|_| Delay::from_millis(10).unwrap());

        assert_eq!(suggestions.len(), 1);
        let s = suggestions[0].suggestion.unwrap();
        assert_eq!(s.delay.as_millis(), 160);
        assert!(!s.too_late);
    }

    #[test]
    fn device_slower_than_the_reference_is_flagged() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true), step("B", false)], epoch);
        tap_along(&mut a, epoch, 1, 100);
        a.advance(&clicks(epoch));
        tap_along(&mut a, epoch, 10, 300);
        a.advance(&clicks(epoch));

        let s = a.suggestions(|_| Delay::ZERO)[0].suggestion.unwrap();

        assert!(s.too_late);
        assert_eq!(s.delay, Delay::ZERO);
    }

    #[test]
    fn cannot_advance_without_enough_taps_and_restart_clears_them() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true)], epoch);
        for n in 1..5 {
            a.tap(at(epoch, n * 1_000 + 200));
        }
        assert!(!a.can_advance());
        assert_eq!(a.advance(&clicks(epoch)).unwrap().id, id("A"));

        a.restart_step();

        assert_eq!(a.taps(), 0);
    }

    #[test]
    fn taps_without_clicks_give_no_suggestion() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true), step("B", false)], epoch);
        tap_along(&mut a, epoch, 1, 250);
        a.advance(&[]);
        tap_along(&mut a, epoch, 10, 100);
        a.advance(&[]);

        assert_eq!(a.suggestions(|_| Delay::ZERO)[0].suggestion, None);
    }

    #[test]
    fn taps_are_ignored_outside_measuring() {
        let epoch = Instant::now();
        let mut a = Assistant::new(vec![step("A", true)], epoch);
        tap_along(&mut a, epoch, 1, 250);
        a.advance(&clicks(epoch));
        a.start_fine_tune();

        a.tap(at(epoch, 20_000));

        assert_eq!(a.taps(), 0);
        assert_eq!(a.phase(), Phase::FineTune);
    }
}
