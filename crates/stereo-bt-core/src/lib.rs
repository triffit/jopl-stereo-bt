//! Platform-independent audio DSP and configuration model for `StereoBT`.
//!
//! Everything here compiles and is tested without Windows. The real-time
//! processing types ([`OutputChain`], [`DelayLine`]) never allocate after
//! construction, so they are safe to use on an audio render thread.

mod channel_mode;
mod delay;
mod delay_line;
mod error;
mod frame;
mod output_chain;
mod sample_rate;
mod volume;

pub use channel_mode::ChannelMode;
pub use delay::Delay;
pub use delay_line::DelayLine;
pub use error::Error;
pub use frame::Frame;
pub use output_chain::OutputChain;
pub use sample_rate::SampleRate;
pub use volume::Volume;
