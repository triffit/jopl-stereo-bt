//! Conversion between [`Frame`]s and interleaved little-endian 32-bit float
//! stereo PCM bytes, the stream format used on every WASAPI endpoint.
//!
//! Both functions are allocation-free and safe on a real-time thread.

use crate::Frame;

/// Bytes in one interleaved stereo `f32` frame.
pub const BYTES_PER_FRAME: usize = 8;

/// Decodes `bytes` into `out` and returns the number of frames written.
///
/// Stops at whichever of `bytes` or `out` runs out first; a trailing partial
/// frame in `bytes` is ignored.
pub fn decode_f32_stereo(bytes: &[u8], out: &mut [Frame]) -> usize {
    let (chunks, _partial) = bytes.as_chunks::<BYTES_PER_FRAME>();
    let mut count = 0;
    for (chunk, frame) in chunks.iter().zip(out.iter_mut()) {
        let [l0, l1, l2, l3, r0, r1, r2, r3] = *chunk;
        *frame = Frame::new(
            f32::from_le_bytes([l0, l1, l2, l3]),
            f32::from_le_bytes([r0, r1, r2, r3]),
        );
        count += 1;
    }
    count
}

/// Encodes `frames` into `out` and returns the number of frames written.
///
/// Stops at whichever of `frames` or `out` runs out first.
pub fn encode_f32_stereo(frames: &[Frame], out: &mut [u8]) -> usize {
    let (chunks, _partial) = out.as_chunks_mut::<BYTES_PER_FRAME>();
    let mut count = 0;
    for (frame, chunk) in frames.iter().zip(chunks.iter_mut()) {
        let [l0, l1, l2, l3] = frame.left.to_le_bytes();
        let [r0, r1, r2, r3] = frame.right.to_le_bytes();
        *chunk = [l0, l1, l2, l3, r0, r1, r2, r3];
        count += 1;
    }
    count
}
