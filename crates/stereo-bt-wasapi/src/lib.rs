//! Thin WASAPI adapter for `JOPL-StereoBT`.
//!
//! Wraps the `wasapi` crate into the few operations the engine needs:
//! listing output devices, loopback-capturing the default output device,
//! rendering to a specific device, and watching for device changes.
//! All streams use 48 kHz 32-bit float stereo with Windows doing any
//! format conversion. The only `unsafe` code lives in four small modules for
//! Windows interfaces `wasapi` does not wrap: endpoint volume/mute, per-app
//! sessions, MMCSS priority and setting the default device.
//!
//! Every thread that uses this crate must first hold a [`ComGuard`].
#![cfg(windows)]

mod app_sessions;
mod battery;
mod com;
mod default_device;
mod device_watcher;
mod devices;
mod endpoint_volume;
mod error;
mod loopback_capture;
mod mic_capture;
mod mmcss;
mod render_stream;
mod stream_format;

pub use app_sessions::{muted_apps, unmute_app};
pub use battery::battery_levels;
pub use com::ComGuard;
pub use default_device::set_default_output;
pub use device_watcher::DeviceWatcher;
pub use devices::{default_output_id, list_output_devices};
pub use endpoint_volume::{is_muted, set_muted, set_volume_scalar, system_volume};
pub use error::Error;
pub use loopback_capture::LoopbackCapture;
pub use mic_capture::MicCapture;
pub use mmcss::MmcssGuard;
pub use render_stream::RenderStream;
pub use stream_format::STREAM_SAMPLE_RATE;
