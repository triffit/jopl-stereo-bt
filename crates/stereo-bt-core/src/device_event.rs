use crate::DeviceId;

/// A change in the set of audio output devices (PRODUCT.md F6, hot-plug).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// A device became active (connected, enabled or plugged in).
    Activated(DeviceId),
    /// A device stopped being active (disconnected, disabled, unplugged or removed).
    Deactivated(DeviceId),
    /// The Windows default output device changed; `None` when there is none.
    DefaultChanged(Option<DeviceId>),
}
