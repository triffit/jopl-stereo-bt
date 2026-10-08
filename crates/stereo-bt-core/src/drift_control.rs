/// Steers a [`crate::DriftResampler`] so an output queue stays at its target
/// fill level despite the capture and device clocks running at slightly
/// different rates (PRODUCT.md F9).
///
/// A PI controller on the smoothed, normalized fill error: the proportional
/// part reacts to the current error, the integral part learns the steady
/// clock difference so the queue settles exactly at the target (no sync
/// offset). The ratio stays within ±[`Self::MAX_DEVIATION`], an inaudible
/// pitch change. Allocation-free and safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct DriftControl {
    target: f64,
    smoothed_error: f64,
    integral: f64,
    ratio: f64,
}

impl DriftControl {
    /// Largest speed-up or slow-down: 0.2 % (about 3.5 cents).
    pub const MAX_DEVIATION: f64 = 0.002;

    /// Weight of each new measurement: a time constant of 200 render cycles
    /// (about 2 s) filters out block-size jitter of the fill level.
    const SMOOTHING: f64 = 0.005;
    /// Proportional and integral gains, tuned for a well-damped response
    /// (about 30 s to settle) with 10 ms render cycles and a 40 ms cushion.
    const PROPORTIONAL: f64 = 0.011;
    const INTEGRAL: f64 = 0.000_016;

    /// Creates a controller for a queue that should hold `target` frames.
    #[must_use]
    pub fn new(target: usize) -> Self {
        // Cushions are a few thousand frames; the conversion is exact.
        #[allow(clippy::cast_precision_loss)]
        let target = target.max(1) as f64;
        Self {
            target,
            smoothed_error: 0.0,
            integral: 0.0,
            ratio: 1.0,
        }
    }

    /// Feeds the queue fill level seen at the start of a render cycle.
    pub fn update(&mut self, available: usize) {
        #[allow(clippy::cast_precision_loss)]
        let error = (available as f64 - self.target) / self.target;
        self.smoothed_error += Self::SMOOTHING * (error - self.smoothed_error);
        self.integral = (self.integral + Self::INTEGRAL * self.smoothed_error)
            .clamp(-Self::MAX_DEVIATION, Self::MAX_DEVIATION);
        let correction = (Self::PROPORTIONAL * self.smoothed_error + self.integral)
            .clamp(-Self::MAX_DEVIATION, Self::MAX_DEVIATION);
        self.ratio = 1.0 + correction;
    }

    /// Input frames to consume per output frame.
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Forgets the measured drift, e.g. after a device restart.
    pub fn reset(&mut self) {
        self.smoothed_error = 0.0;
        self.integral = 0.0;
        self.ratio = 1.0;
    }
}
