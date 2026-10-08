use stereo_bt_core::{ChannelMode, Delay, DeviceId, Volume};

/// User settings for one output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfig {
    /// Device to play on.
    pub id: DeviceId,
    /// Sync delay (F3).
    pub delay: Delay,
    /// Device volume (F4).
    pub volume: Volume,
    /// Part of the stereo signal to play (F2).
    pub channel_mode: ChannelMode,
}

impl OutputConfig {
    /// Settings for `id` with no delay, full volume and stereo.
    #[must_use]
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            delay: Delay::ZERO,
            volume: Volume::FULL,
            channel_mode: ChannelMode::Stereo,
        }
    }
}
