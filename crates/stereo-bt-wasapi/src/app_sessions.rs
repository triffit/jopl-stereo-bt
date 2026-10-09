//! Per-application audio sessions of a device (the Windows volume mixer),
//! used to warn about, and undo, an app that is muted on the default device.

// COM and process calls through the `windows` crate; see the SAFETY notes.
#![allow(unsafe_code)]

use std::path::Path;

use stereo_bt_core::{DeviceId, MutedApp};
use wasapi::WasapiError;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_core::{Interface, PCWSTR, PWSTR};

use crate::Error;

/// Applications currently playing on device `id` while muted for it in the
/// volume mixer.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device or its sessions cannot be queried.
pub fn muted_apps(id: &DeviceId) -> Result<Vec<MutedApp>, Error> {
    let mut muted = Vec::new();
    for (control, volume) in sessions(id)? {
        // SAFETY: valid interface pointers from the session enumerator; the
        // getters only write their out-parameters.
        let (state, is_muted, pid) =
            unsafe { (control.GetState(), volume.GetMute(), control.GetProcessId()) };
        let active = state.map_err(WasapiError::from)? == AudioSessionStateActive;
        // The system sounds session has no process (pid 0); skip it.
        if let (true, Ok(true), Ok(pid @ 1..)) =
            (active, is_muted.map(windows_core::BOOL::as_bool), pid)
        {
            muted.push(MutedApp {
                pid,
                name: process_name(pid).unwrap_or_else(|| format!("process {pid}")),
            });
        }
    }
    Ok(muted)
}

/// Unmutes every audio session of process `pid` on device `id`.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device or its sessions cannot be changed.
pub fn unmute_app(id: &DeviceId, pid: u32) -> Result<(), Error> {
    for (control, volume) in sessions(id)? {
        // SAFETY: valid interface pointer; the getter only writes its out-parameter.
        if unsafe { control.GetProcessId() }.ok() == Some(pid) {
            // SAFETY: valid interface pointer; a null event-context GUID
            // pointer is explicitly allowed by the API.
            unsafe { volume.SetMute(false, std::ptr::null()) }.map_err(WasapiError::from)?;
        }
    }
    Ok(())
}

/// Every session of device `id` with its volume control.
fn sessions(id: &DeviceId) -> Result<Vec<(IAudioSessionControl2, ISimpleAudioVolume)>, Error> {
    let wide_id: Vec<u16> = id.as_str().encode_utf16().chain([0]).collect();
    // SAFETY: standard COM instantiation; without COM initialised on this
    // thread it fails with an error rather than misbehaving.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(WasapiError::from)?;
    // SAFETY: `wide_id` is NUL-terminated UTF-16 and outlives the call.
    let device =
        unsafe { enumerator.GetDevice(PCWSTR(wide_id.as_ptr())) }.map_err(WasapiError::from)?;
    // SAFETY: activating a documented interface on a valid device pointer.
    let manager = unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) }
        .map_err(WasapiError::from)?;
    // SAFETY: valid interface pointer; returns a new enumerator object.
    let list = unsafe { manager.GetSessionEnumerator() }.map_err(WasapiError::from)?;
    // SAFETY: valid interface pointer; only writes its out-parameter.
    let count = unsafe { list.GetCount() }.map_err(WasapiError::from)?;
    let mut sessions = Vec::new();
    for index in 0..count {
        // SAFETY: `index` is within 0..count as reported by the enumerator.
        let control = unsafe { list.GetSession(index) }.map_err(WasapiError::from)?;
        // Every session supports both interfaces; QueryInterface is safe to call.
        let control2: IAudioSessionControl2 = control.cast().map_err(WasapiError::from)?;
        let volume: ISimpleAudioVolume = control.cast().map_err(WasapiError::from)?;
        sessions.push((control2, volume));
    }
    Ok(sessions)
}

/// The executable name of process `pid`, without path or extension.
fn process_name(pid: u32) -> Option<String> {
    // SAFETY: plain process query with the least access right; fails for
    // protected or exited processes, which we then simply do not name.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut length = u32::try_from(buffer.len()).unwrap_or(0);
    // SAFETY: `buffer` is writable for `length` UTF-16 units and `length` is
    // a valid in/out pointer; the handle is valid until closed below.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
    };
    // SAFETY: `process` is a valid handle we own; closed exactly once.
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(buffer.get(..usize::try_from(length).ok()?)?);
    Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}
