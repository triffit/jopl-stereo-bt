//! Audio routing engine for `StereoBT`.
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
mod engine;
mod engine_config;
mod engine_loop;
mod fan_out;
mod mute_journal;
mod output_config;
mod output_controls;
mod platform;
mod render_pipeline;
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
pub use engine::{Engine, EngineError};
pub use engine_config::EngineConfig;
pub use mute_journal::{FileMuteJournal, MuteJournal};
pub use output_config::OutputConfig;
pub use platform::Platform;
pub use session::Session;
pub use status::{OutputStatus, StreamState};
pub use supervisor::{EngineStatus, Supervisor, SupervisorError};
#[cfg(windows)]
pub use wasapi_backend::WasapiBackend;
