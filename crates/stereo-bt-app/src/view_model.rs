//! What the window shows, derived purely from settings, engine status and
//! the last microphone measurement.

use stereo_bt_core::{ChannelMode, DeviceId, DeviceLatency};
use stereo_bt_engine::{EngineStatus, StreamState};

use crate::settings::{DeviceSettings, Settings};

/// Devices this many milliseconds apart sound together.
const IN_SYNC_MS: f64 = 2.0;

/// Color of a status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Neutral,
    Good,
    Caution,
    Error,
}

/// Overall engine state, shown in the info bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusKind {
    Idle,
    Streaming,
    Waiting,
}

/// Each device's own latency from the last microphone measurement, so its
/// timing can be shown for any delay.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Measurement {
    pub(crate) microphone: String,
    /// The default device at measurement time; timing is relative to it.
    pub(crate) source: Option<DeviceId>,
    /// Latency without our delay, per device.
    pub(crate) own_ms: Vec<(DeviceId, f64)>,
}

impl Measurement {
    pub(crate) fn from_latencies(microphone: &str, measured: &[DeviceLatency]) -> Self {
        Self {
            microphone: microphone.to_owned(),
            source: measured.iter().find(|m| m.is_source).map(|m| m.id.clone()),
            own_ms: measured
                .iter()
                .map(|m| (m.id.clone(), m.measured_ms - f64::from(m.delay.as_millis())))
                .collect(),
        }
    }

    fn own(&self, id: &DeviceId) -> Option<f64> {
        self.own_ms
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, ms)| *ms)
    }
}

/// One device card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceRow {
    pub(crate) id: DeviceId,
    pub(crate) name: String,
    /// The Windows default device: the audio source, never a target.
    pub(crate) is_source: bool,
    /// For the source: keeps playing; for others: plays.
    pub(crate) checked: bool,
    /// Connected and not already the default, so it can be made the default.
    pub(crate) can_make_default: bool,
    pub(crate) status: String,
    pub(crate) tone: Tone,
    pub(crate) channel_mode: ChannelMode,
    pub(crate) delay_ms: u16,
    pub(crate) volume_percent: u8,
}

/// One lane of the timing overview: when a device is heard.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lane {
    pub(crate) name: String,
    pub(crate) own_ms: f64,
    pub(crate) delay_ms: u16,
    pub(crate) plays_ms: f64,
    pub(crate) is_source: bool,
    pub(crate) in_sync: bool,
}

/// The timing reference: the default device's own latency, when the last
/// measurement was taken with the current default device.
fn reference_ms(measurement: Option<&Measurement>, status: &EngineStatus) -> Option<f64> {
    let measurement = measurement?;
    let current = status.devices.iter().find(|d| d.is_default)?;
    if measurement.source.as_ref() != Some(&current.id) {
        return None;
    }
    measurement.own(&current.id)
}

/// Present devices in Windows order, then remembered enabled devices that
/// are disconnected (they rejoin automatically when they return).
pub(crate) fn device_rows(
    settings: &Settings,
    status: &EngineStatus,
    measurement: Option<&Measurement>,
) -> Vec<DeviceRow> {
    let reference = reference_ms(measurement, status);
    let mut rows: Vec<DeviceRow> = status
        .devices
        .iter()
        .filter_map(|info| {
            let device = settings.device(&info.id)?;
            Some(row(
                device,
                info.is_default,
                true,
                settings,
                status,
                measurement,
                reference,
            ))
        })
        .collect();
    rows.extend(
        settings
            .devices
            .iter()
            .filter(|d| d.enabled && !status.devices.iter().any(|info| info.id == d.id))
            .map(|d| row(d, false, false, settings, status, measurement, reference)),
    );
    rows
}

