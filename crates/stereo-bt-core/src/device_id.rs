use std::fmt;

use crate::Error;

/// A validated, opaque audio endpoint identifier (the Windows endpoint ID string).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    /// Longest accepted identifier, in bytes.
    pub const MAX_LEN: usize = 512;

    /// Creates a device identifier.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDeviceId`] when `raw` is empty, longer than
    /// [`Self::MAX_LEN`] bytes, or contains control characters.
    pub fn new(raw: impl Into<String>) -> Result<Self, Error> {
        let raw = raw.into();
        let valid =
            !raw.is_empty() && raw.len() <= Self::MAX_LEN && !raw.chars().any(char::is_control);
        if valid {
            Ok(Self(raw))
        } else {
            Err(Error::InvalidDeviceId)
        }
    }

    /// The identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
