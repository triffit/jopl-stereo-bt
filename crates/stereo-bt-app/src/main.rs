//! `JOPL-StereoBT`: play system audio on several devices at once, in sync.
//!
//! Binary edge: wires the engine, settings store and window together.
//! All logic lives in `controller` and the engine crates.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assistant;
mod controller;
// Used by build.rs to embed the exe icon; compiled here only for its tests.
#[cfg(test)]
mod ico;
#[cfg(windows)]
mod icon;
mod settings;
mod settings_store;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod ui;
mod view_model;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use stereo_bt_engine::{Engine, FileRestoreJournal};

    use crate::settings_store::FileSettingsStore;

    let data_dir = std::env::var_os("APPDATA")
        .map(|dir| std::path::PathBuf::from(dir).join("JOPL-StereoBT"))
        .context("the APPDATA environment variable is not set")?;

    match Engine::spawn(FileRestoreJournal::in_dir(&data_dir)) {
        Ok(engine) => {
            ui::run_main(
                engine,
                FileSettingsStore::new(data_dir.join("settings.toml")),
            )?;
        }
        Err(error) => ui::show_error(&format!(
            "The Windows audio system could not be initialised: {error}"
        ))?,
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("JOPL-StereoBT runs on Windows only")
}
