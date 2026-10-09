use crate::{Frame, StereoWidth};

/// Mid/side stereo width: scales the difference between left and right.
/// Wider makes a stereo pair sound further apart; `0` is mono. Normal width
/// is a bit-exact pass-through. No latency, no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StereoWidener {
    width: StereoWidth,
}

impl StereoWidener {
    /// Sets the width.
    pub fn set_width(&mut self, width: StereoWidth) {
        self.width = width;
    }

    /// Processes one frame.
    #[must_use]
    pub fn process_frame(&self, frame: Frame) -> Frame {
        if self.width == StereoWidth::NORMAL {
            return frame;
        }
        let mid = f32::midpoint(frame.left, frame.right);
        let side = (frame.left - frame.right) / 2.0 * f32::from(self.width.as_percent()) / 100.0;
        Frame::new(mid + side, mid - side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widener(percent: u8) -> StereoWidener {
        let mut w = StereoWidener::default();
        w.set_width(StereoWidth::from_percent(percent).unwrap());
        w
    }

    #[test]
    fn normal_width_is_a_bit_exact_pass_through() {
        let frame = Frame::new(0.3, -0.7);

        assert_eq!(widener(100).process_frame(frame), frame);
    }

    #[test]
    fn zero_width_is_mono() {
        let out = widener(0).process_frame(Frame::new(0.6, 0.2));

        assert!((out.left - 0.4).abs() < 1e-6 && (out.right - 0.4).abs() < 1e-6);
    }

    #[test]
    fn double_width_doubles_the_difference_and_keeps_the_middle() {
        let out = widener(200).process_frame(Frame::new(0.6, 0.2));

        assert!((out.left - out.right - 0.8).abs() < 1e-6);
        assert!((f32::midpoint(out.left, out.right) - 0.4).abs() < 1e-6);
    }
}
