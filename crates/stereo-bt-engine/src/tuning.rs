//! Fixed timing parameters, in frames at the 48 kHz stream rate.

/// Queue fill each output keeps before playing: 40 ms. Absorbs capture and
/// scheduling jitter; equal for all outputs so it does not affect sync.
pub(crate) const CUSHION_FRAMES: usize = 1_920;

/// Queue growth beyond the cushion tolerated before trimming: 30 ms.
pub(crate) const MAX_EXCESS_FRAMES: usize = 1_440;

/// Output queue capacity: 1 s.
pub(crate) const QUEUE_FRAMES: usize = 48_000;

/// How long stream threads block before re-checking the stop flag.
pub(crate) const WAIT_TIMEOUT_MS: u32 = 20;
