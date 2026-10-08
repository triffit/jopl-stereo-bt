use std::error::Error as StdError;
use std::sync::Arc;

use stereo_bt_core::{DeviceId, SampleRate};

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

    /// The current default output device (the capture source), if any.
    ///
    /// # Errors
    ///
    /// Returns an error when devices cannot be queried.
    fn default_output(&self) -> Result<Option<DeviceId>, Self::Error>;

    /// All currently active output devices.
    ///
    /// # Errors
    ///
    /// Returns an error when devices cannot be queried.
    fn active_outputs(&self) -> Result<Vec<DeviceId>, Self::Error>;

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
