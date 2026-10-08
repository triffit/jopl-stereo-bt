/// Errors from the Windows audio system.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A WASAPI or COM call failed.
    #[error("Windows audio error: {0}")]
    Audio(#[from] wasapi::WasapiError),
    /// Windows returned a value that failed validation.
    #[error(transparent)]
    InvalidValue(#[from] stereo_bt_core::Error),
}
