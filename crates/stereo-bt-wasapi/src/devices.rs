use stereo_bt_core::{DeviceId, DeviceInfo};
use wasapi::{DeviceEnumerator, Direction};

use crate::Error;

/// Lists all active output (render) devices.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the devices cannot be enumerated or a device
/// reports an invalid identifier.
pub fn list_output_devices() -> Result<Vec<DeviceInfo>, Error> {
    let enumerator = DeviceEnumerator::new()?;
    // Having no default device is normal (e.g. nothing connected), not an error.
    let default_id = enumerator
        .get_default_device(&Direction::Render)
        .and_then(|device| device.get_id())
        .ok();
    let collection = enumerator.get_device_collection(&Direction::Render)?;

    let mut devices = Vec::new();
    for device in &collection {
        let device = device?;
        let raw_id = device.get_id()?;
        let is_default = default_id.as_deref() == Some(raw_id.as_str());
        devices.push(DeviceInfo {
            id: DeviceId::new(raw_id)?,
            name: device.get_friendlyname()?,
            is_default,
        });
    }
    Ok(devices)
}

/// The identifier of the current default output device, or `None` when there is none.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the enumerator fails or reports an invalid identifier.
pub fn default_output_id() -> Result<Option<DeviceId>, Error> {
    let enumerator = DeviceEnumerator::new()?;
    // No default device (nothing connected) is a normal state.
    let Ok(device) = enumerator.get_default_device(&Direction::Render) else {
        return Ok(None);
    };
    Ok(Some(DeviceId::new(device.get_id()?)?))
}
