use std::error::Error as StdError;
use std::sync::Arc;

use stereo_bt_core::{DeviceId, DeviceInfo, SampleRate, SystemVolume};

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
