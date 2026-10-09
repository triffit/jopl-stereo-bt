//! Test doubles for the backend, platform and mute journal.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use stereo_bt_core::{DeviceId, DeviceInfo, Frame, MutedApp, SampleRate, SystemVolume};

use crate::{Backend, CaptureStream, Platform, RenderSink, RestoreJournal};

#[derive(Debug)]
pub(crate) struct FakeError;

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fake error")
    }
}

impl std::error::Error for FakeError {}

/// Captures silence and discards rendered audio; devices listed in
/// `World::failing` fail to open and fail while playing.
pub(crate) struct SilentBackend {
    world: SharedWorld,
}

impl SilentBackend {
    pub(crate) fn new(world: &SharedWorld) -> Self {
        Self {
            world: Arc::clone(world),
        }
    }
}

pub(crate) struct SilentCapture {
    block: [Frame; 480],
    world: SharedWorld,
}

pub(crate) struct DiscardRender {
    id: DeviceId,
    world: SharedWorld,
}

impl Backend for SilentBackend {
    type Error = FakeError;
    type ThreadGuard = ();
    type Capture = SilentCapture;
    type Render = DiscardRender;

    fn enter_thread(&self) -> Result<(), FakeError> {
        Ok(())
    }

    fn open_capture(&self) -> Result<SilentCapture, FakeError> {
        if lock(&self.world).capture_failing {
            return Err(FakeError);
        }
        Ok(SilentCapture {
            block: [Frame::SILENCE; 480],
            world: Arc::clone(&self.world),
        })
    }

    fn open_render(&self, id: &DeviceId) -> Result<DiscardRender, FakeError> {
        let mut world = lock(&self.world);
        world.render_opens.push(id.clone());
        if world.failing.contains(id) {
            return Err(FakeError);
        }
        Ok(DiscardRender {
            id: id.clone(),
            world: Arc::clone(&self.world),
        })
    }
}

impl CaptureStream for SilentCapture {
    type Error = FakeError;

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn read(&mut self, _timeout_ms: u32) -> Result<&[Frame], FakeError> {
        thread::sleep(Duration::from_millis(5));
        if lock(&self.world).capture_failing {
            return Err(FakeError);
        }
        Ok(&self.block)
    }
}

impl RenderSink for DiscardRender {
    type Error = FakeError;

    fn buffer_frames(&self) -> usize {
        480
    }

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn wait(&mut self, _timeout_ms: u32) -> bool {
        thread::sleep(Duration::from_millis(5));
        true
    }

    fn writable_frames(&mut self) -> Result<usize, FakeError> {
        Ok(480)
    }

    fn write(&mut self, _frames: &[Frame]) -> Result<(), FakeError> {
        if lock(&self.world).failing.contains(&self.id) {
            return Err(FakeError);
        }
        Ok(())
    }
}

/// The simulated Windows device world, shared with the test.
#[derive(Debug, Default)]
pub(crate) struct World {
    pub(crate) default: Option<DeviceId>,
    pub(crate) active: Vec<DeviceId>,
    pub(crate) muted: HashSet<DeviceId>,
    /// Devices whose streams fail to open and fail while playing.
    pub(crate) failing: HashSet<DeviceId>,
    /// Whether the capture stream fails.
    pub(crate) capture_failing: bool,
    /// Every render stream opened, in order.
    pub(crate) render_opens: Vec<DeviceId>,
    /// Windows volume per device; full volume when absent.
    pub(crate) volumes: HashMap<DeviceId, SystemVolume>,
    /// Apps muted in the volume mixer, per device.
    pub(crate) muted_apps: Vec<(DeviceId, MutedApp)>,
}

/// A world shared between the test and the platform (possibly on another thread).
pub(crate) type SharedWorld = Arc<Mutex<World>>;

/// Locks a shared fake, ignoring poisoning from an earlier failed assertion.
pub(crate) fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct FakePlatform {
    backend: Arc<SilentBackend>,
    world: SharedWorld,
}

impl FakePlatform {
    pub(crate) fn new(world: &SharedWorld) -> Self {
        Self {
            backend: Arc::new(SilentBackend {
                world: Arc::clone(world),
            }),
            world: Arc::clone(world),
        }
    }
}

