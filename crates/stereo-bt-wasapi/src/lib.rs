//! Thin WASAPI adapter for `StereoBT`.
//!
//! Wraps the `wasapi` crate into the few operations the engine needs:
//! listing output devices, loopback-capturing the default output device,
//! rendering to a specific device, and watching for device changes.
//! All streams use 48 kHz 32-bit float stereo with Windows doing any
//! format conversion. This crate contains no `unsafe` code of its own.
//!
//! Every thread that uses this crate must first hold a [`ComGuard`].
#![cfg(windows)]

mod com;
mod device_watcher;
mod devices;
mod error;
mod loopback_capture;
mod render_stream;
mod stream_format;

pub use com::ComGuard;
pub use device_watcher::DeviceWatcher;
pub use devices::list_output_devices;
pub use error::Error;
pub use loopback_capture::LoopbackCapture;
pub use render_stream::RenderStream;
pub use stream_format::STREAM_SAMPLE_RATE;
