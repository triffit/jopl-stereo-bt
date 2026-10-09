//! Changing the Windows default output device.
//!
//! Windows has no public API for this. Like `EarTrumpet` and `SoundSwitch`, this
//! uses the undocumented `IPolicyConfig` interface of the `CPolicyConfigClient`
//! COM class, which the Windows sound settings themselves use and which has
//! been stable since Windows 7. If a future Windows version drops it, the call
//! fails with an error instead of misbehaving.

// COM calls through a hand-declared interface; see the SAFETY notes.
#![allow(unsafe_code)]

use stereo_bt_core::DeviceId;
use wasapi::WasapiError;
use windows::Win32::Media::Audio::{eConsole, eMultimedia};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows_core::{GUID, PCWSTR};

use self::policy_config::IPolicyConfig;

use crate::Error;

/// `CPolicyConfigClient`.
const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870a_f99c_171d_4f9e_af0d_e63d_f40c_2bc9);

/// The hand-declared COM interface. Method names must match the COM
/// interface exactly, and the `interface` macro generates the vtable glue,
/// so lints about naming, unused slots and the macro's pointer casts do not
/// apply here.
#[allow(
    non_snake_case,
    dead_code,
    clippy::transmute_ptr_to_ptr,
    clippy::undocumented_unsafe_blocks,
    clippy::pedantic
)]
mod policy_config {
    use std::ffi::c_void;

    use windows::Win32::Media::Audio::ERole;
    use windows_core::{BOOL, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

    /// `IPolicyConfig` (Windows 7 and later). Every method must be declared, in
    /// order, so the vtable layout matches; only `SetDefaultEndpoint` is called.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(
            &self,
            device: PCWSTR,
            default: BOOL,
            format: *mut *mut c_void,
        ) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(
            &self,
            device: PCWSTR,
            endpoint: *mut c_void,
            mix: *mut c_void,
        ) -> HRESULT;
        fn GetProcessingPeriod(
            &self,
            device: PCWSTR,
            default: BOOL,
            default_period: *mut i64,
            minimum_period: *mut i64,
        ) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        fn SetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        pub fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: BOOL) -> HRESULT;
    }
}

/// Makes `id` the Windows default output device, for the same roles the
/// Windows sound settings set: console (system and apps) and multimedia.
///
/// Requires a [`crate::ComGuard`] on the calling thread.
///
/// # Errors
///
/// Returns [`Error`] when the device does not exist or Windows refuses.
pub fn set_default_output(id: &DeviceId) -> Result<(), Error> {
    let wide_id: Vec<u16> = id.as_str().encode_utf16().chain([0]).collect();
    // SAFETY: standard COM instantiation of a system class; without COM
    // initialised on this thread it fails with an error.
    let policy: IPolicyConfig =
        unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
            .map_err(WasapiError::from)?;
    for role in [eConsole, eMultimedia] {
        // SAFETY: `policy` is a valid interface pointer whose declared vtable
        // matches IPolicyConfig; `wide_id` is NUL-terminated UTF-16 that
        // outlives the call.
        unsafe { policy.SetDefaultEndpoint(PCWSTR(wide_id.as_ptr()), role) }
            .ok()
            .map_err(WasapiError::from)?;
    }
    Ok(())
}
