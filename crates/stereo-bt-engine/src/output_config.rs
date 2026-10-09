use stereo_bt_core::{BassCut, ChannelMode, Delay, DeviceId, SpeakerPosition, VirtualBass, Volume};

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
    /// High-pass cutoff for small speakers (F15).
    pub bass_cut: BassCut,
    /// Harmonics that make cut bass audible again (F15).
    pub virtual_bass: VirtualBass,
    /// Where the speaker stands in a surround setup (F20).
    pub position: SpeakerPosition,
}

impl OutputConfig {
    /// Settings for `id` with no delay, full volume, stereo and no bass
    /// shaping.
    #[must_use]
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            delay: Delay::ZERO,
            volume: Volume::FULL,
            channel_mode: ChannelMode::Stereo,
            bass_cut: BassCut::OFF,
            virtual_bass: VirtualBass::OFF,
            position: SpeakerPosition::Front,
        }
    }
}
