//! Battery levels of Bluetooth audio devices, as Windows shows them in
//! Settings. Read through the Windows device API (`WinRT`): no `unsafe`.

use stereo_bt_core::DeviceId;
use wasapi::WasapiError;
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Enumeration::Pnp::{PnpObject, PnpObjectType};
use windows::Foundation::IReference;
use windows::Media::Devices::MediaDevice;
use windows_collections::{IIterable, IMapView};
use windows_core::{GUID, HSTRING, IInspectable, Interface};

use crate::Error;

/// The Bluetooth battery level (0..=100) Windows keeps for a device that
/// reports one, e.g. through the hands-free profile.
const BATTERY_KEY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// Links the parts of one physical device (Bluetooth node, audio endpoints).
const CONTAINER_KEY: &str = "System.Devices.ContainerId";

/// The battery level of every device in `known` that reports one.
///
/// Devices without a battery level (wired, or Bluetooth devices that do not
/// report it) are left out.
///
/// # Errors
///
/// Returns [`Error`] when Windows cannot be queried.
pub fn battery_levels(known: &[DeviceId]) -> Result<Vec<(DeviceId, u8)>, Error> {
    let by_container = battery_by_container()?;
    if by_container.is_empty() {
        return Ok(Vec::new());
    }
    let requested: IIterable<HSTRING> = vec![HSTRING::from(CONTAINER_KEY)].into();
    let endpoints = DeviceInformation::FindAllAsyncAqsFilterAndAdditionalProperties(
        &MediaDevice::GetAudioRenderSelector().map_err(windows_error)?,
        &requested,
    )
    .and_then(|operation| operation.join())
    .map_err(windows_error)?;

    let mut levels = Vec::new();
    for endpoint in &endpoints {
        let Ok(interface_id) = endpoint.Id() else {
            continue;
        };
        let Some(id) = known_endpoint(&interface_id.to_string_lossy(), known) else {
            continue;
        };
        let container = endpoint
            .Properties()
            .ok()
            .and_then(|p| guid(&p, CONTAINER_KEY));
        if let Some(&(_, level)) = by_container.iter().find(|(c, _)| Some(*c) == container) {
            levels.push((id, level));
        }
    }
    Ok(levels)
}

/// Battery levels per physical device (container).
fn battery_by_container() -> Result<Vec<(GUID, u8)>, Error> {
    let requested: IIterable<HSTRING> =
        vec![HSTRING::from(BATTERY_KEY), HSTRING::from(CONTAINER_KEY)].into();
    // AQS cannot filter on this unnamed property, so filter here.
    let devices = PnpObject::FindAllAsync(PnpObjectType::Device, &requested)
        .and_then(|operation| operation.join())
        .map_err(windows_error)?;
    let mut levels = Vec::new();
    for device in &devices {
        let Ok(properties) = device.Properties() else {
            continue;
        };
        if let (Some(level), Some(container)) = (
            byte(&properties, BATTERY_KEY),
            guid(&properties, CONTAINER_KEY),
        ) {
            levels.push((container, level.min(100)));
        }
    }
    Ok(levels)
}

fn lookup(properties: &IMapView<HSTRING, IInspectable>, key: &str) -> Option<IInspectable> {
    properties.Lookup(&HSTRING::from(key)).ok()
}

fn byte(properties: &IMapView<HSTRING, IInspectable>, key: &str) -> Option<u8> {
    lookup(properties, key)?
        .cast::<IReference<u8>>()
        .ok()?
        .Value()
        .ok()
}

fn guid(properties: &IMapView<HSTRING, IInspectable>, key: &str) -> Option<GUID> {
    lookup(properties, key)?
        .cast::<IReference<GUID>>()
        .ok()?
        .Value()
        .ok()
}

fn windows_error(error: windows_core::Error) -> Error {
    Error::Audio(WasapiError::Windows(error))
}

/// The known endpoint behind an audio interface id such as
/// `\\?\SWD#MMDEVAPI#{0.0.0.00000000}.{guid}#{e6327cad-…}`.
fn known_endpoint(interface_id: &str, known: &[DeviceId]) -> Option<DeviceId> {
    let endpoint = interface_id.split('#').nth(2)?;
    known
        .iter()
        .find(|id| id.as_str().eq_ignore_ascii_case(endpoint))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "{0.0.0.00000000}.{a1b2c3d4-0000-0000-0000-000000000001}";

    #[test]
    fn endpoint_is_found_in_the_interface_id_ignoring_case() {
        let known = [DeviceId::new(ENDPOINT).unwrap()];
        let interface_id = format!(
            "\\\\?\\SWD#MMDEVAPI#{}#{{e6327cad-dcec-4949-ae8a-991e976a79d2}}",
            ENDPOINT.to_uppercase()
        );

        assert_eq!(
            known_endpoint(&interface_id, &known),
            Some(known[0].clone())
        );
    }

    #[test]
    fn unknown_or_malformed_interface_ids_match_nothing() {
        let known = [DeviceId::new(ENDPOINT).unwrap()];

        assert_eq!(
            known_endpoint("\\\\?\\SWD#MMDEVAPI#{other}#x", &known),
            None
        );
        assert_eq!(known_endpoint("garbage", &known), None);
    }

    #[test]
    #[ignore = "needs Windows with a Bluetooth device that reports its battery"]
    fn prints_battery_levels() {
        let _com = crate::ComGuard::init_mta().unwrap();
        let devices = crate::list_output_devices().unwrap();
        let known: Vec<DeviceId> = devices.iter().map(|d| d.id.clone()).collect();

        let started = std::time::Instant::now();
        let levels = battery_levels(&known).unwrap();
        println!("query took {:?}", started.elapsed());

        for (id, level) in &levels {
            let name = devices
                .iter()
                .find(|d| &d.id == id)
                .map(|d| d.name.as_str());
            println!("{name:?}: {level} %");
        }
        println!("by container: {:?}", battery_by_container().unwrap());
    }
}
