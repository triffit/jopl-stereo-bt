use std::error::Error as StdError;
use std::sync::Arc;

use stereo_bt_core::{DeviceId, DeviceInfo, MutedApp, SampleRate, SystemVolume};

use crate::Backend;

/// Device queries and controls the supervisor needs besides streaming.
///
/// Called only from the supervisor's own thread.
pub trait Platform {
    /// The streaming backend.
    type Backend: Backend;
    /// Error returned by device queries and controls.
    type Error: StdError + Send + Sync + 'static;

    /// The backend sessions stream through.
    fn backend(&self) -> &Arc<Self::Backend>;

    /// The stream sample rate.
    fn sample_rate(&self) -> SampleRate;

    /// All currently active output devices; at most one has `is_default` set
    /// (the capture source).
    ///
    /// # Errors
    ///
    /// Returns an error when devices cannot be queried.
    fn outputs(&self) -> Result<Vec<DeviceInfo>, Self::Error>;

    /// The Windows volume of device `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or cannot be queried.
    fn system_volume(&self, id: &DeviceId) -> Result<SystemVolume, Self::Error>;

    /// Sets the Windows volume slider of device `id` (`0.0..=1.0`).
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or cannot be changed.
    fn set_volume(&self, id: &DeviceId, scalar: f32) -> Result<(), Self::Error>;

    /// Applications playing on device `id` while muted for it in the volume mixer.
    ///
    /// # Errors
    ///
    /// Returns an error when the device cannot be queried.
    fn muted_apps(&self, id: &DeviceId) -> Result<Vec<MutedApp>, Self::Error>;

    /// Battery level in percent of each device in `known` that reports one.
    ///
    /// # Errors
    ///
    /// Returns an error when devices cannot be queried.
    fn battery_levels(&self, known: &[DeviceId]) -> Result<Vec<(DeviceId, u8)>, Self::Error>;

    /// Unmutes process `pid` on device `id` in the volume mixer.
    ///
    /// # Errors
    ///
    /// Returns an error when the device cannot be changed.
    fn unmute_app(&self, id: &DeviceId, pid: u32) -> Result<(), Self::Error>;

    /// Makes device `id` the Windows default output device.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or Windows refuses.
    fn set_default_output(&self, id: &DeviceId) -> Result<(), Self::Error>;

    /// Whether device `id` is muted.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or cannot be queried.
    fn is_muted(&self, id: &DeviceId) -> Result<bool, Self::Error>;

    /// Mutes or unmutes device `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or cannot be changed.
    fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<(), Self::Error>;
}
