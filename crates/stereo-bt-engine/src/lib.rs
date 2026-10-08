//! Audio routing engine for `StereoBT`.
//!
//! A [`Session`] runs one capture thread that fans every captured block out
//! to a lock-free queue per output, and one render thread per output device
//! that plays its queue through delay, channel mapping and volume.
//! The platform is abstracted by [`Backend`]; [`WasapiBackend`] is the
//! Windows implementation.

mod backend;
mod fan_out;
mod output_config;
mod output_controls;
mod render_pipeline;
mod session;
mod state_cell;
mod status;
mod tuning;
#[cfg(windows)]
mod wasapi_backend;

pub use backend::{Backend, CaptureStream, RenderSink};
pub use output_config::OutputConfig;
pub use session::Session;
pub use status::{OutputStatus, StreamState};
#[cfg(windows)]
pub use wasapi_backend::WasapiBackend;
