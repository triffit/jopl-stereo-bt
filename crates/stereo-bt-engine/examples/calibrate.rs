//! Manual hardware check of the microphone measurement: streams to every
//! connected device, measures them, prints the recommendation and restores
//! the original default device. Plays audible test chirps.
//!
//! ```text
//! cargo run --release -p stereo-bt-engine --example calibrate [follow]
//! ```

use std::error::Error;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    use std::thread;
    use std::time::Duration;

    use stereo_bt_engine::{
        CalibrationRequest, CalibrationState, Engine, EngineConfig, FileRestoreJournal,
        OutputConfig,
    };

    let engine = Engine::spawn(FileRestoreJournal::in_dir(
        std::env::temp_dir().join("stereo-bt-calibrate"),
    ))?;
    // The engine thread lists devices shortly after starting.
    thread::sleep(Duration::from_millis(500));
    let devices = engine.status().devices;
    let original_default = devices.iter().find(|d| d.is_default).map(|d| d.id.clone());
    let name = |id: &stereo_bt_core::DeviceId| {
        devices
            .iter()
            .find(|d| &d.id == id)
            .map_or_else(|| id.to_string(), |d| d.name.clone())
    };
    let engine_config = |devices: &[stereo_bt_core::DeviceInfo]| EngineConfig {
        outputs: devices
            .iter()
            .map(|d| OutputConfig::new(d.id.clone()))
            .collect(),
        include_default: true,
        // `follow` raises target volumes to 100 % (restored afterwards).
        follow_system_volume: std::env::args().any(|a| a == "follow"),
    };
    engine.start(engine_config(&devices))?;
    thread::sleep(Duration::from_secs(2));

    // Pass 1 measures; pass 2 applies the suggested delays and measures
    // again to verify that every device then lines up with the default.
    let mut delays: Vec<(stereo_bt_core::DeviceId, stereo_bt_core::Delay)> = Vec::new();
    for pass in 1..=2 {
        println!("--- pass {pass}");
        engine.start_calibration(CalibrationRequest {
            delays: delays.clone(),
            include_default: true,
        })?;
        let mut last = String::new();
        let report = loop {
            thread::sleep(Duration::from_millis(200));
            match engine.calibration() {
                CalibrationState::Running(text) if text != last => {
                    println!("{text}");
                    last = text;
                }
                CalibrationState::Done(report) => break Ok(report),
                CalibrationState::Failed(message) => break Err(message),
                _ => {}
            }
        };
        let report = match report {
            Ok(report) => report,
            Err(message) => {
                println!("failed: {message}");
                break;
            }
        };
        println!("microphone: {}", report.microphone);
        for m in &report.measured {
            let role = if m.is_source { " (default)" } else { "" };
            println!(
                "  {}{role}: {:.1} ms with delay {} ms",
                name(&m.id),
                m.measured_ms,
                m.delay.as_millis()
            );
        }
        for id in &report.not_heard {
            println!("  {}: not heard", name(id));
        }
        println!("outcome: {:?}", report.outcome);
        let stereo_bt_core::CalibrationOutcome::Aligned { delays: suggested } = report.outcome
        else {
            break;
        };
        delays = suggested;
        let mut config = engine_config(&devices);
        for output in &mut config.outputs {
            if let Some((_, delay)) = delays.iter().find(|(id, _)| *id == output.id) {
                output.delay = *delay;
            }
        }
        engine.update(config)?;
        thread::sleep(Duration::from_secs(1));
    }

    engine.stop()?;
    if let Some(original) = original_default {
        engine.make_default(original)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn Error>> {
    Err("Windows only".into())
}
