use stereo_bt_core::Space;

use crate::OutputConfig;

/// Everything the user configures for routing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineConfig {
    /// Selected output devices and their settings.
    pub outputs: Vec<OutputConfig>,
    /// "Include default device in sync set": `true` lets the default device
    /// play natively; `false` (default) mutes it while streaming.
    pub include_default: bool,
    /// Scale every output by the Windows volume (and mute) of the default
    /// device, so all devices get louder and quieter together.
    pub follow_system_volume: bool,
    /// Stereo width and reverb for every output.
    pub space: Space,
}
