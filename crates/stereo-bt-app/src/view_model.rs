//! What the window shows, derived purely from settings and engine status.

use stereo_bt_core::{ChannelMode, DeviceId};
use stereo_bt_engine::{EngineStatus, StreamState};

use crate::settings::{DeviceSettings, Settings};

/// Color category of a device's health text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Neutral,
    Good,
    Warning,
    Error,
}

/// One device card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceRow {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    /// The Windows default device: the audio source, never a target.
    pub(crate) is_source: bool,
    pub(crate) enabled: bool,
    pub(crate) channel_mode: ChannelMode,
    pub(crate) delay_ms: u16,
    pub(crate) volume_percent: u8,
    pub(crate) health_text: String,
    pub(crate) health: Health,
}

/// Present devices in Windows order, then remembered enabled devices that
/// are disconnected (they rejoin automatically when they return).
pub(crate) fn device_rows(settings: &Settings, status: &EngineStatus) -> Vec<DeviceRow> {
    let mut rows: Vec<DeviceRow> = status
        .devices
        .iter()
        .filter_map(|info| {
            let device = settings.device(&info.id)?;
            Some(row(device, info.is_default, true, settings, status))
        })
        .collect();
    rows.extend(
        settings
            .devices
            .iter()
            .filter(|d| d.enabled && !status.devices.iter().any(|info| info.id == d.id))
            .map(|d| row(d, false, false, settings, status)),
    );
    rows
}

fn row(
    device: &DeviceSettings,
    is_source: bool,
    present: bool,
    settings: &Settings,
    status: &EngineStatus,
) -> DeviceRow {
    let (health_text, health) = health(device, is_source, present, settings, status);
    DeviceRow {
        id: device.id.clone(),
        name: device.name.clone(),
        is_source,
        enabled: device.enabled,
        channel_mode: device.channel_mode,
        delay_ms: device.delay.as_millis(),
        volume_percent: device.volume.as_percent(),
        health_text: health_text.into(),
        health,
    }
}

fn health(
    device: &DeviceSettings,
    is_source: bool,
    present: bool,
    settings: &Settings,
    status: &EngineStatus,
) -> (&'static str, Health) {
    if is_source {
        return match (settings.include_default, status.running) {
            (true, _) => ("Source · plays natively", Health::Neutral),
            (false, true) => ("Source · muted while streaming", Health::Neutral),
            (false, false) => ("Source · will be muted while streaming", Health::Neutral),
        };
    }
    if !device.enabled {
        return ("Off", Health::Neutral);
    }
    if !present {
        return ("Disconnected", Health::Warning);
    }
    if !status.running {
        return ("Ready", Health::Neutral);
    }
    match status.outputs.iter().find(|o| o.id == device.id) {
        Some(o) if o.state == StreamState::Running => ("Playing", Health::Good),
        Some(o) if o.state == StreamState::Failed => ("Device error", Health::Error),
        Some(_) => ("Starting…", Health::Neutral),
        None => ("Waiting", Health::Warning),
    }
}

/// One line summarizing the engine for the status bar.
pub(crate) fn status_line(settings: &Settings, status: &EngineStatus) -> String {
    if !status.running {
        return "Stopped".into();
    }
    let Some(source) = &status.source else {
        return "Waiting for devices…".into();
    };
    let source_name = settings
        .device(source)
        .map_or_else(|| source.to_string(), |d| d.name.clone());
    let playing = status
        .outputs
        .iter()
        .filter(|o| o.state == StreamState::Running)
        .count();
    let dropouts: u64 = status.outputs.iter().map(|o| o.underruns).sum();
    let devices = if playing == 1 { "device" } else { "devices" };
    let line = format!("Streaming from {source_name} to {playing} {devices}");
    if dropouts > 0 {
        format!("{line} · {dropouts} dropouts")
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use stereo_bt_core::DeviceInfo;
    use stereo_bt_engine::OutputStatus;

    use super::*;

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    fn info(raw: &str, is_default: bool) -> DeviceInfo {
        DeviceInfo {
            id: id(raw),
            name: format!("Device {raw}"),
            is_default,
        }
    }

    fn setup(enabled: &[&str]) -> (Settings, EngineStatus) {
        let status = EngineStatus {
            devices: vec![info("A", true), info("B", false), info("C", false)],
            ..EngineStatus::default()
        };
        let mut settings = Settings::default();
        settings.learn(&status.devices);
        for raw in enabled {
            settings.device_mut(&id(raw)).unwrap().enabled = true;
        }
        (settings, status)
    }

    fn running(status: &mut EngineStatus, outputs: &[(&str, StreamState, u64)]) {
        status.running = true;
        status.source = Some(id("A"));
        status.outputs = outputs
            .iter()
            .map(|&(raw, state, underruns)| OutputStatus {
                id: id(raw),
                state,
                underruns,
                overruns: 0,
                trims: 0,
            })
            .collect();
    }

    #[test]
    fn rows_follow_windows_device_order_and_mark_the_source() {
        let (settings, status) = setup(&[]);

        let rows = device_rows(&settings, &status);

        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Device A", "Device B", "Device C"]);
        assert!(rows[0].is_source);
        assert!(!rows[1].is_source);
    }

    #[test]
    fn f6_disconnected_enabled_device_is_shown_as_disconnected() {
        let (settings, mut status) = setup(&["C"]);
        status.devices.retain(|d| d.id != id("C"));

        let rows = device_rows(&settings, &status);

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].health_text, "Disconnected");
        assert_eq!(rows[2].health, Health::Warning);
    }

    #[test]
    fn disconnected_disabled_device_is_hidden() {
        let (settings, mut status) = setup(&[]);
        status.devices.retain(|d| d.id != id("C"));

        assert_eq!(device_rows(&settings, &status).len(), 2);
    }

    #[test]
    fn health_reflects_stream_state_while_running() {
        let (settings, mut status) = setup(&["B", "C"]);
        running(
            &mut status,
            &[
                ("B", StreamState::Running, 0),
                ("C", StreamState::Failed, 0),
            ],
        );

        let rows = device_rows(&settings, &status);

        assert_eq!(rows[1].health_text, "Playing");
        assert_eq!(rows[1].health, Health::Good);
        assert_eq!(rows[2].health_text, "Device error");
        assert_eq!(rows[2].health, Health::Error);
    }

    #[test]
    fn source_text_depends_on_include_default() {
        let (mut settings, mut status) = setup(&[]);
        running(&mut status, &[]);
        assert_eq!(
            device_rows(&settings, &status)[0].health_text,
            "Source · muted while streaming"
        );

        settings.include_default = true;

        assert_eq!(
            device_rows(&settings, &status)[0].health_text,
            "Source · plays natively"
        );
    }

    #[test]
    fn status_line_summarizes_streaming_and_dropouts() {
        let (settings, mut status) = setup(&["B"]);
        assert_eq!(status_line(&settings, &status), "Stopped");

        running(&mut status, &[("B", StreamState::Running, 3)]);

        assert_eq!(
            status_line(&settings, &status),
            "Streaming from Device A to 1 device · 3 dropouts"
        );
    }
}