impl Platform for FakePlatform {
    type Backend = SilentBackend;
    type Error = FakeError;

    fn backend(&self) -> &Arc<SilentBackend> {
        &self.backend
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::HZ_48000
    }

    fn outputs(&self) -> Result<Vec<DeviceInfo>, FakeError> {
        let world = lock(&self.world);
        Ok(world
            .active
            .iter()
            .map(|id| DeviceInfo {
                id: id.clone(),
                name: id.to_string(),
                is_default: world.default.as_ref() == Some(id),
            })
            .collect())
    }

    fn system_volume(&self, id: &DeviceId) -> Result<SystemVolume, FakeError> {
        let world = lock(&self.world);
        let volume = world.volumes.get(id).copied().unwrap_or(SystemVolume::FULL);
        // Mirror Windows: the mute flag is part of the endpoint volume.
        Ok(SystemVolume::new(
            volume_db(volume),
            f32::from(volume.percent()) / 100.0,
            volume.is_muted() || world.muted.contains(id),
        ))
    }

    fn set_volume(&self, id: &DeviceId, scalar: f32) -> Result<(), FakeError> {
        let mut world = lock(&self.world);
        if !world.active.contains(id) {
            return Err(FakeError);
        }
        let level_db = if scalar > 0.0 {
            20.0 * scalar.log10()
        } else {
            -96.0
        };
        world
            .volumes
            .insert(id.clone(), SystemVolume::new(level_db, scalar, false));
        Ok(())
    }

    fn muted_apps(&self, id: &DeviceId) -> Result<Vec<MutedApp>, FakeError> {
        Ok(lock(&self.world)
            .muted_apps
            .iter()
            .filter(|(device, _)| device == id)
            .map(|(_, app)| app.clone())
            .collect())
    }

    fn unmute_app(&self, id: &DeviceId, pid: u32) -> Result<(), FakeError> {
        lock(&self.world)
            .muted_apps
            .retain(|(device, app)| !(device == id && app.pid == pid));
        Ok(())
    }

    fn set_default_output(&self, id: &DeviceId) -> Result<(), FakeError> {
        let mut world = lock(&self.world);
        if !world.active.contains(id) {
            return Err(FakeError);
        }
        world.default = Some(id.clone());
        Ok(())
    }

    fn is_muted(&self, id: &DeviceId) -> Result<bool, FakeError> {
        Ok(lock(&self.world).muted.contains(id))
    }

    fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<(), FakeError> {
        let mut world = lock(&self.world);
        if !world.active.contains(id) {
            return Err(FakeError);
        }
        if muted {
            world.muted.insert(id.clone());
        } else {
            world.muted.remove(id);
        }
        Ok(())
    }
}

/// Recorded original volumes, shared with the test.
pub(crate) type VolumeRecord = Arc<Mutex<Vec<(DeviceId, f32)>>>;

/// In-memory journal (mutes, volumes), shared with the test.
pub(crate) struct FakeJournal(
    pub(crate) Arc<Mutex<Vec<DeviceId>>>,
    pub(crate) VolumeRecord,
);

impl RestoreJournal for FakeJournal {
    fn load_mutes(&self) -> io::Result<Vec<DeviceId>> {
        Ok(lock(&self.0).clone())
    }

    fn save_mutes(&self, muted: &[DeviceId]) -> io::Result<()> {
        *lock(&self.0) = muted.to_vec();
        Ok(())
    }

    fn load_volumes(&self) -> io::Result<Vec<(DeviceId, f32)>> {
        Ok(lock(&self.1).clone())
    }

    fn save_volumes(&self, volumes: &[(DeviceId, f32)]) -> io::Result<()> {
        *lock(&self.1) = volumes.to_vec();
        Ok(())
    }
}

/// The decibel level of a fake volume, recovered from its amplitude.
fn volume_db(volume: SystemVolume) -> f32 {
    let amplitude = volume.unmuted().amplitude();
    if amplitude <= 0.0 {
        -96.0
    } else {
        20.0 * amplitude.log10()
    }
}
