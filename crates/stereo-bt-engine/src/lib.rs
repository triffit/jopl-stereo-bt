//! Audio routing engine for `JOPL-StereoBT`.
//!
//! A [`Session`] runs one capture thread that fans every captured block out
//! to a lock-free queue per output, and one render thread per output device
//! that plays its queue through delay, channel mapping and volume.
//! The platform is abstracted by [`Backend`]; [`WasapiBackend`] is the
//! Windows implementation.
//!
//! A [`Supervisor`] keeps the session in line with the configuration and the
//! devices present (hot-plug, default changes, default-device mute), and an
//! [`Engine`] runs it on its own thread as the UI's single entry point.

#![forbid(unsafe_code)]

mod backend;
mod calibration;
mod click_track;
mod engine;
mod engine_config;
mod engine_loop;
mod fan_out;
mod mic_timeline;
mod output_config;
mod output_controls;
mod platform;
mod render_pipeline;
mod restore_journal;
mod session;
mod state_cell;
mod status;
mod supervisor;
#[cfg(test)]
mod test_fakes;
mod tuning;
#[cfg(windows)]
mod wasapi_backend;

pub use backend::{Backend, CaptureStream, RenderSink};
pub use calibration::{CalibrationReport, CalibrationRequest, CalibrationState};
pub use click_track::TestSignal;
pub use engine::{Engine, EngineError};
pub use engine_config::EngineConfig;
pub use output_config::OutputConfig;
pub use platform::Platform;
pub use restore_journal::{FileRestoreJournal, RestoreJournal};
pub use session::Session;
pub use status::{OutputStatus, StreamState};
pub use supervisor::{EngineStatus, Supervisor, SupervisorError};
#[cfg(windows)]
pub use wasapi_backend::WasapiBackend;