fn row(
    device: &DeviceSettings,
    is_source: bool,
    present: bool,
    settings: &Settings,
    status: &EngineStatus,
    measurement: Option<&Measurement>,
    reference: Option<f64>,
) -> DeviceRow {
    let (status_text, tone) = describe(
        device,
        is_source,
        present,
        settings,
        status,
        measurement,
        reference,
    );
    DeviceRow {
        id: device.id.clone(),
        name: device.name.clone(),
        is_source,
        checked: if is_source {
            settings.include_default
        } else {
            device.enabled
        },
        can_make_default: present && !is_source,
        status: status_text,
        tone,
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
    measurement: Option<&Measurement>,
    reference: Option<f64>,
) -> (String, Tone) {
    if is_source {
        let text = if settings.include_default {
            "The others are timed to this device"
        } else {
            "Muted while playing; it only feeds the other devices"
        };
        return (text.into(), Tone::Good);
    }
    if !device.enabled {
        return ("Off".into(), Tone::Neutral);
    }
    if !present {
        return (
            "Disconnected; plays again when it reconnects".into(),
            Tone::Caution,
        );
    }
    if let Some(output) = status.outputs.iter().find(|o| o.id == device.id) {
        match output.state {
            StreamState::Failed => {
                let error = output.error.as_deref().unwrap_or("unknown error");
                return (format!("Device error, retrying: {error}"), Tone::Error);
            }
            StreamState::Starting => return ("Starting…".into(), Tone::Neutral),
            StreamState::Running | StreamState::Stopped => {}
        }
    }
    sync_status(device, measurement, reference)
}

fn sync_status(
    device: &DeviceSettings,
    measurement: Option<&Measurement>,
    reference: Option<f64>,
) -> (String, Tone) {
    let (Some(own), Some(reference)) = (measurement.and_then(|m| m.own(&device.id)), reference)
    else {
        return ("Not measured yet".into(), Tone::Neutral);
    };
    let off = own + f64::from(device.delay.as_millis()) - reference;
    // Rounded to whole milliseconds for display.
    #[allow(clippy::cast_possible_truncation)]
    let ms = off.abs().round() as i64;
    if off.abs() <= IN_SYNC_MS {
        ("In sync".into(), Tone::Good)
    } else if off < 0.0 {
        (format!("Plays {ms} ms early"), Tone::Caution)
    } else {
        (format!("Plays {ms} ms late"), Tone::Caution)
    }
}

/// The timing overview: the default device, then every playing device that
/// was measured. Empty when nothing usable was measured.
pub(crate) fn lanes(
    settings: &Settings,
    status: &EngineStatus,
    measurement: Option<&Measurement>,
) -> Vec<Lane> {
    let (Some(measurement), Some(reference)) = (measurement, reference_ms(measurement, status))
    else {
        return Vec::new();
    };
    status
        .devices
        .iter()
        .filter_map(|info| {
            let device = settings.device(&info.id)?;
            if !info.is_default && !device.enabled {
                return None;
            }
            let own = measurement.own(&info.id)?;
            let delay_ms = if info.is_default {
                0
            } else {
                device.delay.as_millis()
            };
            let plays_ms = own + f64::from(delay_ms);
            Some(Lane {
                name: device.name.clone(),
                own_ms: own,
                delay_ms,
                plays_ms,
                is_source: info.is_default,
                in_sync: (plays_ms - reference).abs() <= IN_SYNC_MS,
            })
        })
        .collect()
}

/// Axis of the timing overview.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Timeline {
    /// The right end of the axis.
    pub(crate) scale_ms: f64,
    /// Labelled positions, from 0 up to `scale_ms`.
    pub(crate) ticks: Vec<u16>,
    /// When the default device is heard: the line the others align to.
    pub(crate) reference_ms: f64,
}

/// An axis that fits every lane with room to spare, in round steps.
pub(crate) fn timeline(lanes: &[Lane]) -> Timeline {
    let latest = lanes.iter().map(|l| l.plays_ms).fold(0.0, f64::max);
    let step: u16 = if latest > 230.0 { 100 } else { 50 };
    let steps = ((latest + 20.0) / f64::from(step)).ceil().max(2.0);
    // At most a few dozen steps: delays are capped at 500 ms.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks: Vec<u16> = (0..=steps as u16).map(|i| i * step).collect();
    Timeline {
        scale_ms: steps * f64::from(step),
        ticks,
        reference_ms: lanes
            .iter()
            .find(|l| l.is_source)
            .map_or(0.0, |l| l.plays_ms),
    }
}

