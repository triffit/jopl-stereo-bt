use stereo_bt_core::SampleRate;
use wasapi::{SampleType, WaveFormat};

/// Sample rate of every capture and render stream.
pub const STREAM_SAMPLE_RATE: SampleRate = SampleRate::HZ_48000;

/// 48 kHz, 32-bit float, interleaved stereo.
pub(crate) fn wave_format() -> WaveFormat {
    let hz = STREAM_SAMPLE_RATE.as_hz() as usize;
    WaveFormat::new(32, 32, &SampleType::Float, hz, 2, None)
}
