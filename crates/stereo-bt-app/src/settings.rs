//! The user's saved choices (PRODUCT.md F8): per-device settings keyed by
//! endpoint identifier, remembered across sessions and reconnects.

use serde::{Deserialize, Serialize};
use stereo_bt_core::{BassCut, ChannelMode, Delay, DeviceId, DeviceInfo, VirtualBass, Volume};
use stereo_bt_engine::{EngineConfig, OutputConfig};

/// Settings for one device, remembered even while it is disconnected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct DeviceSettings {
    pub(crate) id: DeviceId,
    /// Last known name, shown while the device is disconnected.
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) delay: Delay,
    pub(crate) volume: Volume,
    pub(crate) channel_mode: ChannelMode,
    /// High-pass cutoff for small speakers (F15); missing in older files.
    #[serde(default)]
    pub(crate) bass_cut: BassCut,
    #[serde(default)]
    pub(crate) virtual_bass: VirtualBass,
    /// The device's own volume while following the system volume.
    #[serde(default)]
    pub(crate) max_volume: Volume,
}

impl DeviceSettings {
    fn new(info: &DeviceInfo) -> Self {
        Self {
            id: info.id.clone(),
            name: info.name.clone(),
            enabled: false,
            delay: Delay::ZERO,
            volume: Volume::FULL,
            channel_mode: ChannelMode::Stereo,
            bass_cut: BassCut::OFF,
            virtual_bass: VirtualBass::OFF,
            max_volume: Volume::FULL,
        }
    }
}

/// Everything persisted between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub(crate) struct Settings {
    /// Whether the Windows default device keeps playing while streaming
    /// (`false` mutes it, which needs a spare output device as the source).
    pub(crate) include_default: bool,
    /// Whether every device follows the Windows volume of the default device.
    pub(crate) follow_system_volume: bool,
    /// Whether closing the window while playing keeps playing in the tray.
    pub(crate) keep_streaming_on_close: bool,
    pub(crate) devices: Vec<DeviceSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            include_default: true,
            follow_system_volume: true,
            keep_streaming_on_close: true,
            devices: Vec::new(),
        }
    }
}

impl Settings {
    /// Adds settings for newly seen devices and refreshes known names.
    /// Returns whether anything changed.
    pub(crate) fn learn(&mut self, devices: &[DeviceInfo]) -> bool {
        let mut changed = false;
        for info in devices {
            match self.devices.iter_mut().find(|d| d.id == info.id) {
                Some(known) if known.name != info.name => {
                    known.name.clone_from(&info.name);
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.devices.push(DeviceSettings::new(info));
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn device(&self, id: &DeviceId) -> Option<&DeviceSettings> {
        self.devices.iter().find(|d| &d.id == id)
    }

    pub(crate) fn device_mut(&mut self, id: &DeviceId) -> Option<&mut DeviceSettings> {
        self.devices.iter_mut().find(|d| &d.id == id)
    }

    /// The engine configuration: every enabled device, in settings order.
    pub(crate) fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            outputs: self
                .devices
                .iter()
                .filter(|d| d.enabled)
                .map(|d| OutputConfig {
                    id: d.id.clone(),
                    delay: d.delay,
                    volume: d.volume,
                    channel_mode: d.channel_mode,
                    bass_cut: d.bass_cut,
                    virtual_bass: d.virtual_bass,
                    max_volume: d.max_volume,
                })
                .collect(),
            include_default: self.include_default,
            follow_system_volume: self.follow_system_volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(raw: &str, name: &str) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId::new(raw).unwrap(),
            name: name.into(),
            is_default: false,
        }
    }

    #[test]
    fn f8_new_devices_are_learned_disabled_with_neutral_settings() {
        let mut settings = Settings::default();

        assert!(settings.learn(&[info("a", "Speakers")]));

        let device = &settings.devices[0];
        assert!(!device.enabled);
        assert_eq!(device.delay, Delay::ZERO);
        assert_eq!(device.volume, Volume::FULL);
        assert_eq!(device.channel_mode, ChannelMode::Stereo);
    }

    #[test]
    fn f8_known_devices_keep_their_settings_and_get_renamed() {
        let mut settings = Settings::default();
        settings.learn(&[info("a", "Old name")]);
        settings.devices[0].enabled = true;

        assert!(settings.learn(&[info("a", "New name")]));
        assert!(!settings.learn(&[info("a", "New name")]));

        assert!(settings.devices[0].enabled);
        assert_eq!(settings.devices[0].name, "New name");
    }

    #[test]
    fn engine_config_contains_only_enabled_devices() {
        let mut settings = Settings::default();
        settings.learn(&[info("a", "A"), info("b", "B")]);
        settings.devices[1].enabled = true;
        settings.devices[1].delay = Delay::from_millis(42).unwrap();
        settings.include_default = true;

        let config = settings.engine_config();

        assert_eq!(config.outputs.len(), 1);
        assert_eq!(config.outputs[0].id.as_str(), "b");
        assert_eq!(config.outputs[0].delay.as_millis(), 42);
        assert!(config.include_default);
    }

    #[test]
    fn f15_device_files_without_sound_settings_load_with_them_off() {
        let old = "[[devices]]
id = \"a\"
name = \"A\"
enabled = true
                   delay = 0
volume = 80
channel-mode = \"stereo\"
";

        let settings: Settings = toml::from_str(old).unwrap();

        let device = &settings.devices[0];
        assert_eq!(device.bass_cut, BassCut::OFF);
        assert_eq!(device.virtual_bass, VirtualBass::OFF);
        assert_eq!(device.max_volume, Volume::FULL);
    }

    #[test]
    fn f15_sound_settings_reach_the_engine() {
        let mut settings = Settings::default();
        settings.learn(&[info("a", "A")]);
        let device = &mut settings.devices[0];
        device.enabled = true;
        device.bass_cut = BassCut::from_hz(150).unwrap();
        device.virtual_bass = VirtualBass::from_percent(40).unwrap();
        device.max_volume = Volume::from_percent(70).unwrap();

        let output = &settings.engine_config().outputs[0];

        assert_eq!(output.bass_cut.hz(), Some(150));
        assert_eq!(output.virtual_bass.as_percent(), 40);
        assert_eq!(output.max_volume.as_percent(), 70);
    }

    #[test]
    fn f8_settings_round_trip_through_toml() {
        let mut settings = Settings::default();
        settings.learn(&[info("{0.0.0.00000000}.{x}", "Headphones")]);
        settings.devices[0].channel_mode = ChannelMode::RightOnly;

        let text = toml::to_string(&settings).unwrap();

        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), settings);
    }

    #[test]
    fn default_device_keeps_playing_by_default() {
        assert!(Settings::default().include_default);
        assert!(Settings::default().engine_config().include_default);
    }

    #[test]
    fn devices_follow_the_system_volume_by_default() {
        assert!(Settings::default().follow_system_volume);
        assert!(Settings::default().engine_config().follow_system_volume);
    }

    #[test]
    fn settings_saved_before_this_option_existed_follow_by_default() {
        let old = "include-default = false\n";

        let settings: Settings = toml::from_str(old).unwrap();

        assert!(settings.follow_system_volume);
        assert!(settings.keep_streaming_on_close);
        assert!(!settings.include_default);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }
}
