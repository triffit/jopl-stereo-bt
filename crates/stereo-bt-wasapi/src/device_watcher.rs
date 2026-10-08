use std::sync::mpsc::Sender;

use stereo_bt_core::{DeviceEvent, DeviceId};
use wasapi::{
    DeviceEnumerator, DeviceEventCallbacks, DeviceEventRegistration, DeviceState, Direction, Role,
};

use crate::Error;

/// Forwards Windows audio device changes as [`DeviceEvent`]s (PRODUCT.md F6).
///
/// Events for capture endpoints (e.g. a headset microphone) are forwarded
/// too; consumers filter on the device identifiers they know.
/// Notifications stop when this value is dropped.
pub struct DeviceWatcher {
    _registration: DeviceEventRegistration,
}

impl DeviceWatcher {
    /// Starts forwarding device changes to `events`.
    ///
    /// Requires a [`crate::ComGuard`] on the calling thread. Events are sent
    /// from a Windows audio system thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the notification callback cannot be registered.
    pub fn start(events: &Sender<DeviceEvent>) -> Result<Self, Error> {
        let mut callbacks = DeviceEventCallbacks::new();

        let sender = events.clone();
        callbacks.set_device_state_callback(move |raw_id, state| {
            if let Ok(id) = DeviceId::new(raw_id) {
                let event = if state == DeviceState::Active {
                    DeviceEvent::Activated(id)
                } else {
                    DeviceEvent::Deactivated(id)
                };
                // A closed channel only means nobody listens any more.
                let _ = sender.send(event);
            }
        });

        let sender = events.clone();
        callbacks.set_device_removed_callback(move |raw_id| {
            if let Ok(id) = DeviceId::new(raw_id) {
                let _ = sender.send(DeviceEvent::Deactivated(id));
            }
        });

        let sender = events.clone();
        callbacks.set_default_device_callback(move |direction, role, raw_id| {
            if direction == Direction::Render && role == Role::Console {
                let id = raw_id.and_then(|raw| DeviceId::new(raw).ok());
                let _ = sender.send(DeviceEvent::DefaultChanged(id));
            }
        });

        let registration = DeviceEnumerator::new()?.register_notification_callback(callbacks)?;
        Ok(Self {
            _registration: registration,
        })
    }
}
