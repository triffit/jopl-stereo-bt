//! The app icon, drawn in code: two overlapping circles (left and right
//! device). Avoids an image decoder dependency for the tray icon.

/// Accent colors of the left and right circle.
const LEFT: [f32; 3] = [76.0, 194.0, 255.0];
const RIGHT: [f32; 3] = [153.0, 235.0, 255.0];

/// The icon as `size × size` RGBA pixels, row by row.
pub(crate) fn rgba(size: u32) -> Vec<u8> {
    let s = f32::from(u16::try_from(size).unwrap_or(u16::MAX));
    let radius = s * 0.28;
    let circles = [(s * 0.375, s * 0.5, LEFT), (s * 0.625, s * 0.5, RIGHT)];
    let mut pixels = Vec::with_capacity(usize::try_from(size * size * 4).unwrap_or(0));
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (to_f32(x) + 0.5, to_f32(y) + 0.5);
            let mut color = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for &(cx, cy, rgb) in &circles {
                // Anti-aliased coverage of this pixel by the circle.
                let coverage = (radius - (px - cx).hypot(py - cy) + 0.5).clamp(0.0, 1.0);
                for (channel, value) in color.iter_mut().zip(rgb) {
                    *channel = *channel * (1.0 - coverage) + value * coverage;
                }
                alpha += coverage * (1.0 - alpha);
            }
            pixels.extend(color.map(to_byte));
            pixels.push(to_byte(alpha * 255.0));
        }
    }
    pixels
}

fn to_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

fn to_byte(value: f32) -> u8 {
    // Clamped to 0..=255 first, so the cast cannot truncate or wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = value.round().clamp(0.0, 255.0) as u8;
    byte
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(icon: &[u8], size: usize, x: usize, y: usize) -> &[u8] {
        let at = (y * size + x) * 4;
        &icon[at..at + 4]
    }

    #[test]
    fn has_four_bytes_per_pixel() {
        assert_eq!(rgba(32).len(), 32 * 32 * 4);
    }

    #[test]
    fn center_is_opaque_and_corners_transparent() {
        let icon = rgba(32);

        assert_eq!(pixel(&icon, 32, 16, 16)[3], 255);
        assert_eq!(pixel(&icon, 32, 0, 0)[3], 0);
        assert_eq!(pixel(&icon, 32, 31, 31)[3], 0);
    }
}