/// Whether a device name suggests something worn (shown with a headphones icon).
pub(crate) fn looks_worn(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "headphone",
        "headset",
        "earbud",
        "buds",
        "airpods",
        "hands-free",
    ]
    .iter()
    .any(|word| name.contains(word))
}

/// How many playing devices are measured to be out of sync.
pub(crate) fn out_of_sync(rows: &[DeviceRow]) -> usize {
    rows.iter()
        .filter(|r| r.status.starts_with("Plays "))
        .count()
}

/// The info bar: a state word and what is happening.
pub(crate) fn status_line(settings: &Settings, status: &EngineStatus) -> (&'static str, String) {
    if !status.running {
        return ("Off", "press Play on multiple devices to start".into());
    }
    if status.capture == Some(StreamState::Failed) {
        return ("Active", "the audio source is unavailable; retrying".into());
    }
    let Some(source) = &status.source else {
        return ("Active", "waiting for devices".into());
    };
    let source_name = settings
        .device(source)
        .map_or_else(|| source.to_string(), |d| d.name.clone());
    let playing = status
        .outputs
        .iter()
        .filter(|o| o.state == StreamState::Running)
        .count();
    let devices = if playing == 1 { "device" } else { "devices" };
    let line = format!("playing from {source_name} on {playing} {devices}");
    let dropouts: u64 = status.outputs.iter().map(|o| o.underruns).sum();
    if dropouts > 0 {
        ("Active", format!("{line}, {dropouts} dropouts"))
    } else {
        ("Active", line)
    }
}

