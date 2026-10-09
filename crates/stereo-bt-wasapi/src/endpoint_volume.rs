//! Endpoint volume and mute via `IAudioEndpointVolume`, which the `wasapi`
//! crate does not wrap.
//!
//! Loopback is captured before endpoint volume and mute are applied, so the
//! app can mute the default device without losing its source, and must apply
//! the system volume itself to follow it on the other devices.

// The only `unsafe` in this crate besides `mmcss`: plain COM calls through
// the `windows` crate. Each block states the invariants it relies on.
#![allow(unsafe_code)]

use stereo_bt_core::{DeviceId, SystemVolume};
use wasapi::WasapiError;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::PCWSTR;

use crate::Error;

/// Whether the endpoint `id` is muted.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device does not exist or cannot be queried.
pub fn is_muted(id: &DeviceId) -> Result<bool, Error> {
    let volume = endpoint_volume(id)?;
    // SAFETY: `volume` is a valid interface pointer obtained from `Activate`;
    // `GetMute` only writes its out-parameter, which the wrapper owns.
    let muted = unsafe { volume.GetMute() }.map_err(WasapiError::from)?;
    Ok(muted.as_bool())
}

/// Mutes or unmutes the endpoint `id`.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device does not exist or cannot be changed.
pub fn set_muted(id: &DeviceId, muted: bool) -> Result<(), Error> {
    let volume = endpoint_volume(id)?;
    // SAFETY: `volume` is a valid interface pointer; a null event-context
    // GUID pointer is explicitly allowed by the API.
    unsafe { volume.SetMute(muted, std::ptr::null()) }.map_err(WasapiError::from)?;
    Ok(())
}

/// The Windows volume (decibels, slider position, mute) of endpoint `id`.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device does not exist or cannot be queried.
pub fn system_volume(id: &DeviceId) -> Result<SystemVolume, Error> {
    let volume = endpoint_volume(id)?;
    // SAFETY: `volume` is a valid interface pointer; each getter only writes
    // its out-parameter, which the wrapper owns.
    let (level_db, scalar, muted) = unsafe {
        (
            volume.GetMasterVolumeLevel(),
            volume.GetMasterVolumeLevelScalar(),
            volume.GetMute(),
        )
    };
    Ok(SystemVolume::new(
        level_db.map_err(WasapiError::from)?,
        scalar.map_err(WasapiError::from)?,
        muted.map_err(WasapiError::from)?.as_bool(),
    ))
}

/// Sets the Windows volume slider of endpoint `id` (`0.0..=1.0`).
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device does not exist or cannot be changed.
pub fn set_volume_scalar(id: &DeviceId, scalar: f32) -> Result<(), Error> {
    let volume = endpoint_volume(id)?;
    // SAFETY: `volume` is a valid interface pointer; a null event-context
    // GUID pointer is explicitly allowed by the API.
    unsafe { volume.SetMasterVolumeLevelScalar(scalar.clamp(0.0, 1.0), std::ptr::null()) }
        .map_err(WasapiError::from)?;
    Ok(())
}

fn endpoint_volume(id: &DeviceId) -> Result<IAudioEndpointVolume, Error> {
    let wide_id: Vec<u16> = id.as_str().encode_utf16().chain([0]).collect();
    // SAFETY: standard COM instantiation; without COM initialised on this
    // thread it fails with an error rather than misbehaving.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(WasapiError::from)?;
    // SAFETY: `wide_id` is NUL-terminated UTF-16 and outlives the call.
    let device =
        unsafe { enumerator.GetDevice(PCWSTR(wide_id.as_ptr())) }.map_err(WasapiError::from)?;
    // SAFETY: activating a documented interface on a valid device pointer;
    // no activation parameters are passed.
    let volume = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
        .map_err(WasapiError::from)?;
    Ok(volume)
}
