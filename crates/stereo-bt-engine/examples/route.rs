//! Manual hardware check: route system audio to one or more devices.
//!
//! ```text
//! cargo run -p stereo-bt-engine --example route              # list devices
//! cargo run -p stereo-bt-engine --example route -- 30 <index>[:l|r|m][:v<volume%>][:delay_ms] ...
//! ```
//!
//! The default output device cannot be a target: it is the capture source,
//! and rendering into it would feed our own output back into the capture.

use std::error::Error;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    use stereo_bt_core::{ChannelMode, Delay, Volume};
    use stereo_bt_engine::{OutputConfig, Session, WasapiBackend};
    use stereo_bt_wasapi::{ComGuard, STREAM_SAMPLE_RATE, list_output_devices};

    let devices = {
        let _com = ComGuard::init_mta()?;
        list_output_devices()?
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((seconds, targets)) = args.split_first() else {
        for (index, device) in devices.iter().enumerate() {
            let marker = if device.is_default { " (default)" } else { "" };
            println!("{index}: {}{marker}", device.name);
        }
        return Ok(());
    };

    let mut configs = Vec::new();
    for target in targets {
        let mut parts = target.split(':');
        let index: usize = parts.next().unwrap_or_default().parse()?;
        let device = devices.get(index).ok_or("no device with that index")?;
        if device.is_default {
            return Err("the default device is the capture source; pick another".into());
        }
        let mut config = OutputConfig::new(device.id.clone());
        for part in parts {
            match part {
                "l" => config.channel_mode = ChannelMode::LeftOnly,
                "r" => config.channel_mode = ChannelMode::RightOnly,
                "m" => config.channel_mode = ChannelMode::MonoMix,
                volume if volume.starts_with('v') => {
                    config.volume = Volume::from_percent(volume[1..].parse()?)?;
                }
                millis => config.delay = Delay::from_millis(millis.parse()?)?,
            }
        }
        println!(
            "-> {} {:?} {} ms",
            device.name,
            config.channel_mode,
            config.delay.as_millis()
        );
        configs.push(config);
    }

    let session = Session::start(&Arc::new(WasapiBackend), STREAM_SAMPLE_RATE, &configs)?;
    for _ in 0..seconds.parse::<u32>()? {
        thread::sleep(Duration::from_secs(1));
        println!("capture {:?}", session.capture_state());
        for status in session.output_status() {
            println!(
                "  {:?} underruns {} overruns {} trims {}",
                status.state, status.underruns, status.overruns, status.trims
            );
        }
    }
    session.stop();
    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn Error>> {
    Err("Windows only".into())
}
