/// One stereo sample pair in 32-bit float, nominal range `-1.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    /// Left channel sample.
    pub left: f32,
    /// Right channel sample.
    pub right: f32,
}

impl Frame {
    /// A frame of digital silence.
    pub const SILENCE: Self = Self::new(0.0, 0.0);

    /// Creates a frame from its left and right samples.
    #[must_use]
    pub const fn new(left: f32, right: f32) -> Self {
        Self { left, right }
    }

    /// Multiplies both channels by `gain`.
    #[must_use]
    pub fn scaled(self, gain: f32) -> Self {
        Self::new(self.left * gain, self.right * gain)
    }
}
