//! Platform-independent audio DSP and configuration model for `JOPL-StereoBT`.
//!
//! Everything here compiles and is tested without Windows. The real-time
//! processing types ([`OutputChain`], [`DelayLine`], [`pcm`]) never allocate
//! after construction, so they are safe to use on an audio render thread.

#![forbid(unsafe_code)]

mod auto_calibration;
mod bass_cut;
mod bass_shaper;
mod biquad;
mod channel_mode;
mod click;
mod delay;
mod delay_line;
mod device_event;
mod device_id;
mod device_info;
mod drift_control;
mod drift_resampler;
mod error;
mod frame;
mod limiter;
mod muted_app;
mod output_chain;
pub mod pcm;
mod peak_meter;
mod ping;
mod playout;
mod reverb;
mod sample_rate;
mod space;
mod stereo_widener;
mod system_volume;
mod tap_analysis;
mod volume;

pub use auto_calibration::{
    Arrival, CalibrationOutcome, DeviceLatency, align_to_slowest, find_arrival, plan_first_round,
    plan_second_round,
};
pub use bass_cut::{BassCut, VirtualBass};
pub use bass_shaper::BassShaper;
pub use channel_mode::ChannelMode;
pub use click::ClickGenerator;
pub use delay::Delay;
pub use delay_line::DelayLine;
pub use device_event::DeviceEvent;
pub use device_id::DeviceId;
pub use device_info::DeviceInfo;
pub use drift_control::DriftControl;
pub use drift_resampler::DriftResampler;
pub use error::Error;
pub use frame::Frame;
pub use limiter::Limiter;
pub use muted_app::MutedApp;
pub use output_chain::OutputChain;
pub use peak_meter::{PeakMeter, meter_fill};
pub use ping::Ping;
pub use playout::{Playout, PlayoutPlan};
pub use reverb::Reverb;
pub use sample_rate::SampleRate;
pub use space::{Percent, Space, StereoWidth};
pub use stereo_widener::StereoWidener;
pub use system_volume::SystemVolume;
pub use tap_analysis::{DelaySuggestion, TapAnalysis, suggest_delay};
pub use volume::Volume;
