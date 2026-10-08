//! What the window shows, derived purely from settings and engine status.

use stereo_bt_core::{ChannelMode, DeviceId};
use stereo_bt_engine::{EngineStatus, StreamState};

use crate::settings::{DeviceSettings, Settings};

/// Color category of a device badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BadgeKind {
    /// The default (source) device.
    Info,
    /// Playing in sync.
    Good,
    Neutral,
    Warning,
    Error,
}

/// A small status pill on a device card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Badge {
    pub(crate) text: &'static str,
    pub(crate) kind: BadgeKind,
}

/// Overall engine state, shown as the colored dot in the info bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusKind {
    Idle,
    Streaming,
    Waiting,
}

/// One device card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceRow {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    /// The Windows default device: the audio source, never a target.
    pub(crate) is_source: bool,
    /// Checkbox state: for the source, whether it plays (include-default mode).
    pub(crate) checked: bool,
    pub(crate) subtitle: String,
    pub(crate) badge: Option<Badge>,
    pub(crate) channel_mode: ChannelMode,
    pub(crate) delay_ms: u16,
    pub(crate) volume_percent: u8,
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
    let (subtitle, badge) = describe(device, is_source, present, settings, status);
    DeviceRow {
        id: device.id.clone(),
        name: device.name.clone(),
        is_source,
        checked: if is_source {
            settings.include_default
        } else {
            device.enabled
        },
        subtitle,
        badge,
        channel_mode: device.channel_mode,
        delay_ms: device.delay.as_millis(),
        volume_percent: device.volume.as_percent(),
    }
}

fn describe(
    device: &DeviceSettings,
    is_source: bool,
    present: bool,
    settings: &Settings,
    status: &EngineStatus,
) -> (String, Option<Badge>) {
    let (subtitle, badge) = describe_static(device, is_source, present, settings, status);
    if let Some(error) = status
        .outputs
        .iter()
        .find(|o| o.id == device.id && o.state == StreamState::Failed)
        .and_then(|o| o.error.as_deref())
    {
        return (format!("Device error · retrying — {error}"), badge);
    }
    (subtitle.into(), badge)
}

fn describe_static(
    device: &DeviceSettings,
    is_source: bool,
    present: bool,
    settings: &Settings,
    status: &EngineStatus,
) -> (&'static str, Option<Badge>) {
    if is_source {
        let subtitle = if settings.include_default {
            "Default playback · plays natively"
        } else {
            "Default playback · muted while streaming"
        };
        return (
            subtitle,
            Some(Badge {
                text: "Default",
                kind: BadgeKind::Info,
            }),
        );
    }
    if !device.enabled {
        return ("Secondary output", None);
    }
    if !present {
        return (
            "Disconnected · rejoins automatically",
            Some(Badge {
                text: "Offline",
                kind: BadgeKind::Warning,
            }),
        );
    }
    if !status.running {
        return (
            "Secondary output",
            Some(Badge {
                text: "Ready",
                kind: BadgeKind::Neutral,
            }),
        );
    }
    match status.outputs.iter().find(|o| o.id == device.id) {
        Some(o) if o.state == StreamState::Running => (
            "Secondary output",
            Some(Badge {
                text: "Synced",
                kind: BadgeKind::Good,
            }),
        ),
        Some(o) if o.state == StreamState::Failed => (
            "Device error",
            Some(Badge {
                text: "Error",
                kind: BadgeKind::Error,
            }),
        ),
        Some(_) => (
            "Secondary output",
            Some(Badge {
                text: "Starting",
                kind: BadgeKind::Neutral,
            }),
        ),
        None => (
            "Secondary output",
            Some(Badge {
                text: "Waiting",
                kind: BadgeKind::Warning,
            }),
        ),
    }
}