/// The info bar's color.
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
    use stereo_bt_core::{Delay, DeviceInfo};
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

    /// A measures 76 ms itself; B 35 ms; C 52 ms (all without delay).
    fn measurement() -> Measurement {
        Measurement::from_latencies(
            "Mic",
            &[
                DeviceLatency {
                    id: id("A"),
                    is_source: true,
                    measured_ms: 76.0,
                    delay: Delay::ZERO,
                },
                DeviceLatency {
                    id: id("B"),
                    is_source: false,
                    measured_ms: 45.0,
                    delay: Delay::from_millis(10).unwrap(),
                },
                DeviceLatency {
                    id: id("C"),
                    is_source: false,
                    measured_ms: 52.0,
                    delay: Delay::ZERO,
                },
            ],
        )
    }

    #[test]
    fn rows_follow_windows_device_order_and_mark_the_source() {
        let (settings, status) = setup(&[]);

        let rows = device_rows(&settings, &status, None);

        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Device A", "Device B", "Device C"]);
        assert!(rows[0].is_source);
        assert_eq!(rows[0].status, "The others are timed to this device");
        assert_eq!(rows[1].status, "Off");
        assert!(!rows[0].can_make_default);
        assert!(rows[1].can_make_default);
    }

    #[test]
    fn source_checkbox_shows_whether_it_keeps_playing() {
        let (mut settings, status) = setup(&[]);
        assert!(device_rows(&settings, &status, None)[0].checked);

        settings.include_default = false;

        let rows = device_rows(&settings, &status, None);
        assert!(!rows[0].checked);
        assert!(rows[0].status.starts_with("Muted while playing"));
    }

    #[test]
    fn measured_devices_say_whether_they_are_in_sync() {
        let (mut settings, status) = setup(&["B", "C"]);
        settings.device_mut(&id("B")).unwrap().delay = Delay::from_millis(41).unwrap();
        settings.device_mut(&id("C")).unwrap().delay = Delay::from_millis(12).unwrap();

        let rows = device_rows(&settings, &status, Some(&measurement()));

        assert_eq!(rows[1].status, "In sync");
        assert_eq!(rows[1].tone, Tone::Good);
        assert_eq!(rows[2].status, "Plays 12 ms early");
        assert_eq!(rows[2].tone, Tone::Caution);
        assert_eq!(out_of_sync(&rows), 1);
    }

    #[test]
    fn unmeasured_device_says_so() {
        let (settings, status) = setup(&["B"]);

        assert_eq!(
            device_rows(&settings, &status, None)[1].status,
            "Not measured yet"
        );
    }

    #[test]
    fn measurement_with_another_default_device_no_longer_counts() {
        let (settings, mut status) = setup(&["B"]);
        status.devices = vec![info("A", false), info("B", false), info("C", true)];

        let rows = device_rows(&settings, &status, Some(&measurement()));

        assert_eq!(rows[1].status, "Not measured yet");
        assert!(lanes(&settings, &status, Some(&measurement())).is_empty());
    }

    #[test]
    fn f6_disconnected_enabled_device_is_shown_with_a_caution() {
        let (settings, mut status) = setup(&["C"]);
        status.devices.retain(|d| d.id != id("C"));

        let rows = device_rows(&settings, &status, None);

        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[2].status,
            "Disconnected; plays again when it reconnects"
        );
        assert_eq!(rows[2].tone, Tone::Caution);
    }

    #[test]
    fn disconnected_disabled_device_is_hidden() {
        let (settings, mut status) = setup(&[]);
        status.devices.retain(|d| d.id != id("C"));

        assert_eq!(device_rows(&settings, &status, None).len(), 2);
    }

    #[test]
    fn failed_device_shows_its_error() {
        let (settings, mut status) = setup(&["B"]);
        running(&mut status, &[("B", StreamState::Failed, 0)]);

        let rows = device_rows(&settings, &status, None);

        assert_eq!(rows[1].status, "Device error, retrying: device unplugged");
        assert_eq!(rows[1].tone, Tone::Error);
    }

    #[test]
    fn lanes_show_when_each_playing_device_is_heard() {
        let (mut settings, status) = setup(&["B"]);
        settings.device_mut(&id("B")).unwrap().delay = Delay::from_millis(41).unwrap();

        let lanes = lanes(&settings, &status, Some(&measurement()));

        assert_eq!(lanes.len(), 2, "C is off");
        assert!(lanes[0].is_source);
        assert!((lanes[0].plays_ms - 76.0).abs() < 1e-9);
        assert!((lanes[1].own_ms - 35.0).abs() < 1e-9);
        assert!((lanes[1].plays_ms - 76.0).abs() < 1e-9);
        assert!(lanes[1].in_sync);
    }

    #[test]
    fn timeline_fits_the_latest_lane_in_round_steps() {
        let (mut settings, status) = setup(&["B"]);
        settings.device_mut(&id("B")).unwrap().delay = Delay::from_millis(41).unwrap();
        let lanes = lanes(&settings, &status, Some(&measurement()));

        let axis = timeline(&lanes);

        assert_eq!(axis.ticks, [0, 50, 100]);
        assert!((axis.scale_ms - 100.0).abs() < 1e-9);
        assert!((axis.reference_ms - 76.0).abs() < 1e-9);
    }

    #[test]
    fn timeline_uses_wider_steps_for_long_delays() {
        let lane = Lane {
            name: "Slow".into(),
            own_ms: 240.0,
            delay_ms: 0,
            plays_ms: 240.0,
            is_source: true,
            in_sync: true,
        };

        assert_eq!(timeline(&[lane]).ticks, [0, 100, 200, 300]);
    }

    #[test]
    fn headphones_are_recognised_by_name() {
        assert!(looks_worn("Headphones (JBL GO)"));
        assert!(looks_worn("Headset (Galaxy Buds2)"));
        assert!(!looks_worn("Speakers (Realtek(R) Audio)"));
    }

    #[test]
    fn status_line_says_off_or_active() {
        let (settings, mut status) = setup(&["B"]);
        assert_eq!(
            status_line(&settings, &status),
            ("Off", "press Play on multiple devices to start".to_owned())
        );

        running(&mut status, &[("B", StreamState::Running, 3)]);

        assert_eq!(
            status_line(&settings, &status),
            (
                "Active",
                "playing from Device A on 1 device, 3 dropouts".to_owned()
            )
        );
        assert_eq!(status_kind(&status), StatusKind::Streaming);
    }

    #[test]
    fn failed_capture_is_shown_as_retrying() {
        let (settings, mut status) = setup(&["B"]);
        running(&mut status, &[("B", StreamState::Running, 0)]);
        status.capture = Some(StreamState::Failed);

        assert_eq!(
            status_line(&settings, &status).1,
            "the audio source is unavailable; retrying"
        );
        assert_eq!(status_kind(&status), StatusKind::Waiting);
    }
}
