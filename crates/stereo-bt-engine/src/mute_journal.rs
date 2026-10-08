use std::fs;
use std::io;
use std::path::PathBuf;

use stereo_bt_core::DeviceId;

/// Durable record of the devices this app muted, so a crash never leaves
/// a device muted: the next start restores them.
pub trait MuteJournal {
    /// The devices recorded as muted by us.
    ///
    /// # Errors
    ///
    /// Returns an error when the record exists but cannot be read.
    fn load(&self) -> io::Result<Vec<DeviceId>>;

    /// Replaces the record. Called *before* muting a device.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be written.
    fn save(&self, muted: &[DeviceId]) -> io::Result<()>;
}

/// A [`MuteJournal`] stored as one device identifier per line in a text file.
#[derive(Debug, Clone)]
pub struct FileMuteJournal {
    path: PathBuf,
}

impl FileMuteJournal {
    /// Uses the file at `path`; it need not exist yet.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl MuteJournal for FileMuteJournal {
    fn load(&self) -> io::Result<Vec<DeviceId>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        // Lines that are not valid identifiers cannot be restored; skip them.
        Ok(text
            .lines()
            .filter_map(|line| DeviceId::new(line).ok())
            .collect())
    }

    fn save(&self, muted: &[DeviceId]) -> io::Result<()> {
        if muted.is_empty() {
            return match fs::remove_file(&self.path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = String::new();
        for id in muted {
            text.push_str(id.as_str());
            text.push('\n');
        }
        fs::write(&self.path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(name: &str) -> FileMuteJournal {
        let dir = std::env::temp_dir().join(format!("stereo-bt-test-{}", std::process::id()));
        FileMuteJournal::new(dir.join(name))
    }

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    #[test]
    fn missing_file_means_nothing_muted() {
        assert!(journal("missing").load().unwrap().is_empty());
    }

    #[test]
    fn saved_devices_load_back_in_order() {
        let journal = journal("round-trip");

        journal.save(&[id("{a}"), id("{b}")]).unwrap();

        assert_eq!(journal.load().unwrap(), vec![id("{a}"), id("{b}")]);
        journal.save(&[]).unwrap();
    }

    #[test]
    fn saving_empty_list_removes_the_file() {
        let journal = journal("cleared");
        journal.save(&[id("{a}")]).unwrap();

        journal.save(&[]).unwrap();

        assert!(!journal.path.exists());
        assert!(journal.load().unwrap().is_empty());
    }
}