/// One line summarizing the engine for the info bar.
pub(crate) fn status_line(settings: &Settings, status: &EngineStatus) -> String {
    if !status.running {
        return match status.devices.len() {
            1 => "1 device — select and start.".into(),
            count => format!("{count} devices — select and start."),
        };
    }
    if status.capture == Some(StreamState::Failed) {
        return "Audio source unavailable · retrying…".into();
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

/// The info-bar dot.
pub(crate) fn status_kind(status: &EngineStatus) -> StatusKind {
    if status.capture == Some(StreamState::Failed) {
        return StatusKind::Waiting;
    }
    match (status.running, status.outputs.is_empty()) {
        (false, _) => StatusKind::Idle,
        (true, false) => StatusKind::Streaming,
        (true, true) => StatusKind::Waiting,
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
                error: (state == StreamState::Failed).then(|| "device unplugged".to_owned()),
            })
            .collect();
    }

    fn badge_text(row: &DeviceRow) -> Option<&'static str> {
        row.badge.as_ref().map(|b| b.text)
    }

    #[test]
    fn rows_follow_windows_device_order_and_mark_the_source() {
        let (settings, status) = setup(&[]);

        let rows = device_rows(&settings, &status);

        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Device A", "Device B", "Device C"]);
        assert!(rows[0].is_source);
        assert_eq!(badge_text(&rows[0]), Some("Default"));
        assert!(!rows[1].is_source);
        assert_eq!(badge_text(&rows[1]), None);
    }

    #[test]
    fn source_checkbox_shows_whether_it_plays() {
        let (mut settings, status) = setup(&[]);
        let rows = device_rows(&settings, &status);
        assert!(rows[0].checked);
        assert_eq!(rows[0].subtitle, "Default playback · plays natively");

        settings.include_default = false;

        let rows = device_rows(&settings, &status);
        assert!(!rows[0].checked);
        assert_eq!(rows[0].subtitle, "Default playback · muted while streaming");
    }

    #[test]
    fn f6_disconnected_enabled_device_is_shown_offline() {
        let (settings, mut status) = setup(&["C"]);
        status.devices.retain(|d| d.id != id("C"));

        let rows = device_rows(&settings, &status);

        assert_eq!(rows.len(), 3);
        assert_eq!(badge_text(&rows[2]), Some("Offline"));
        assert_eq!(rows[2].badge.as_ref().unwrap().kind, BadgeKind::Warning);
    }

    #[test]
    fn disconnected_disabled_device_is_hidden() {
        let (settings, mut status) = setup(&[]);
        status.devices.retain(|d| d.id != id("C"));

        assert_eq!(device_rows(&settings, &status).len(), 2);
    }

    #[test]
    fn badges_reflect_stream_state_while_running() {
        let (settings, mut status) = setup(&["B", "C"]);
        assert_eq!(
            badge_text(&device_rows(&settings, &status)[1]),
            Some("Ready")
        );
        running(
            &mut status,
            &[
                ("B", StreamState::Running, 0),
                ("C", StreamState::Failed, 0),
            ],
        );

        let rows = device_rows(&settings, &status);

        assert_eq!(badge_text(&rows[1]), Some("Synced"));
        assert_eq!(badge_text(&rows[2]), Some("Error"));
        assert_eq!(
            rows[2].subtitle,
            "Device error · retrying — device unplugged"
        );
    }

    #[test]
    fn status_line_counts_devices_when_stopped_and_summarizes_streaming() {
        let (settings, mut status) = setup(&["B"]);
        assert_eq!(
            status_line(&settings, &status),
            "3 devices — select and start."
        );
        assert_eq!(status_kind(&status), StatusKind::Idle);

        running(&mut status, &[("B", StreamState::Running, 3)]);

        assert_eq!(
            status_line(&settings, &status),
            "Streaming from Device A to 1 device · 3 dropouts"
        );
        assert_eq!(status_kind(&status), StatusKind::Streaming);
    }

    #[test]
    fn failed_capture_is_shown_as_retrying() {
        let (settings, mut status) = setup(&["B"]);
        running(&mut status, &[("B", StreamState::Running, 0)]);
        status.capture = Some(StreamState::Failed);

        assert_eq!(
            status_line(&settings, &status),
            "Audio source unavailable · retrying…"
        );
        assert_eq!(status_kind(&status), StatusKind::Waiting);
    }

    #[test]
    fn running_without_outputs_is_waiting() {
        let (_, mut status) = setup(&[]);
        running(&mut status, &[]);

        assert_eq!(status_kind(&status), StatusKind::Waiting);
    }
}
