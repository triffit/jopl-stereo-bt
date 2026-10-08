//! Value types serialise as plain values and reject invalid input when loaded
//! (settings files are untrusted input; PRODUCT.md F8).

use serde::{Deserialize, Serialize};
use stereo_bt_core::{ChannelMode, Delay, DeviceId, Volume};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Doc {
    id: DeviceId,
    delay: Delay,
    volume: Volume,
    mode: ChannelMode,
}

#[test]
fn values_round_trip_as_plain_toml() {
    let doc = Doc {
        id: DeviceId::new("{0.0.0.00000000}.{abc}").unwrap(),
        delay: Delay::from_millis(120).unwrap(),
        volume: Volume::from_percent(80).unwrap(),
        mode: ChannelMode::LeftOnly,
    };

    let text = toml::to_string(&doc).unwrap();

    assert!(text.contains("delay = 120"));
    assert!(text.contains("volume = 80"));
    assert!(text.contains("mode = \"left-only\""));
    assert_eq!(toml::from_str::<Doc>(&text).unwrap(), doc);
}

#[test]
fn out_of_range_delay_is_rejected_when_loading() {
    let text = "id = \"x\"\ndelay = 9000\nvolume = 50\nmode = \"stereo\"\n";

    assert!(toml::from_str::<Doc>(text).is_err());
}

#[test]
fn out_of_range_volume_is_rejected_when_loading() {
    let text = "id = \"x\"\ndelay = 0\nvolume = 101\nmode = \"stereo\"\n";

    assert!(toml::from_str::<Doc>(text).is_err());
}

#[test]
fn empty_device_id_is_rejected_when_loading() {
    let text = "id = \"\"\ndelay = 0\nvolume = 50\nmode = \"stereo\"\n";

    assert!(toml::from_str::<Doc>(text).is_err());
}
