/// Errors from the Windows audio system.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A WASAPI or COM call failed.
    #[error("{}", describe(.0))]
    Audio(#[from] wasapi::WasapiError),
    /// No microphone suitable for measuring latency (Bluetooth hands-free
    /// microphones are skipped on purpose).
    #[error("no suitable microphone found")]
    NoMicrophone,
    /// Windows returned a value that failed validation.
    #[error(transparent)]
    InvalidValue(#[from] stereo_bt_core::Error),
}

/// A readable explanation of a WASAPI error, with the Windows code kept for
/// support; errors without a known explanation keep their raw text.
fn describe(error: &wasapi::WasapiError) -> String {
    let code = match error {
        wasapi::WasapiError::Windows(e) | wasapi::WasapiError::RegisterNotifications(e) => {
            e.code().0
        }
        _ => return format!("Windows audio error: {error}"),
    };
    match explain(code) {
        Some(text) => format!("{text} ({})", hex(code)),
        None => format!("Windows audio error: {error}"),
    }
}

/// What a Windows audio error code means for the user, if it is a known one.
fn explain(code: i32) -> Option<&'static str> {
    // HRESULTs are conventionally written as unsigned hex.
    #[allow(clippy::cast_sign_loss)]
    let text = match code as u32 {
        // AUDCLNT_E_DEVICE_INVALIDATED
        0x8889_0004 => "The device was disconnected or changed.",
        // AUDCLNT_E_UNSUPPORTED_FORMAT
        0x8889_0008 => "The device does not accept 48 kHz stereo audio.",
        // AUDCLNT_E_DEVICE_IN_USE
        0x8889_000A => {
            "Another program is using this device exclusively. Close that program, or untick \
             \"Allow applications to take exclusive control of this device\" in the Windows \
             sound settings (device properties, Advanced)."
        }
        // AUDCLNT_E_ENDPOINT_CREATE_FAILED
        0x8889_000F => {
            "Windows could not open this device. A Bluetooth adapter often plays to only one \
             device at a time: turn off another Bluetooth audio device, or connect this one \
             with a USB Bluetooth audio transmitter or a cable."
        }
        // AUDCLNT_E_SERVICE_NOT_RUNNING
        0x8889_0010 => "The Windows Audio service is not running.",
        // AUDCLNT_E_RESOURCES_INVALIDATED
        0x8889_0026 => "Windows reconfigured the audio device.",
        // HRESULT_FROM_WIN32(ERROR_NOT_FOUND)
        0x8007_0490 => "The device is not available; it may be disconnected or disabled.",
        _ => return None,
    };
    Some(text)
}

fn hex(code: i32) -> String {
    // HRESULTs are conventionally written as unsigned hex.
    #[allow(clippy::cast_sign_loss)]
    let unsigned = code as u32;
    format!("0x{unsigned:08X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_error(code: u32) -> Error {
        #[allow(clippy::cast_possible_wrap)]
        let hresult = windows_core::HRESULT(code as i32);
        Error::Audio(wasapi::WasapiError::Windows(windows_core::Error::from(
            hresult,
        )))
    }

    #[test]
    fn device_in_use_explains_exclusive_mode() {
        let text = windows_error(0x8889_000A).to_string();

        assert!(text.starts_with("Another program is using this device exclusively."));
        assert!(text.ends_with("(0x8889000A)"));
    }

    #[test]
    fn endpoint_create_failed_explains_the_bluetooth_limit() {
        let text = windows_error(0x8889_000F).to_string();

        assert!(text.starts_with("Windows could not open this device."));
        assert!(text.contains("only one device at a time"));
        assert!(text.ends_with("(0x8889000F)"));
    }

    #[test]
    fn unknown_codes_keep_the_raw_windows_text() {
        let text = windows_error(0x8889_0007).to_string();

        assert!(text.starts_with("Windows audio error:"), "{text}");
    }

    #[test]
    fn errors_without_a_code_keep_their_text() {
        let text = Error::Audio(wasapi::WasapiError::EventTimeout).to_string();

        assert_eq!(text, "Windows audio error: Handle wait timed out");
    }
}
