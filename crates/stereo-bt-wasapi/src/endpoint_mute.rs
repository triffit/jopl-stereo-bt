//! Endpoint mute via `IAudioEndpointVolume`, which the `wasapi` crate does not wrap.
//!
//! Used to silence the default device while its loopback stays the audio
//! source: loopback is captured before endpoint mute is applied.

// The only `unsafe` in this crate besides `mmcss`: plain COM calls through
// the `windows` crate. Each block states the invariants it relies on.
#![allow(unsafe_code)]

use stereo_bt_core::DeviceId;
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
