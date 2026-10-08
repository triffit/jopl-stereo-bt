use crate::Frame;

/// Variable-ratio resampler for clock-drift correction (PRODUCT.md F9).
///
/// Plays its input at `ratio` input frames per output frame, using 4-point
/// cubic Hermite (Catmull-Rom) interpolation. Meant for ratios within a few
/// tenths of a percent of 1, where it is transparent; it adds a constant
/// latency of 3 frames. Allocation-free and safe on a real-time thread.
#[derive(Debug, Clone)]
pub struct DriftResampler {
    /// The last four input frames, oldest first; output interpolates between
    /// `history[1]` and `history[2]`.
    history: [Frame; 4],
    /// Fractional read position between `history[1]` and `history[2]`, in `0.0..1.0`.
    position: f64,
}

impl Default for DriftResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl DriftResampler {
    /// Creates a resampler whose history is silent.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [Frame::SILENCE; 4],
            position: 0.0,
        }
    }

    /// How many input frames the next [`Self::process`] call with the same
    /// arguments consumes to produce `out_len` output frames.
    #[must_use]
    pub fn input_needed(&self, out_len: usize, ratio: f64) -> usize {
        whole(self.position + to_f64(out_len) * ratio)
    }

    /// Produces `out.len()` frames from `input`, which should hold exactly
    /// [`Self::input_needed`] frames; missing input is treated as silence and
    /// surplus input is ignored.
    pub fn process(&mut self, input: &[Frame], ratio: f64, out: &mut [Frame]) {
        let mut next_input = input.iter().copied();
        let mut consumed = 0;
        for (k, frame) in out.iter_mut().enumerate() {
            // Computed from the start of the block (not accumulated) so the
            // total matches `input_needed` exactly.
            let read_at = self.position + to_f64(k) * ratio;
            let shifts = whole(read_at);
            while consumed < shifts {
                self.push(next_input.next().unwrap_or(Frame::SILENCE));
                consumed += 1;
            }
            *frame = self.interpolate(fraction(read_at));
        }
        let end = self.position + to_f64(out.len()) * ratio;
        while consumed < whole(end) {
            self.push(next_input.next().unwrap_or(Frame::SILENCE));
            consumed += 1;
        }
        self.position = end - end.floor();
    }

    fn push(&mut self, frame: Frame) {
        self.history.rotate_left(1);
        self.history[3] = frame;
    }

    fn interpolate(&self, t: f32) -> Frame {
        let [y0, y1, y2, y3] = self.history;
        Frame::new(
            catmull_rom(y0.left, y1.left, y2.left, y3.left, t),
            catmull_rom(y0.right, y1.right, y2.right, y3.right, t),
        )
    }
}

/// Catmull-Rom spline through `y1` (t = 0) and `y2` (t = 1).
fn catmull_rom(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    y1 + 0.5
        * t
        * (y2 - y0 + t * (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3 + t * (3.0 * (y1 - y2) + y3 - y0)))
}

fn to_f64(count: usize) -> f64 {
    // Block sizes are far below 2^52, so the conversion is exact.
    #[allow(clippy::cast_precision_loss)]
    let value = count as f64;
    value
}

fn whole(position: f64) -> usize {
    // Positions are non-negative and far below usize::MAX; floor() is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = position.floor() as usize;
    count
}

fn fraction(position: f64) -> f32 {
    // In 0.0..1.0; f32 precision is ample for an interpolation weight.
    #[allow(clippy::cast_possible_truncation)]
    let t = (position - position.floor()) as f32;
    t
}
