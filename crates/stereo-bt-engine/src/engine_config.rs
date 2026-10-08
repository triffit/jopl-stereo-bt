use crate::OutputConfig;

/// Everything the user configures for routing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineConfig {
    /// Selected output devices and their settings.
    pub outputs: Vec<OutputConfig>,
    /// "Include default device in sync set": `true` lets the default device
    /// play natively; `false` (default) mutes it while streaming.
    pub include_default: bool,
}
