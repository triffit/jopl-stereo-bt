//! Acceptance tests for the platform-independent device model.

use stereo_bt_core::{DeviceId, Error};

#[test]
fn device_id_keeps_the_endpoint_id_string() {
    let raw = "{0.0.0.00000000}.{6b2a3c7e-1111-2222-3333-444455556666}";

    let id = DeviceId::new(raw).unwrap();

    assert_eq!(id.as_str(), raw);
}

#[test]
fn empty_device_id_is_rejected() {
    assert_eq!(DeviceId::new(""), Err(Error::InvalidDeviceId));
}

#[test]
fn overlong_device_id_is_rejected() {
    let raw = "x".repeat(DeviceId::MAX_LEN + 1);

    assert_eq!(DeviceId::new(raw), Err(Error::InvalidDeviceId));
}

#[test]
fn device_id_with_control_characters_is_rejected() {
    assert_eq!(DeviceId::new("abc\0def"), Err(Error::InvalidDeviceId));
}
