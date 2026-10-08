//! Thin WASAPI adapter for `JOPL-StereoBT`.
//!
//! Wraps the `wasapi` crate into the few operations the engine needs:
//! listing output devices, loopback-capturing the default output device,
//! rendering to a specific device, and watching for device changes.
//! All streams use 48 kHz 32-bit float stereo with Windows doing any
//! format conversion. The only `unsafe` code lives in two small modules for
//! COM interfaces `wasapi` does not wrap: endpoint mute and MMCSS priority.
//!
//! Every thread that uses this crate must first hold a [`ComGuard`].
#![cfg(windows)]

mod com;
mod device_watcher;
mod devices;
mod endpoint_mute;
mod error;
mod loopback_capture;
mod mmcss;
mod render_stream;
mod stream_format;

pub use com::ComGuard;
pub use device_watcher::DeviceWatcher;
pub use devices::{default_output_id, list_output_devices};
pub use endpoint_mute::{is_muted, set_muted};
pub use error::Error;
pub use loopback_capture::LoopbackCapture;
pub use mmcss::MmcssGuard;
pub use render_stream::RenderStream;
pub use stream_format::STREAM_SAMPLE_RATE;
