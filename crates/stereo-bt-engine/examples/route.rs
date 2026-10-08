//! Manual hardware check: route system audio through the full engine.
//!
//! ```text
//! cargo run -p stereo-bt-engine --example route              # list devices
//! cargo run -p stereo-bt-engine --example route -- 30 [mute] <index>[:l|r|m][:v<volume%>][:delay_ms] ...
//! ```
//!
//! The default output device is the capture source. Without `mute` it keeps
//! playing natively (include-default mode); with `mute` it is muted while
//! streaming and restored afterwards. Selected devices that are the current
//! default are skipped automatically. Plug devices in or out, or change the
//! default device, while it runs to exercise hot-plug handling.

use std::error::Error;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    use stereo_bt_core::{ChannelMode, Delay, Volume};
    use stereo_bt_engine::{Engine, EngineConfig, FileMuteJournal, OutputConfig};
    use stereo_bt_wasapi::{ComGuard, list_output_devices};

    let devices = {
        let _com = ComGuard::init_mta()?;
        list_output_devices()?
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((seconds, mut targets)) = args.split_first() else {
        for (index, device) in devices.iter().enumerate() {
            let marker = if device.is_default { " (default)" } else { "" };
            println!("{index}: {}{marker}", device.name);
        }
        return Ok(());
    };
    let mute = targets.first().is_some_and(|t| t == "mute");
    if mute {
        targets = &targets[1..];
    }

    let mut config = EngineConfig {
        outputs: Vec::new(),
        include_default: !mute,
    };
    for target in targets {
        let mut parts = target.split(':');
        let index: usize = parts.next().unwrap_or_default().parse()?;
        let device = devices.get(index).ok_or("no device with that index")?;
        let mut output = OutputConfig::new(device.id.clone());
        for part in parts {
            match part {
                "l" => output.channel_mode = ChannelMode::LeftOnly,
                "r" => output.channel_mode = ChannelMode::RightOnly,
                "m" => output.channel_mode = ChannelMode::MonoMix,
                volume if volume.starts_with('v') => {
                    output.volume = Volume::from_percent(volume[1..].parse()?)?;
                }
                millis => output.delay = Delay::from_millis(millis.parse()?)?,
            }
        }
        println!(
            "-> {} {:?} {} ms {} %",
            device.name,
            output.channel_mode,
            output.delay.as_millis(),
            output.volume.as_percent()
        );
        config.outputs.push(output);
    }

    let journal = FileMuteJournal::new(std::env::temp_dir().join("stereo-bt-route-mutes.txt"));
    let engine = Engine::spawn(journal)?;
    engine.start(config)?;
    let name = |id: &stereo_bt_core::DeviceId| {
        devices
            .iter()
            .find(|d| &d.id == id)
            .map_or_else(|| id.to_string(), |d| d.name.clone())
    };
    for _ in 0..seconds.parse::<u32>()? {
        thread::sleep(Duration::from_secs(1));
        let status = engine.status();
        println!(
            "source {} | muted {} | capture {:?} | sessions {}",
            status.source.as_ref().map_or_else(|| "-".into(), name),
            status.muted.len(),
            status.capture,
            status.session_starts
        );
        for output in &status.outputs {
            println!(
                "  {} {:?} underruns {} overruns {} trims {}",
                name(&output.id),
                output.state,
                output.underruns,
                output.overruns,
                output.trims
            );
        }
        if let Some(error) = engine.last_error() {
            println!("  last error: {error}");
        }
    }
    engine.stop()?;
    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn Error>> {
    Err("Windows only".into())
}
