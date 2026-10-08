//! Acceptance tests for interleaved 32-bit float stereo PCM conversion.

use proptest::prelude::*;
use stereo_bt_core::{Frame, pcm};

#[test]
fn decodes_little_endian_interleaved_frames() {
    let mut bytes = Vec::new();
    for sample in [0.5f32, -0.25, 1.0, -1.0] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let mut frames = [Frame::SILENCE; 2];

    let decoded = pcm::decode_f32_stereo(&bytes, &mut frames);

    assert_eq!(decoded, 2);
    assert_eq!(frames, [Frame::new(0.5, -0.25), Frame::new(1.0, -1.0)]);
}

#[test]
fn decode_stops_at_shorter_of_input_and_output() {
    let bytes = [0u8; 3 * pcm::BYTES_PER_FRAME];
    let mut frames = [Frame::new(9.0, 9.0); 2];

    assert_eq!(pcm::decode_f32_stereo(&bytes, &mut frames), 2);
}

#[test]
fn decode_ignores_trailing_partial_frame() {
    let bytes = [0u8; pcm::BYTES_PER_FRAME + 5];
    let mut frames = [Frame::new(9.0, 9.0); 4];

    assert_eq!(pcm::decode_f32_stereo(&bytes, &mut frames), 1);
    assert_eq!(frames[1], Frame::new(9.0, 9.0));
}

#[test]
fn encode_stops_at_shorter_of_input_and_output() {
    let frames = [Frame::new(0.1, 0.2); 4];
    let mut bytes = [0u8; 2 * pcm::BYTES_PER_FRAME];

    assert_eq!(pcm::encode_f32_stereo(&frames, &mut bytes), 2);
}

proptest! {
    #[test]
    fn encode_then_decode_round_trips(
        samples in prop::collection::vec((-1.0f32..=1.0, -1.0f32..=1.0), 0..256)
    ) {
        let frames: Vec<Frame> = samples.iter().map(|&(l, r)| Frame::new(l, r)).collect();
        let mut bytes = vec![0u8; frames.len() * pcm::BYTES_PER_FRAME];
        let mut decoded = vec![Frame::SILENCE; frames.len()];

        let encoded = pcm::encode_f32_stereo(&frames, &mut bytes);
        let count = pcm::decode_f32_stereo(&bytes, &mut decoded);

        prop_assert_eq!(encoded, frames.len());
        prop_assert_eq!(count, frames.len());
        prop_assert_eq!(decoded, frames);
    }
}
