use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use stereo_bt_core::DeviceId;

/// Durable record of every Windows setting this app changed (device mutes
/// and device volumes), so a crash never leaves them changed: the next start
/// restores them. Each change is recorded *before* it is made.
pub trait RestoreJournal {
    /// Devices recorded as muted by us.
    ///
    /// # Errors
    ///
    /// Returns an error when the record exists but cannot be read.
    fn load_mutes(&self) -> io::Result<Vec<DeviceId>>;

    /// Replaces the record of devices muted by us.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be written.
    fn save_mutes(&self, muted: &[DeviceId]) -> io::Result<()>;

    /// Devices whose volume we changed, with their original volume (slider
    /// position, `0.0..=1.0`).
    ///
    /// # Errors
    ///
    /// Returns an error when the record exists but cannot be read.
    fn load_volumes(&self) -> io::Result<Vec<(DeviceId, f32)>>;

    /// Replaces the record of changed volumes.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be written.
    fn save_volumes(&self, volumes: &[(DeviceId, f32)]) -> io::Result<()>;
}

/// A [`RestoreJournal`] stored as two small text files in one folder.
#[derive(Debug, Clone)]
pub struct FileRestoreJournal {
    mutes: PathBuf,
    volumes: PathBuf,
}

impl FileRestoreJournal {
    /// Uses `mute-journal.txt` and `volume-journal.txt` in `dir`, which need
    /// not exist yet.
    #[must_use]
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            mutes: dir.as_ref().join("mute-journal.txt"),
            volumes: dir.as_ref().join("volume-journal.txt"),
        }
    }
}

impl RestoreJournal for FileRestoreJournal {
    fn load_mutes(&self) -> io::Result<Vec<DeviceId>> {
        // Lines that are not valid identifiers cannot be restored; skip them.
        Ok(read_lines(&self.mutes)?
            .iter()
            .filter_map(|line| DeviceId::new(line.as_str()).ok())
            .collect())
    }

    fn save_mutes(&self, muted: &[DeviceId]) -> io::Result<()> {
        write_lines(&self.mutes, muted.iter().map(DeviceId::to_string))
    }

    fn load_volumes(&self) -> io::Result<Vec<(DeviceId, f32)>> {
        Ok(read_lines(&self.volumes)?
            .iter()
            .filter_map(|line| {
                let (volume, id) = line.split_once(' ')?;
                let volume: f32 = volume.parse().ok()?;
                (0.0..=1.0)
                    .contains(&volume)
                    .then_some(())
                    .and_then(|()| DeviceId::new(id).ok())
                    .map(|id| (id, volume))
            })
            .collect())
    }

    fn save_volumes(&self, volumes: &[(DeviceId, f32)]) -> io::Result<()> {
        write_lines(
            &self.volumes,
            volumes.iter().map(|(id, volume)| format!("{volume} {id}")),
        )
    }
}

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    // Lossy: unreadable bytes only spoil their own line, which is then skipped,
    // instead of keeping the engine from starting.
    match fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .map(str::to_owned)
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Writes one entry per line, atomically (temp file + rename), so a crash
/// mid-write never leaves a half-written journal; an empty list removes the file.
fn write_lines(path: &Path, lines: impl Iterator<Item = String>) -> io::Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    if text.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(name: &str) -> FileRestoreJournal {
        let dir = std::env::temp_dir()
            .join(format!("stereo-bt-test-{}", std::process::id()))
            .join(name);
        FileRestoreJournal::in_dir(dir)
    }

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    #[test]
    fn missing_files_mean_nothing_changed() {
        let journal = journal("missing");

        assert!(journal.load_mutes().unwrap().is_empty());
        assert!(journal.load_volumes().unwrap().is_empty());
    }

    #[test]
    fn saved_mutes_load_back_in_order() {
        let journal = journal("mutes");

        journal.save_mutes(&[id("{a}"), id("{b}")]).unwrap();

        assert_eq!(journal.load_mutes().unwrap(), vec![id("{a}"), id("{b}")]);
        journal.save_mutes(&[]).unwrap();
    }

    #[test]
    fn saved_volumes_load_back_with_their_original_level() {
        let journal = journal("volumes");

        journal
            .save_volumes(&[(id("{a}"), 0.1), (id("{b}"), 0.44)])
            .unwrap();

        assert_eq!(
            journal.load_volumes().unwrap(),
            vec![(id("{a}"), 0.1), (id("{b}"), 0.44)]
        );
        journal.save_volumes(&[]).unwrap();
    }

    #[test]
    fn unreadable_bytes_only_skip_their_own_line() {
        let journal = journal("bytes");
        fs::create_dir_all(journal.mutes.parent().unwrap()).unwrap();
        // Two bytes that are never valid UTF-8 (0xFF, 0xFE) on the middle line.
        let mut bytes = b"{a}\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE]);
        bytes.extend_from_slice(b" garbage\n{b}\n");
        fs::write(&journal.mutes, bytes).unwrap();

        let loaded = journal.load_mutes().unwrap();

        assert!(loaded.contains(&id("{a}")) && loaded.contains(&id("{b}")));
        journal.save_mutes(&[]).unwrap();
    }

    #[test]
    fn saving_replaces_the_journal_and_leaves_no_temp_file() {
        let journal = journal("replace");
        journal.save_mutes(&[id("{a}"), id("{b}")]).unwrap();

        journal.save_mutes(&[id("{c}")]).unwrap();

        assert_eq!(journal.load_mutes().unwrap(), vec![id("{c}")]);
        assert!(!journal.mutes.with_extension("tmp").exists());
        journal.save_mutes(&[]).unwrap();
    }

    #[test]
    fn saving_empty_lists_removes_the_files() {
        let journal = journal("cleared");
        journal.save_mutes(&[id("{a}")]).unwrap();
        journal.save_volumes(&[(id("{a}"), 0.5)]).unwrap();

        journal.save_mutes(&[]).unwrap();
        journal.save_volumes(&[]).unwrap();

        assert!(!journal.mutes.exists());
        assert!(!journal.volumes.exists());
    }

    #[test]
    fn corrupt_volume_lines_are_skipped() {
        let journal = journal("corrupt");
        fs::create_dir_all(journal.volumes.parent().unwrap()).unwrap();
        fs::write(&journal.volumes, "abc {a}\n7.5 {b}\n0.3 {c}\nnospace\n").unwrap();

        assert_eq!(journal.load_volumes().unwrap(), vec![(id("{c}"), 0.3)]);
        journal.save_volumes(&[]).unwrap();
    }
}
