use crate::DeviceId;

/// An active audio output device as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable endpoint identifier.
    pub id: DeviceId,
    /// Human-readable name, e.g. "Headphones (WH-1000XM4)".
    pub name: String,
    /// Whether this is the current Windows default output device.
    pub is_default: bool,
}
