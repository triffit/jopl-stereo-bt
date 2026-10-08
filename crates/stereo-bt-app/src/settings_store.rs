//! Loading and saving [`Settings`] as TOML.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::settings::Settings;

/// Where settings are persisted; abstracted for tests.
pub(crate) trait SettingsStore {
    /// Loads saved settings; a missing file yields defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but cannot be read or parsed.
    fn load(&self) -> io::Result<Settings>;

    /// Saves settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    fn save(&self, settings: &Settings) -> io::Result<()>;
}

/// Settings in a TOML file, written atomically (temp file + rename).
pub(crate) struct FileSettingsStore {
    path: PathBuf,
}

impl FileSettingsStore {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl SettingsStore for FileSettingsStore {
    fn load(&self) -> io::Result<Settings> {
        match fs::read_to_string(&self.path) {
            Ok(text) => toml::from_str(&text).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error),
        }
    }

    fn save(&self, settings: &Settings) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(settings).map_err(io::Error::other)?;
        let temp = self.path.with_extension("toml.tmp");
        fs::write(&temp, text)?;
        fs::rename(&temp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use stereo_bt_core::{DeviceId, DeviceInfo};

    use super::*;

    fn store(name: &str) -> FileSettingsStore {
        let dir = std::env::temp_dir().join(format!("stereo-bt-app-test-{}", std::process::id()));
        FileSettingsStore::new(dir.join(name))
    }

    #[test]
    fn missing_file_loads_defaults() {
        assert_eq!(store("missing.toml").load().unwrap(), Settings::default());
    }

    #[test]
    fn saved_settings_load_back() {
        let store = store("round-trip.toml");
        let mut settings = Settings::default();
        settings.learn(&[DeviceInfo {
            id: DeviceId::new("a").unwrap(),
            name: "A".into(),
            is_default: false,
        }]);

        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), settings);
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_panic() {
        let store = store("corrupt.toml");
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        fs::write(&store.path, "devices = 12").unwrap();

        assert!(store.load().is_err());
    }
}
