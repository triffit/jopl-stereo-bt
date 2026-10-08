//! Minimal Windows `.ico` encoder: 32-bit BGRA bitmap entries, as used by
//! `build.rs` to embed the app icon in the executable.

/// Encodes square RGBA images (`(size, pixels)`, pixels row by row from the
/// top) into an `.ico` file. Sizes must be 1..=256.
pub(crate) fn encode(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    const DIR_SIZE: usize = 6;
    const ENTRY_SIZE: usize = 16;
    let bitmaps: Vec<Vec<u8>> = images
        .iter()
        .map(|(size, rgba)| bitmap(*size, rgba))
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&u16_of(images.len()).to_le_bytes());

    let mut offset = DIR_SIZE + ENTRY_SIZE * images.len();
    for ((size, _), data) in images.iter().zip(&bitmaps) {
        // A size of 256 is stored as 0.
        let dimension = u8::try_from(*size).unwrap_or(0);
        out.extend_from_slice(&[dimension, dimension, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // color planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&u32_of(data.len()).to_le_bytes());
        out.extend_from_slice(&u32_of(offset).to_le_bytes());
        offset += data.len();
    }
    for data in bitmaps {
        out.extend_from_slice(&data);
    }
    out
}

/// One icon image: `BITMAPINFOHEADER`, bottom-up BGRA pixels, empty AND mask.
fn bitmap(size: u32, rgba: &[u8]) -> Vec<u8> {
    let pixels_per_row = usize::try_from(size).unwrap_or(0);
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&size.to_le_bytes()); // width
    out.extend_from_slice(&(size * 2).to_le_bytes()); // height of color + mask
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&[0; 24]); // no compression, sizes and palette unused

    for row in rgba.chunks_exact(pixels_per_row * 4).rev() {
        for &[red, green, blue, alpha] in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    // The alpha channel defines transparency; the AND mask stays all opaque.
    let mask_row = pixels_per_row.div_ceil(32) * 4;
    out.resize(out.len() + mask_row * pixels_per_row, 0);
    out
}

fn u16_of(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn u32_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    fn solid(size: u32, rgba: [u8; 4]) -> (u32, Vec<u8>) {
        let count = usize::try_from(size * size).unwrap();
        (size, rgba.repeat(count))
    }

    #[test]
    fn directory_lists_every_image() {
        let ico = encode(&[solid(16, [0; 4]), solid(256, [0; 4])]);

        assert_eq!(u16_at(&ico, 2), 1);
        assert_eq!(u16_at(&ico, 4), 2);
        assert_eq!(ico[6], 16);
        assert_eq!(ico[6 + 16], 0, "256 is stored as 0");
    }

    #[test]
    fn entry_offsets_and_sizes_cover_the_file_exactly() {
        let ico = encode(&[solid(16, [0; 4]), solid(32, [0; 4])]);

        let second_offset = u32_at(&ico, 6 + 16 + 12);
        let second_size = u32_at(&ico, 6 + 16 + 8);

        assert_eq!(
            second_offset + second_size,
            u32::try_from(ico.len()).unwrap()
        );
    }

    #[test]
    fn pixels_are_stored_bottom_up_as_bgra() {
        let mut rgba = vec![0; 2 * 2 * 4];
        rgba[..4].copy_from_slice(&[1, 2, 3, 4]); // top-left pixel
        let ico = encode(&[(2, rgba)]);

        let pixels = usize::try_from(u32_at(&ico, 18)).unwrap() + 40;
        // Bottom-up: the top row is stored second.
        assert_eq!(&ico[pixels + 8..pixels + 12], &[3, 2, 1, 4]);
    }
}
